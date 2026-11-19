//! The lossless 2026 document (`docs/WRITING.md`): a `.skp` file held as
//! its header bytes plus every archive entry, with `model.dat` available
//! as an editable record tree. `open` then `write` reproduces the file
//! byte for byte — with the original DEFLATE streams while an entry is
//! untouched, and with the zlib-exact encoder once it changes — and
//! `model` reads the same [`Model`] as [`Model::parse`].

use crate::model::Error;
use crate::tree26::Record;
use crate::zip::{self, Entries, OutEntry};
use crate::Model;

/// The bytes before the archive of every observed 2026 file: the two
/// string records, then `VFF`, u16 8, u16 1, the u16 revision (§16.27;
/// 17 here, set by [`Document::blank`]) and the integrity word (§16.1).
const HEADER_MAGIC: [u8; 9] = [0x56, 0x46, 0x46, 0x08, 0x00, 0x01, 0x00, 0x11, 0x00];

/// One archive entry: its decoded content, and the original compressed
/// stream while the content is untouched.
#[derive(Debug, Clone)]
struct Entry {
    name: String,
    /// General-purpose flags: `0x6` for level-1 entries, `0x0` for stored.
    flags: u16,
    content: Vec<u8>,
    raw: Option<Vec<u8>>,
}

/// The flags (and so the zlib level) SketchUp uses for an entry name:
/// fastest (`0x6`, level 1) for `.dat` and `.xml`, normal (`0x0`, level 0)
/// for everything else.
pub(crate) fn flags_for(name: &str) -> u16 {
    match name
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("xml") if name.starts_with("environments/") => 0x0,
        Some("xml") if name.starts_with("hatchpatterns/") => 0x2,
        Some("dat" | "xml") => 0x6,
        _ => 0x0,
    }
}

fn level_of(flags: u16) -> u8 {
    match flags & 0x6 {
        0x6 => 1,
        0x2 => 9,
        _ => 0,
    }
}

/// Whether the flags name a zlib level the encoder does not reproduce:
/// `0x4`, which no file seen uses (`0x2`, level 9, is reproduced). Such an
/// entry is carried as the original stream.
fn unreproduced(flags: u16) -> bool {
    flags & 0x6 == 0x4
}

/// A SketchUp 2026 file, losslessly.
#[derive(Debug, Clone)]
pub struct Document {
    header: Vec<u8>,
    version: String,
    entries: Vec<Entry>,
    pub(crate) notes: Vec<String>,
    /// An entry was added, replaced or removed, so the header's integrity
    /// word (§16.1) no longer matches and [`write`](Self::write) computes
    /// it anew.
    edited: bool,
}

impl Document {
    /// Open a 2026 file's bytes. Refuses what the reader refuses: no
    /// archive, a missing `model.dat`, or an entry that fails its CRC-32.
    pub fn open(d: &[u8]) -> Result<Document, Error> {
        if crate::detect_container(d) != crate::Container::Zip {
            return Err(Error("not a 2026 .skp container".into()));
        }
        let (version, _) =
            crate::header_info(d).ok_or_else(|| Error("malformed .skp header".into()))?;
        let start = crate::ctx::zip_start(d)
            .ok_or_else(|| Error("no ZIP archive after the header".into()))?;
        let zip = zip::Archive::open(d, start).map_err(Error)?;
        let mut entries = Vec::with_capacity(zip.entries.len());
        for e in &zip.entries {
            let content = zip.read_entry(e).map_err(Error)?;
            // The original stream is reused only when it is DEFLATE, the
            // one method the writer declares; a stored entry (method 0)
            // is re-encoded from its content.
            let raw = (e.method() == 8).then(|| zip.raw(e).to_vec());
            entries.push(Entry {
                name: e.name.clone(),
                flags: e.flags(),
                content,
                raw,
            });
        }
        if !entries.iter().any(|e| e.name == "model.dat") {
            return Err(Error("2026 container without model.dat".into()));
        }
        Ok(Document {
            header: d[..start].to_vec(),
            version,
            entries,
            notes: Vec::new(),
            edited: false,
        })
    }

    /// [`open`](Self::open) from a path.
    pub fn read(path: impl AsRef<std::path::Path>) -> Result<Document, Error> {
        let d = std::fs::read(path).map_err(|e| Error(e.to_string()))?;
        Document::open(&d)
    }

