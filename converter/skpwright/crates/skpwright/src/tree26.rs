//! The 2026 record tree (`docs/SKP_FORMAT.md` §16.2) as an owned,
//! editable structure: every record is `u16 tag | u32 length | payload`,
//! and a payload is either a leaf value or a sequence of child records.
//! Tags are type-scoped, so which tags are containers is a fixed table
//! ([`CONTAINERS`]); a record outside it stays a leaf and serializes back
//! verbatim, which keeps a re-serialized tree byte-identical to the
//! `model.dat` it was parsed from whatever the reader knows.

use crate::model::Error;

/// Every tag whose payload is a sequence of records, established over the
/// corpus: each of these tiles as records in every non-empty occurrence
/// (73 files, 264,747 entity bases), and no other tag does in all of its
/// occurrences (a zero f64 triple tiles as four empty tag-0 records, which
/// is why the parser never guesses). Kept sorted for binary search.
pub(crate) const CONTAINERS: &[u16] = &[
    0x01F4, 0x01F5, 0x01F6, 0x01F7, 0x01F8, 0x01F9, 0x01FA, 0x01FB, 0x01FC, 0x01FD, 0x01FE, 0x01FF,
    0x0200, 0x0203, 0x0204, 0x0205, 0x0206, 0x0207, 0x0208, 0x0209, 0x020E, 0x0210, 0x0213, 0x0215,
    0x03E8, 0x05DC, 0x05DD, 0x07D0, 0x09C4, 0x0BB8, 0x0DAC, 0x0DAE, 0x0FA0, 0x1194, 0x1195, 0x1388,
    0x1389, 0x138A, 0x138B, 0x138C, 0x138D, 0x1390, 0x1391, 0x1392, 0x1393, 0x1396, 0x1397, 0x1398,
    0x1399, 0x139B, 0x139D, 0x13A4, 0x13EC, 0x157C, 0x1585, 0x1588, 0x1770, 0x1771, 0x1964, 0x1B58,
    0x1D4C, 0x1F40, 0x2328, 0x251C, 0x251D, 0x251E, 0x2710, 0x2711, 0x2712, 0x2713, 0x2717, 0x2718,
    0x2CEC, 0x2CED, 0x2EE0, 0x2EE6, 0x30D4, 0x30D5, 0x32C8, 0x34BC, 0x36B1, 0x36B2, 0x36B3, 0x36B5,
    0x38A4, 0x38AE, 0x3A98, 0x3A99, 0x3A9B, 0x3C8C, 0x3C8F, 0x3E80, 0x4074, 0x4075, 0x4076, 0x4268,
    0x4269, 0x426C, 0x445C, 0x4650, 0x4A38, 0x4C2C, 0x4E20, 0x4E21, 0x5014, 0x5208, 0x520B, 0x520C,
    0x53FC, 0x55F0, 0x55F4, 0x57E4, 0x59D8, 0x5BCC, 0x5BCD, 0x5BCE, 0x5FB4, 0x61A8, 0x61A9, 0x61AA,
    0x61AC, 0x639C, 0x639F, 0x63A0, 0x6590, 0x6784, 0x6978, 0x6979, 0x697B, 0x6B6C, 0x6D60, 0x6D61,
    0x6F54, 0x7148, 0x714A, 0x714D, 0x714E, 0x714F, 0x7158, 0x715B, 0x733C, 0x7530, 0x7918, 0x7919,
    0x7B0C, 0x7B0E, 0x7D64, 0x7D82, 0x7EC2,
];

pub(crate) fn is_container(tag: u16) -> bool {
    CONTAINERS.binary_search(&tag).is_ok()
}

/// A record's payload.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// Raw bytes: a leaf value, or an unknown record kept verbatim.
    Leaf(Vec<u8>),
    /// Child records.
    Kids(Vec<Record>),
}

/// One record of `model.dat`.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub tag: u16,
    pub body: Body,
}

impl Record {
    pub fn leaf(tag: u16, bytes: impl Into<Vec<u8>>) -> Record {
        Record {
            tag,
            body: Body::Leaf(bytes.into()),
        }
    }

    pub fn kids(tag: u16, kids: Vec<Record>) -> Record {
        Record {
            tag,
            body: Body::Kids(kids),
        }
    }

