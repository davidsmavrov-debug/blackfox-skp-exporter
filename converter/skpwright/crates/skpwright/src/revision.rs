//! The model revision of the 2026 container (spec §16.27): the u16
//! `0x74` of `meta/meta.dat`, repeated as the header byte before the
//! archive, and the save history `0x78` beside it. SketchUp 2027 opens a
//! file whose revision is at most its own (18) and refuses a higher one,
//! whatever the version strings say; the revision, not the version label,
//! is what a writer must get right.

use crate::read26::{records, Node};

/// A revision of the 2026 container's model format (`meta/meta.dat`
/// `0x74`). Revision 17 is what the web app writes in every observed file
/// (`{26.2.0}`), 18 what SketchUp 2027 writes (`{27.0.267}`); both are
/// read and written here. Other values are only read: a lower one as it
/// is, a higher one as 18 with a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub u16);

impl Revision {
    /// The revision of the web app's files (`{26.2.0}`).
    pub const R17: Revision = Revision(17);
    /// The revision of SketchUp 2027's files (`{27.0.267}`): the first to
    /// hold the features of spec §16.17 to §16.26.
    pub const R18: Revision = Revision(18);
    /// The newest revision this crate reads completely and writes.
    pub const NEWEST: Revision = Revision::R18;
    /// The revisions this crate writes, oldest first.
    pub const WRITTEN: [Revision; 2] = [Revision::R17, Revision::R18];

    /// The number stored in the file.
    pub fn number(self) -> u16 {
        self.0
    }

    /// The header version string a file of this revision is written
    /// with: `{26.2.0}` for 17, `{27.0.267}` for 18; `None` for a
    /// revision that is not written.
    pub fn version_label(self) -> Option<&'static str> {
        match self {
            Revision::R17 => Some("{26.2.0}"),
            Revision::R18 => Some("{27.0.267}"),
            _ => None,
        }
    }

    /// Whether the revision is one of SketchUp 2027's writer line (the
    /// meta fields, history entry and integrity constant of `{27.`).
    pub(crate) fn desktop(self) -> bool {
        self >= Revision::R18
    }
}

impl std::fmt::Display for Revision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which revision [`Model::to_archive`](crate::Model::to_archive) and
/// [`Document::from_model_for`](crate::Document::from_model_for) write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArchiveTarget {
    /// The lowest revision that holds the model's content
    /// ([`Model::min_revision`](crate::Model::min_revision)), and never
    /// lower than the revision the model was read with: 17 for a model
    /// that uses none of the features added after 2017, else 18.
    #[default]
    ByContent,
    /// Exactly this revision: 17 leaves out what only 18 holds, with a
    /// note for each kind of thing left out; 18 writes everything. Any
    /// other revision is an error.
    Revision(Revision),
}

/// One entry of a 2026 file's save history, oldest first: the records
/// `0x78 > 0xc8 > 0xc9 > 0xca` of `meta/meta.dat`. A save by the
/// application of the last entry updates that entry's time; a save by
/// another appends an entry (spec §16.27).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SaveEntry {
    /// `0xcb`: the application, e.g. `SketchUp Web`; `Pre SU 2021` for the
    /// entry a model made before the 2026 container starts with.
    pub app: String,
    /// `0xcc`: the save time, seconds since 1970.
    pub time: u32,
    /// `0xcd`: a u64 per application (undecoded; candidate: a build
    /// identifier). An entry carried over by a later save can change it.
    pub build: u64,
}

/// What `meta/meta.dat` says about the file's revision and history.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Meta {
    /// `0x74`, when the record is there and holds a u16.
    pub(crate) revision: Option<u16>,
    pub(crate) history: Vec<SaveEntry>,
}