    /// A new, empty document with the standard header for `version`
    /// (e.g. `{26.2.0}`) and `revision`, and no entries; the authoring
    /// path fills it.
    pub(crate) fn blank(version: &str, revision: crate::Revision) -> Document {
        let mut header = Vec::new();
        for s in ["SketchUp Model", version] {
            header.extend([0xFF, 0xFE, 0xFF, s.encode_utf16().count() as u8]);
            header.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
        }
        header.extend(HEADER_MAGIC);
        let at = header.len() - 2;
        header[at..].copy_from_slice(&revision.number().to_le_bytes());
        header.extend(crate::env::random_bytes());
        Document {
            header,
            version: version.to_string(),
            entries: Vec::new(),
            notes: Vec::new(),
            edited: true,
        }
    }

    /// The header's version string, e.g. `{26.2.0}`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The model revision (§16.27): `0x74` of `meta/meta.dat`, else the
    /// header's revision byte; `None` when the file holds neither. The
    /// value is the file's own, a revision newer than
    /// [`Revision::NEWEST`](crate::Revision::NEWEST) included (the
    /// [`model`](Self::model) reads such a file as the newest). Edits keep
    /// it: none of the in-place setters adds a feature of a newer revision.
    pub fn revision(&self) -> Option<crate::Revision> {
        self.entry("meta/meta.dat")
            .and_then(|m| crate::revision::read_meta(m).0.revision)
            .or_else(|| crate::revision::header_revision(&self.header).map(u16::from))
            .map(crate::Revision)
    }

    /// Anything the parse had to work around (a listed container that did
    /// not tile); empty on every corpus file.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Entry names in archive order.
    pub fn entry_names(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    /// An entry's decoded bytes.
    pub fn entry(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.content.as_slice())
    }