    pub fn empty(tag: u16) -> Record {
        Record::leaf(tag, Vec::new())
    }

    // ------------------------------------------------------------ parsing

    /// Split `data` into the records that tile it exactly; `None` when it
    /// does not tile.
    fn split(data: &[u8]) -> Option<Vec<(u16, &[u8])>> {
        let mut out = Vec::new();
        let mut o = 0usize;
        while o < data.len() {
            let head = data.get(o..o + 6)?;
            let tag = u16::from_le_bytes([head[0], head[1]]);
            let len = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
            let body = (o + 6).checked_add(len).and_then(|e| data.get(o + 6..e))?;
            out.push((tag, body));
            o += 6 + len;
        }
        Some(out)
    }

    /// Build the record `tag` from `payload`, descending into container
    /// tags. A container whose payload does not tile is kept as a leaf and
    /// noted in `notes` (never silent, never fatal: the bytes still round
    /// trip). `depth` bounds recursion on hostile input.
    pub(crate) fn parse(tag: u16, payload: &[u8], depth: u32, notes: &mut Vec<String>) -> Record {
        if !is_container(tag) || payload.is_empty() {
            return Record::leaf(tag, payload);
        }
        if depth > 64 {
            notes.push(format!(
                "record {tag:#06x} nested deeper than 64: kept as bytes"
            ));
            return Record::leaf(tag, payload);
        }
        match Self::split(payload) {
            Some(parts) => Record::kids(
                tag,
                parts
                    .into_iter()
                    .map(|(t, p)| Record::parse(t, p, depth + 1, notes))
                    .collect(),
            ),
            None => {
                notes.push(format!(
                    "container record {tag:#06x} does not tile as records: kept as bytes"
                ));
                Record::leaf(tag, payload)
            }
        }
    }

    /// This record and those under it as text, one a line, indented by
    /// depth: the tag, then the children's count or the value. A value
    /// of at most 32 bytes is shown in hexadecimal, a longer one by its
    /// length.
    pub(crate) fn describe_into(&self, out: &mut String, depth: usize) {
        use std::fmt::Write;
        // Iterative, so that the depth of a tree costs no stack.
        let mut pending: Vec<(&Record, usize)> = vec![(self, depth)];
        while let Some((rec, depth)) = pending.pop() {
            let _ = write!(out, "{:width$}{:#06x}", "", rec.tag, width = depth * 2);
            match &rec.body {
                Body::Kids(kids) => {
                    let _ = writeln!(out, " {{{} records}}", kids.len());
                    pending.extend(kids.iter().rev().map(|k| (k, depth + 1)));
                }
                Body::Leaf(bytes) if bytes.is_empty() => {
                    let _ = writeln!(out, " empty");
                }
                Body::Leaf(bytes) if bytes.len() <= 32 => {
                    out.push(' ');
                    for b in bytes {
                        let _ = write!(out, "{b:02x}");
                    }
                    out.push('\n');
                }
                Body::Leaf(bytes) => {
                    let _ = writeln!(out, " [{} bytes]", bytes.len());
                }
            }
        }
    }

    /// Parse a whole `model.dat`: exactly one top-level record.
    pub fn parse_model_dat(dat: &[u8], notes: &mut Vec<String>) -> Result<Record, Error> {
        let parts =
            Self::split(dat).ok_or_else(|| Error("model.dat does not frame as a record".into()))?;
        match parts.as_slice() {
            [(tag, payload)] => Ok(Record::parse(*tag, payload, 0, notes)),
            [] => Err(Error("empty model.dat".into())),
            _ => Err(Error(format!(
                "model.dat holds {} top-level records, expected one",
                parts.len()
            ))),
        }
    }

    // ------------------------------------------------------------ writing

    pub fn payload_len(&self) -> usize {
        match &self.body {
            Body::Leaf(b) => b.len(),
            Body::Kids(k) => k.iter().map(|r| 6 + r.payload_len()).sum(),
        }
    }

    /// Append `tag | length | payload` to `out`.
    pub fn write_into(&self, out: &mut Vec<u8>) {
        out.extend(self.tag.to_le_bytes());
        out.extend((self.payload_len() as u32).to_le_bytes());
        self.write_payload(out);
    }