/// Read the revision and the save history from the content of
/// `meta/meta.dat`. A missing or malformed record leaves its field empty
/// and is named in the second value (one line each), so a damaged
/// `meta.dat` is never silent.
pub(crate) fn read_meta(meta: &[u8]) -> (Meta, Vec<String>) {
    let mut out = Meta::default();
    let mut problems = Vec::new();
    let Some(top) = (match records(meta, 0) {
        Ok(r) => r.first().copied(),
        Err(e) => {
            problems.push(format!("meta.dat does not parse as records: {e}"));
            None
        }
    }) else {
        if problems.is_empty() {
            problems.push("meta.dat holds no records".into());
        }
        return (out, problems);
    };
    // Lenient: a damaged record deep in the tree (an overclaiming `0x78`)
    // must not hide the `0x74` that precedes it.
    let (top, damaged) = Node::of_lenient(top);
    if let Some(at) = damaged {
        problems.push(format!("a record of meta.dat is damaged at {at:#x}"));
    }
    match top.get(0x74) {
        None => problems.push("meta.dat has no model revision record (0x74)".into()),
        Some(r) => match <[u8; 2]>::try_from(r.data) {
            Ok(b) => out.revision = Some(u16::from_le_bytes(b)),
            Err(_) => problems.push(format!(
                "the model revision record (0x74) holds {} bytes, not 2",
                r.data.len()
            )),
        },
    }
    let list = match top.node(0x78) {
        Ok(Some(n)) => match n
            .node(0xC8)
            .and_then(|c| c.map(|c| c.node(0xC9)).transpose())
        {
            Ok(l) => l.flatten(),
            Err(e) => {
                problems.push(format!("the save history (0x78) is malformed: {e}"));
                None
            }
        },
        Ok(None) => None,
        Err(e) => {
            problems.push(format!("the save history (0x78) is malformed: {e}"));
            None
        }
    };
    let entries = list.as_ref().map(|l| l.nodes(0xCA));
    let entries = match entries {
        Some(Ok(v)) => v,
        Some(Err(e)) => {
            problems.push(format!("the save history (0x78) is malformed: {e}"));
            Vec::new()
        }
        None => Vec::new(),
    };
    for e in &entries {
        let time = e.get(0xCC).and_then(|r| <[u8; 4]>::try_from(r.data).ok());
        let build = e.get(0xCD).filter(|r| r.data.len() == 8);
        if time.is_none() || build.is_none() {
            problems.push(format!(
                "a save-history entry lacks a 4-byte time (0xcc) or an 8-byte build (0xcd): entry {}",
                out.history.len()
            ));
        }
        out.history.push(SaveEntry {
            app: crate::read26::text(e.get(0xCB)),
            time: time.map_or(0, u32::from_le_bytes),
            build: e.get(0xCD).map_or(0, |r| {
                let mut b = [0u8; 8];
                let n = r.data.len().min(8);
                b[..n].copy_from_slice(&r.data[..n]);
                u64::from_le_bytes(b)
            }),
        });
    }
    (out, problems)
}