    /// Replace an entry's content, or add the entry. A new entry goes
    /// before `model.dat`, which SketchUp writes last.
    pub fn set_entry(&mut self, name: &str, content: Vec<u8>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.name == name) {
            if e.content != content {
                self.edited = true;
                e.content = content;
                e.raw = None;
                if unreproduced(e.flags) {
                    e.flags = flags_for(name);
                }
            }
            return;
        }
        self.edited = true;
        let e = Entry {
            name: name.to_string(),
            flags: flags_for(name),
            content,
            raw: None,
        };
        match self.entries.iter().position(|e| e.name == "model.dat") {
            Some(i) => self.entries.insert(i, e),
            None => self.entries.push(e),
        }
    }

    /// Remove an entry; `true` when it existed. Removing an entry before
    /// `meta/meta.dat` moves it off the third place, the one layout the
    /// integrity word is known for: [`write`](Self::write) then fails
    /// rather than keep a stale word.
    pub fn remove_entry(&mut self, name: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.name != name);
        let removed = before != self.entries.len();
        self.edited |= removed;
        removed
    }

    /// The `model.dat` record tree.
    pub fn tree(&mut self) -> Result<Record, Error> {
        let dat = self
            .entry("model.dat")
            .ok_or_else(|| Error("2026 container without model.dat".into()))?;
        let mut notes = Vec::new();
        let tree = Record::parse_model_dat(dat, &mut notes)?;
        self.notes = notes;
        Ok(tree)
    }

    /// Replace `model.dat` with the serialization of `root`.
    pub fn set_tree(&mut self, root: &Record) {
        self.set_entry("model.dat", root.to_bytes());
    }

    /// Forget every original compressed stream, so that [`write`](Self::write)
    /// re-encodes every entry from its content.
    ///
    /// An entry whose level the encoder does not reproduce (see
    /// [`carried_streams`](Self::carried_streams)) keeps its original
    /// stream: it has no other faithful encoding.
    pub fn repack(&mut self) {
        for e in &mut self.entries {
            if !unreproduced(e.flags) {
                e.raw = None;
            }
        }
    }

    /// The file's bytes. Fails only past the ZIP32 limits (an entry or
    /// the archive over 4 GiB, more than 65,535 entries).
    pub fn write(&self) -> Result<Vec<u8>, Error> {
        let mut streams: Vec<(u32, Vec<u8>)> = Vec::with_capacity(self.entries.len());
        for e in &self.entries {
            let crc = zip::crc32(&e.content);
            let compressed = match &e.raw {
                Some(raw) => raw.clone(),
                None => crate::deflate::compress(&e.content, level_of(e.flags)),
            };
            streams.push((crc, compressed));
        }
        let out_entries: Vec<OutEntry<'_>> = self
            .entries
            .iter()
            .zip(&streams)
            .map(|(e, (crc, c))| OutEntry {
                name: &e.name,
                flags: e.flags,
                crc32: *crc,
                size: e.content.len(),
                compressed: c,
            })
            .collect();
        let mut out = self.header.clone();
        if self.edited {
            let crcs: Vec<u32> = streams.iter().map(|(c, _)| *c).collect();
            match self.integrity_word(&crcs) {
                Some(w) => {
                    let at = out.len() - 4;
                    out[at..].copy_from_slice(&w.to_le_bytes());
                }
                // The word is known only for `meta/meta.dat` as the third
                // entry; keeping the header's old word would write a file
                // SketchUp reports as failing its integrity checks.
                None if self.entries.iter().any(|e| e.name == "meta/meta.dat") => {
                    return Err(Error(
                        "the integrity word is unknown for this entry layout (meta/meta.dat \
                         must be the third entry of an edited document)"
                            .into(),
                    ));
                }
                None => {}
            }
        }
        out.extend(zip::write_archive(&out_entries).map_err(Error)?);
        Ok(out)
    }

    /// The header's integrity word (§16.15): the CRC-32 of the first
    /// entry-listing bytes of `meta/meta.dat` (all but its last four)
    /// followed by a table of 32-bit words, one per entry — the even
    /// entries' first, then the odd — each the CRC-32 of the entry's
    /// content, less 42 for an odd entry, and for `meta/meta.dat` (always
    /// the third entry) a word that depends on the entry count and on the
    /// last four bytes of `meta.dat` (`integrity::meta_word`). `None`
    /// where the layout or the count is not one whose word is known.
    fn integrity_word(&self, crcs: &[u32]) -> Option<u32> {
        let n = self.entries.len();
        let meta = &self
            .entries
            .get(2)
            .filter(|e| e.name == "meta/meta.dat")?
            .content;
        let tail: [u8; 4] = meta.get(meta.len().checked_sub(4)?..)?.try_into().ok()?;
        let word = |i: usize| match i {
            2 => crate::integrity::meta_word(n.div_ceil(2), tail),
            i if i % 2 == 1 => Some(crcs[i].wrapping_sub(42)),
            i => Some(crcs[i]),
        };
        let mut table = Vec::with_capacity(4 * n);
        for i in (0..n).step_by(2).chain((1..n).step_by(2)) {
            table.extend(word(i)?.to_le_bytes());
        }
        let mut data = meta.get(..meta.len().checked_sub(4)?)?.to_vec();
        data.extend(table);
        Some(zip::crc32(&data))
    }

    /// [`write`](Self::write) to a path.
    pub fn write_to(&self, path: impl AsRef<std::path::Path>) -> Result<(), Error> {
        std::fs::write(path, self.write()?).map_err(|e| Error(e.to_string()))
    }

    /// Check the document against `file`, the bytes it was opened from:
    /// the header and the archive must write back as the file, and
    /// `model.dat` must read as a record tree (§16.2) that serializes to
    /// the entry it was read from, with every container tiling as
    /// records. An error names the first thing that does not fit.
    pub fn verify(&self, file: &[u8]) -> Result<(), Error> {
        let written = self.write()?;
        if written != file {
            let at = written
                .iter()
                .zip(file)
                .position(|(a, b)| a != b)
                .unwrap_or(written.len().min(file.len()));
            return Err(Error(format!(
                "the file does not reproduce from its archive: first difference at {at:#x}"
            )));
        }
        let dat = self
            .entry("model.dat")
            .ok_or_else(|| Error("2026 container without model.dat".into()))?;
        let mut notes = Vec::new();
        let tree = Record::parse_model_dat(dat, &mut notes)?;
        if let Some(note) = notes.first() {
            return Err(Error(note.clone()));
        }
        let back = tree.to_bytes();
        if back != dat {
            let at = back
                .iter()
                .zip(dat)
                .position(|(a, b)| a != b)
                .unwrap_or(back.len().min(dat.len()));
            return Err(Error(format!(
                "model.dat does not reproduce from its records: first difference at {at:#x}"
            )));
        }
        Ok(())
    }

    /// The entries whose compressed stream is not the one this encoder
    /// gives for their content: none in a file SketchUp saved. Such an
    /// entry reads the same; [`repack`](Self::repack) changes its bytes.
    pub fn foreign_streams(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| !unreproduced(e.flags))
            .filter(|e| match &e.raw {
                Some(raw) => *raw != crate::deflate::compress(&e.content, level_of(e.flags)),
                None => false,
            })
            .map(|e| e.name.as_str())
            .collect()
    }

    /// The entries compressed at a zlib level this encoder does not
    /// reproduce (flags `0x4`; none in any file seen, the hatch pattern
    /// definitions' level 9 being reproduced). They are written as the
    /// original stream, so a file that holds them still round trips; once
    /// the content is replaced the entry takes the flags its name gets.
    pub fn carried_streams(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| unreproduced(e.flags) && e.raw.is_some())
            .map(|e| e.name.as_str())
            .collect()
    }

    /// The file as text: the archive's entries, then the records of
    /// `model.dat` in stream order, children indented under their
    /// container. The analysis view of the container, for reading a file
    /// SketchUp wrote next to one written here. A short value is shown
    /// as its bytes, a long one by its length.
    pub fn records(&self) -> Result<String, Error> {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(out, "header {}: {} bytes", self.version, self.header.len());
        for e in &self.entries {
            let stream = match &e.raw {
                Some(raw) => format!("{} bytes compressed", raw.len()),
                None => "stored".to_string(),
            };
            let _ = writeln!(
                out,
                "entry {:?}: {} bytes, flags {:#x}, {stream}",
                e.name,
                e.content.len(),
                e.flags
            );
        }
        let dat = self
            .entry("model.dat")
            .ok_or_else(|| Error("2026 container without model.dat".into()))?;
        let mut notes = Vec::new();
        let tree = Record::parse_model_dat(dat, &mut notes)?;
        let _ = writeln!(out, "model.dat:");
        tree.describe_into(&mut out, 1);
        for n in notes {
            let _ = writeln!(out, "note: {n}");
        }
        Ok(out)
    }

    /// The document as a [`Model`] — the same decode as [`Model::parse`].
    pub fn model(&self) -> Result<Model, Error> {
        crate::read26::parse_entries(
            self,
            self.version.clone(),
            crate::revision::header_revision(&self.header),
        )
    }

    /// A new document from a model (`docs/WRITING.md`): every section
    /// SketchUp 2026 writes, filled from the model, with the archive
    /// entries the records refer to. The model may come from either
    /// container or from another program. The revision is the lowest
    /// that holds the content and no lower than the model's own
    /// ([`ArchiveTarget::ByContent`](crate::ArchiveTarget::ByContent)).
    pub fn from_model(model: &Model) -> Result<Document, Error> {
        crate::build26::from_model(model, crate::ArchiveTarget::ByContent)
    }

    /// [`from_model`](Self::from_model) with the save time and seed taken
    /// from `env` rather than the system: a reproducible file, or one
    /// written where there is no system clock.
    pub fn from_model_with(model: &Model, env: crate::WriteEnv) -> Result<Document, Error> {
        crate::env::with_env(env, || {
            crate::build26::from_model(model, crate::ArchiveTarget::ByContent)
        })
    }

    /// [`from_model`](Self::from_model) at the revision `target` names
    /// (§16.27): `ArchiveTarget::Revision(Revision::R17)` writes a file of
    /// SketchUp 2026 and the web app (`{26.2.0}`), leaving out what only
    /// revision 18 holds with a note for each kind of thing left out
    /// ([`notes`](Self::notes)); `Revision::R18` a file of SketchUp 2027
    /// (`{27.0.267}`). Any other revision is an error.
    pub fn from_model_for(model: &Model, target: crate::ArchiveTarget) -> Result<Document, Error> {
        crate::build26::from_model(model, target)
    }

    /// Put the entries in the order SketchUp writes them: `meta/`,
    /// hatch patterns, materials, watermarks, thumbnails, styles, scene thumbnails,
    /// classifications, then `model.dat` last.
    pub(crate) fn reorder_entries(&mut self) {
        let rank = |n: &str| -> u8 {
            if n == "model.dat" {
                9
            } else if n.starts_with("meta/") {
                0
            } else if n.starts_with("hatchpatterns/") {
                1
            } else if n.starts_with("materials/") {
                2
            } else if n.starts_with("watermarks/") {
                3
            } else if n.starts_with("thumbnails/") {
                4
            } else if n.starts_with("styles/") {
                5
            } else if n.starts_with("scene_thumbnails/") {
                6
            } else if n.starts_with("classifications/") {
                7
            } else {
                8
            }
        };
        self.entries.sort_by_key(|e| rank(&e.name));
    }
}