    fn write_payload(&self, out: &mut Vec<u8>) {
        match &self.body {
            Body::Leaf(b) => out.extend_from_slice(b),
            Body::Kids(k) => {
                for r in k {
                    r.write_into(out);
                }
            }
        }
    }

    /// The record with its header, as bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.payload_len());
        self.write_into(&mut out);
        out
    }

    // ------------------------------------------------------------ access

    /// Child records (empty for a leaf).
    pub fn children(&self) -> &[Record] {
        match &self.body {
            Body::Kids(k) => k,
            Body::Leaf(_) => &[],
        }
    }

    /// Mutable children. An empty leaf of a container tag becomes a
    /// container; a non-empty leaf is not a container and yields `None`.
    pub fn children_mut(&mut self) -> Option<&mut Vec<Record>> {
        if let Body::Leaf(b) = &self.body {
            if b.is_empty() && is_container(self.tag) {
                self.body = Body::Kids(Vec::new());
            }
        }
        match &mut self.body {
            Body::Kids(k) => Some(k),
            Body::Leaf(_) => None,
        }
    }

    /// The first child with `tag`.
    pub fn get(&self, tag: u16) -> Option<&Record> {
        self.children().iter().find(|r| r.tag == tag)
    }

    pub fn get_mut(&mut self, tag: u16) -> Option<&mut Record> {
        self.children_mut()?.iter_mut().find(|r| r.tag == tag)
    }

    /// Every child with `tag`, in order.
    pub fn all(&self, tag: u16) -> impl Iterator<Item = &Record> {
        self.children().iter().filter(move |r| r.tag == tag)
    }

    /// Follow a path of tags, first match at each level.
    pub fn path(&self, tags: &[u16]) -> Option<&Record> {
        let mut r = self;
        for &t in tags {
            r = r.get(t)?;
        }
        Some(r)
    }

    pub fn path_mut(&mut self, tags: &[u16]) -> Option<&mut Record> {
        let mut r = self;
        for &t in tags {
            r = r.get_mut(t)?;
        }
        Some(r)
    }

    /// Replace the first child with `rec.tag`, or append `rec`. Returns
    /// `false`, changing nothing, when this record is a leaf.
    pub fn set(&mut self, rec: Record) -> bool {
        let Some(kids) = self.children_mut() else {
            return false;
        };
        match kids.iter_mut().find(|r| r.tag == rec.tag) {
            Some(slot) => *slot = rec,
            None => kids.push(rec),
        }
        true
    }

    /// Remove every child with `tag`; returns how many were removed.
    pub fn remove(&mut self, tag: u16) -> usize {
        let Some(kids) = self.children_mut() else {
            return 0;
        };
        let before = kids.len();
        kids.retain(|r| r.tag != tag);
        before - kids.len()
    }

    /// Leaf bytes (`None` for a container).
    pub fn bytes(&self) -> Option<&[u8]> {
        match &self.body {
            Body::Leaf(b) => Some(b),
            Body::Kids(_) => None,
        }
    }

    // ------------------------------------------------------------ values

    /// Little-endian unsigned integer of the payload's width (ids are
    /// written 1–3 bytes wide, §16.2; u32 fields 4).
    pub fn uint(&self) -> Option<u64> {
        let b = self.bytes()?;
        (b.len() <= 8).then(|| b.iter().rev().fold(0u64, |v, &x| (v << 8) | x as u64))
    }

    pub fn f64s(&self) -> Vec<f64> {
        self.bytes()
            .map(|b| {
                b.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|c| f64::from_le_bytes(*c))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn text(&self) -> String {
        self.bytes()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default()
    }

    /// An unsigned integer as narrow as its value needs (at least 1 byte),
    /// the way ids are stored.
    pub fn narrow(tag: u16, v: u64) -> Record {
        let mut bytes = v.to_le_bytes().to_vec();
        while bytes.len() > 1 && bytes.last() == Some(&0) {
            bytes.pop();
        }
        Record::leaf(tag, bytes)
    }

    pub fn u8(tag: u16, v: u8) -> Record {
        Record::leaf(tag, vec![v])
    }

    pub fn u16(tag: u16, v: u16) -> Record {
        Record::leaf(tag, v.to_le_bytes().to_vec())
    }

    pub fn u32(tag: u16, v: u32) -> Record {
        Record::leaf(tag, v.to_le_bytes().to_vec())
    }

    pub fn f32(tag: u16, v: f32) -> Record {
        Record::leaf(tag, v.to_le_bytes().to_vec())
    }

    pub fn f64(tag: u16, v: f64) -> Record {
        Record::leaf(tag, v.to_le_bytes().to_vec())
    }

    pub fn f64s_of(tag: u16, v: &[f64]) -> Record {
        Record::leaf(
            tag,
            v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>(),
        )
    }

    pub fn string(tag: u16, s: &str) -> Record {
        Record::leaf(tag, s.as_bytes().to_vec())
    }

    /// A packed id list (§16.2): repeated `<width:u8> <id>`.
    pub fn id_list(tag: u16, ids: &[u64]) -> Record {
        let mut b = Vec::new();
        for &id in ids {
            let mut v = id.to_le_bytes().to_vec();
            while v.len() > 1 && v.last() == Some(&0) {
                v.pop();
            }
            b.push(v.len() as u8);
            b.extend(v);
        }
        Record::leaf(tag, b)
    }

    /// Decode a packed id list.
    pub fn ids(&self) -> Vec<u64> {
        let mut out = Vec::new();
        let Some(b) = self.bytes() else { return out };
        let mut o = 0;
        while o < b.len() {
            let w = b[o] as usize;
            let Some(v) = b.get(o + 1..o + 1 + w) else {
                break;
            };
            out.push(v.iter().rev().fold(0u64, |a, &x| (a << 8) | x as u64));
            o += 1 + w;
        }
        out
    }

    /// Replace a leaf's bytes.
    pub fn set_bytes(&mut self, bytes: Vec<u8>) {
        self.body = Body::Leaf(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_are_sorted() {
        assert!(CONTAINERS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn narrow_ints_and_id_lists() {
        assert_eq!(Record::narrow(1, 0).bytes().unwrap(), &[0]);
        assert_eq!(Record::narrow(1, 255).bytes().unwrap(), &[255]);
        assert_eq!(Record::narrow(1, 256).bytes().unwrap(), &[0, 1]);
        assert_eq!(Record::narrow(1, 0x1_0000).bytes().unwrap(), &[0, 0, 1]);
        let l = Record::id_list(2, &[1, 300, 70000]);
        assert_eq!(l.ids(), vec![1, 300, 70000]);
        assert_eq!(l.bytes().unwrap().len(), 2 + 3 + 4);
    }

    /// Every `model.dat` of the corpus parses with no notes and writes back
    /// byte for byte.
    #[test]
    fn corpus_trees_round_trip() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let mut files: Vec<_> = std::fs::read_dir(root.join("2026"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("skp"))
            .collect();
        files.push(root.join("third-party/theater-2026.skp"));
        for p in files {
            let d = std::fs::read(&p).unwrap();
            let start = d.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
            let a = crate::zip::Archive::open(&d, start).unwrap();
            let dat = a.read("model.dat").unwrap().unwrap();
            let mut notes = Vec::new();
            let tree = Record::parse_model_dat(&dat, &mut notes).unwrap();
            assert!(notes.is_empty(), "{}: {notes:?}", p.display());
            assert!(
                tree.to_bytes() == dat,
                "{}: tree does not round trip",
                p.display()
            );
            // Every listed container really is one here: no leaf carries
            // a container tag.
            fn check(r: &Record, p: &std::path::Path) {
                if let Body::Leaf(b) = &r.body {
                    assert!(
                        b.is_empty() || !is_container(r.tag),
                        "{}: container {:#06x} parsed as leaf",
                        p.display(),
                        r.tag
                    );
                }
                for k in r.children() {
                    check(k, p);
                }
            }
            check(&tree, &p);
        }
    }

    #[test]
    fn set_on_a_leaf_is_refused() {
        let mut leaf = Record::u8(1, 0);
        assert!(!leaf.set(Record::u8(2, 0)));
        assert_eq!(leaf.bytes(), Some(&[0u8][..]));
        let mut kids = Record::kids(1, vec![]);
        assert!(kids.set(Record::u8(2, 0)));
        assert_eq!(kids.remove(2), 1);
    }
}