/// The header's revision byte: the u16 after `VFF`, u16 8, u16 1 in the
/// 13 bytes before the archive (§16.1), or `None` when `header` (every
/// byte before the archive) does not end in that layout.
pub(crate) fn header_revision(header: &[u8]) -> Option<u8> {
    let tail = header.get(header.len().checked_sub(13)?..)?;
    (tail[..7] == [0x56, 0x46, 0x46, 0x08, 0x00, 0x01, 0x00] && tail[8] == 0).then_some(tail[7])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_written_meta_reads_back() {
        let history = [SaveEntry {
            app: "Pre SU 2021".into(),
            time: 7,
            build: 0x1234,
        }];
        for rev in Revision::WRITTEN {
            let m = crate::xml26::meta_dat(&[1; 16], "Inches", "x", 9, rev, &history);
            let (meta, problems) = read_meta(&m);
            assert!(problems.is_empty(), "{problems:?}");
            assert_eq!(meta.revision, Some(rev.number()));
            assert_eq!(meta.history.len(), 2);
            assert_eq!(meta.history[0], history[0]);
            assert_eq!(meta.history[1].time, 9);
        }
    }

    fn rec(tag: u16, body: &[u8]) -> Vec<u8> {
        let mut v = tag.to_le_bytes().to_vec();
        v.extend((body.len() as u32).to_le_bytes());
        v.extend(body);
        v
    }

    /// Odd `meta.dat` contents never panic; each damage is named, and the
    /// fields that survive are kept.
    #[test]
    fn odd_metas_are_reported_not_silent() {
        let rev = rec(0x74, &[18, 0]);
        let entry = |t: &[u8], b: &[u8]| {
            let mut e = rec(0xCB, b"app");
            e.extend(rec(0xCC, t));
            e.extend(rec(0xCD, b));
            rec(0xCA, &e)
        };
        let history = |e: Vec<u8>| rec(0x78, &rec(0xC8, &rec(0xC9, &e)));
        let top = |kids: Vec<u8>| rec(0x64, &kids);
        let cat = |a: &[Vec<u8>]| a.concat();
        let overclaim = {
            let mut v = 0x78u16.to_le_bytes().to_vec();
            v.extend(0xFFFF_FFFFu32.to_le_bytes());
            v.extend([1, 2, 3]);
            v
        };
        type Case<'a> = (&'a str, Vec<u8>, Option<u16>, usize, &'a str);
        let cases: Vec<Case> = vec![
            ("empty", vec![], None, 0, "no records"),
            ("garbage", vec![0xFF; 40], None, 0, "does not parse"),
            (
                "truncated top",
                top(rev.clone())[..5].to_vec(),
                None,
                0,
                "does not parse",
            ),
            ("empty top", top(vec![]), None, 0, "no model revision"),
            (
                "0x74 length 0",
                top(rec(0x74, &[])),
                None,
                0,
                "holds 0 bytes",
            ),
            (
                "0x74 length 1",
                top(rec(0x74, &[7])),
                None,
                0,
                "holds 1 bytes",
            ),
            (
                "0x74 length 65535",
                top(rec(0x74, &vec![0; 65535])),
                None,
                0,
                "holds 65535",
            ),
            (
                "duplicated 0x74 keeps the first",
                top(cat(&[rev.clone(), rec(0x74, &[99, 0])])),
                Some(18),
                0,
                "",
            ),
            (
                "0x78 overclaim",
                top(cat(&[rev.clone(), overclaim])),
                Some(18),
                0,
                "damaged",
            ),
            (
                "0x78 junk",
                top(cat(&[rev.clone(), rec(0x78, &[9; 7])])),
                Some(18),
                0,
                "save history",
            ),
            (
                "entry without time",
                top(cat(&[rev.clone(), history(entry(&[1], &[0; 8]))])),
                Some(18),
                1,
                "lacks",
            ),
            (
                "sound",
                top(cat(&[rev.clone(), history(entry(&[1, 0, 0, 0], &[0; 8]))])),
                Some(18),
                1,
                "",
            ),
        ];
        for (name, meta, revision, entries, problem) in cases {
            let (m, problems) = read_meta(&meta);
            assert_eq!(m.revision, revision, "{name}");
            assert_eq!(m.history.len(), entries, "{name}");
            if problem.is_empty() {
                assert!(problems.is_empty(), "{name}: {problems:?}");
            } else {
                assert!(
                    problems.iter().any(|p| p.contains(problem)),
                    "{name}: {problems:?}"
                );
            }
        }
    }

    #[test]
    fn the_header_byte_is_found_only_in_the_header_layout() {
        let mut h = b"xx".to_vec();
        h.extend([
            0x56, 0x46, 0x46, 0x08, 0x00, 0x01, 0x00, 0x12, 0x00, 1, 2, 3, 4,
        ]);
        assert_eq!(header_revision(&h), Some(0x12));
        assert_eq!(header_revision(&h[3..]), None);
        h[3] = 0;
        assert_eq!(header_revision(&h), None);
    }
}