impl Entries for Document {
    fn read(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(self.entry(name).map(<[u8]>::to_vec))
    }
    fn list(&self) -> Vec<(String, usize)> {
        self.entries
            .iter()
            .map(|e| (e.name.clone(), e.content.len()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus_files() -> Vec<std::path::PathBuf> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let mut files: Vec<_> = std::fs::read_dir(root.join("2026"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("skp"))
            .collect();
        files.push(root.join("third-party/theater-2026.skp"));
        files.sort();
        files
    }

    #[test]
    fn open_write_is_identity_with_and_without_repack() {
        for p in corpus_files() {
            let d = std::fs::read(&p).unwrap();
            let mut doc = Document::open(&d).unwrap();
            let w = doc.write().unwrap();
            if w != d {
                let at = w
                    .iter()
                    .zip(&d)
                    .position(|(a, b)| a != b)
                    .unwrap_or(w.len().min(d.len()));
                panic!(
                    "{}: untouched copy differs at {at:#x} (ours {} bytes, file {} bytes): ours {:02x?} file {:02x?}",
                    p.display(), w.len(), d.len(),
                    &w[at.saturating_sub(4)..(at + 12).min(w.len())],
                    &d[at.saturating_sub(4)..(at + 12).min(d.len())]
                );
            }
            doc.repack();
            assert!(
                doc.write().unwrap() == d,
                "{}: re-encoded copy differs",
                p.display()
            );
            let tree = doc.tree().unwrap();
            doc.set_tree(&tree);
            assert!(
                doc.write().unwrap() == d,
                "{}: re-serialized tree differs",
                p.display()
            );
            assert!(doc.notes().is_empty(), "{}: {:?}", p.display(), doc.notes());
        }
    }

    #[test]
    fn document_model_equals_model_parse() {
        for p in corpus_files() {
            let d = std::fs::read(&p).unwrap();
            let doc = Document::open(&d).unwrap();
            let a = Model::parse(&d).unwrap();
            let b = doc.model().unwrap();
            assert_eq!(a.to_json(), b.to_json(), "{}", p.display());
        }
    }

    /// The integrity word recomputed from the entries is the one
    /// SketchUp wrote, in every file whose entry count has a known
    /// constant.
    #[test]
    fn the_integrity_word_is_reproduced() {
        let corpus = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let (mut n, mut skipped) = (0, 0);
        for dir in ["2026", "2027"] {
            for e in std::fs::read_dir(corpus.join(dir)).unwrap() {
                let p = e.unwrap().path();
                if p.extension().and_then(|x| x.to_str()) != Some("skp") {
                    continue;
                }
                let d = std::fs::read(&p).unwrap();
                let doc = Document::open(&d).unwrap();
                let crcs: Vec<u32> = doc.entries.iter().map(|e| zip::crc32(&e.content)).collect();
                match doc.integrity_word(&crcs) {
                    Some(w) => {
                        let at = doc.header.len() - 4;
                        let stored = u32::from_le_bytes(doc.header[at..].try_into().unwrap());
                        assert_eq!(w, stored, "{}", p.display());
                        n += 1;
                    }
                    None => skipped += 1,
                }
            }
        }
        assert!(n >= 150 && skipped == 0, "{n} reproduced, {skipped} not");
    }

    #[test]
    fn blank_header_matches_the_corpus_shape() {
        let doc = Document::blank("{26.2.0}", crate::Revision::R17);
        assert_eq!(doc.header.len(), 0x41);
        assert_eq!(&doc.header[0x34..0x3d], &HEADER_MAGIC);
        let r18 = Document::blank("{27.0.267}", crate::Revision::R18);
        assert_eq!(crate::revision::header_revision(&r18.header), Some(0x12));
        assert_eq!(&doc.header[0x20..0x24], &[0xFF, 0xFE, 0xFF, 8]);
        assert_eq!(
            &doc.header[0x24..0x34],
            "{26.2.0}"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    /// An archive entry stored uncompressed (method 0) is not SketchUp's
    /// layout, but it opens; the writer declares DEFLATE for every entry,
    /// so such an entry is re-encoded from its content rather than
    /// copied through as a stream that is not DEFLATE.
    #[test]
    fn stored_entries_are_reencoded_on_write() {
        let d = std::fs::read(&corpus_files()[0]).unwrap();
        let start = crate::ctx::zip_start(&d).unwrap();
        let dat = Document::open(&d)
            .unwrap()
            .entry("model.dat")
            .unwrap()
            .to_vec();
        let mut file = d[..start].to_vec();
        file.extend(crate::zip::tests::stored_archive(b"model.dat", &dat, b""));
        let doc = Document::open(&file).unwrap();
        let written = doc.write().unwrap();
        let again = Document::open(&written).unwrap();
        assert_eq!(again.entry("model.dat").unwrap(), &dat[..]);
        assert_eq!(
            again.model().unwrap().geometry.len(),
            doc.model().unwrap().geometry.len()
        );
    }
}
