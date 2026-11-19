//! The stream container's archive codec (`docs/WRITING.md`, SKP_FORMAT §3):
//! one type that both reads and writes the MFC `CArchive` object stream, so
//! every record layout in [`crate::file17`] is written once and serves both
//! directions. Reading fills a value from the stream; writing emits the
//! value. A layout that reads a file and writes it back reproduces the
//! bytes, because the two directions cannot disagree about a field.
//!
//! The codec owns what the stream shares between records: the single
//! 1-based store map (one running slot counter for class declarations and
//! objects), lazy class declaration at first use, back-references by slot,
//! the big-tag escape at slot 0x7FFF, string length escalation and the
//! presence-masked persistent id, which the codec reads and writes only
//! in a release that stores it (§14, [`Release::has_pids`]).

use std::collections::HashMap;

use crate::file17::{Body, Obj, Ptr};
use crate::model::Error;
use crate::Release;

pub(crate) type R<T = ()> = Result<T, Error>;

const W_NEW_CLASS: u16 = 0xFFFF;
const W_BIG: u16 = 0x7FFF;
const DW_BIG_CLASS: u32 = 0x8000_0000;
/// The ids of objects read in place ([`Ar::inline`]) count from here,
/// past every store-map slot a file can hold.
pub(crate) const INLINE_IDS: u32 = 0x4000_0000;

/// Deepest object nesting followed: definitions inline inside instances
/// inside definitions. Far past any observed model; a stream that nests
/// deeper is refused rather than overflowing the stack.
const MAX_DEPTH: u32 = 256;

/// A UTF-16 string as stored: code units, so an unpaired surrogate
/// survives a round trip.
#[derive(Clone, PartialEq, Eq, Default, Hash)]
pub(crate) struct S16(pub Vec<u16>);

impl std::fmt::Debug for S16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.text())
    }
}

impl S16 {
    pub fn new(s: &str) -> S16 {
        S16(s.encode_utf16().collect())
    }
    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for S16 {
    fn from(s: &str) -> S16 {
        S16::new(s)
    }
}

/// Where an object lies in the file it is read from, kept while it is
/// being read.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Frame {
    pub id: u32,
    /// The offset of the object's tag.
    pub at: usize,
    /// The offset past its last byte.
    pub end: usize,
    /// The last slot assigned while it was read: its own, or that of the
    /// last object inside it.
    pub last: u32,
    /// Offsets the layouts mark ([`Ar::mark`]). The first: the
    /// coordinates of a vertex, the drawing base of an element, the
    /// length of an image. The second: the transform of an instance,
    /// the entity list of a definition.
    pub marks: [usize; 2],
    /// The last slot assigned before the entity list of a definition
    /// (or of the model), and the last one assigned inside the list.
    pub entities: (u32, u32),
}

/// What a reader that collects ([`Ar::collector`]) holds when the file
/// is read: every object as the entity the model is built from, by
/// slot, and the records of the classes the settings are read from.
pub(crate) struct Collected {
    /// The release the file was read as.
    pub release: Release,
    /// The store map: slot 0 a sentinel, a class or an object at every
    /// other.
    pub map: Vec<crate::carchive::Slot>,
    /// The definitions, as they ended.
    pub definitions: Vec<Frame>,
    /// Every attribute container an object names in its preamble, as
    /// (the object's slot, the container's slot); the model's own space
    /// names its container as slot 0.
    pub attributes: Vec<(u32, u32)>,
    /// The marks set outside any object: by the model's own space.
    pub file: Frame,
    pub records: HashMap<u32, Body>,
}

impl Collected {
    fn new(release: Release) -> Collected {
        Collected {
            release,
            map: vec![crate::carchive::Slot::Sentinel],
            definitions: Vec::new(),
            attributes: Vec::new(),
            file: Frame::default(),
            records: HashMap::new(),
        }
    }
}

/// The codec. `Ar::reader` reads, `Ar::writer` writes; every field method
/// takes `&mut` to the value and does the one or the other.
pub(crate) struct Ar<'a> {
    /// The release whose layouts the stream holds.
    release: Release,
    /// Reading: the objects are collected, and every pointer read is a
    /// reference to one.
    collected: Option<Collected>,
    /// Reading: the objects being read, outermost first.
    open: Vec<Frame>,
    rd: Option<&'a [u8]>,
    pos: usize,
    out: Vec<u8>,
    /// The last slot assigned (the map is 1-based).
    slots: u32,
    /// Reading: the class declared at a slot.
    class_at: HashMap<u32, &'static str>,
    /// Reading: the class of the object at a slot.
    object_at: HashMap<u32, &'static str>,
    /// Writing: the slot a class was declared at.
    slot_of_class: HashMap<&'static str, u32>,
    /// Writing: the slot an object (by id) was written at.
    slot_of_id: HashMap<u32, u32>,
    /// Class schemas: as declared in the stream when reading, as the
    /// version map gives them when writing.
    schemas: HashMap<String, u16>,
    depth: u32,
    /// Reading: the objects read in place so far ([`Ar::inline`]).
    inlined: u32,
    /// Writing: objects named by reference only, written where the
    /// first reference to them falls (a flattened file,
    /// [`crate::file17::write_flat`]).
    pub pending: HashMap<u32, Body>,
}

fn err<T>(msg: String) -> R<T> {
    Err(Error(msg))
}

impl<'a> Ar<'a> {
    /// A reader that keeps no tree of objects: each is handed over as
    /// it is read ([`Collected`]), and the pointer to it reads as a
    /// reference.
    pub fn collector(d: &'a [u8], pos: usize, release: Release) -> Ar<'a> {
        let mut ar = Ar::reader(d, pos, release);
        ar.collected = Some(Collected::new(release));
        ar
    }

    /// The release whose layouts the stream holds.
    pub fn release(&self) -> Release {
        self.release
    }

    /// Reading, with the objects collected.
    pub fn collecting(&self) -> bool {
        self.collected.is_some()
    }

    /// What a collector read.
    pub fn into_collected(self) -> Option<Collected> {
        self.collected
    }

    fn frame(&mut self) -> Option<&mut Frame> {
        let c = self.collected.as_mut()?;
        Some(match self.open.last_mut() {
            Some(f) => f,
            None => &mut c.file,
        })
    }

    /// Collecting: note the offset as mark `k` of the object being read
    /// (of the file, outside any object).
    pub fn mark(&mut self, k: usize) {
        let pos = self.pos;
        if let Some(f) = self.frame() {
            f.marks[k] = pos;
        }
    }

    /// Collecting: an entity list begins (`true`) or has ended.
    pub fn mark_entities(&mut self, begins: bool) {
        let slots = self.slots;
        if let Some(f) = self.frame() {
            if begins {
                f.entities = (slots, slots);
            } else {
                f.entities.1 = slots;
            }
        }
    }

    pub fn reader(d: &'a [u8], pos: usize, release: Release) -> Ar<'a> {
        Ar {
            release,
            collected: None,
            open: Vec::new(),
            rd: Some(d),
            pos,
            out: Vec::new(),
            slots: 0,
            class_at: HashMap::new(),
            object_at: HashMap::new(),
            slot_of_class: HashMap::new(),
            slot_of_id: HashMap::new(),
            schemas: HashMap::new(),
            depth: 0,
            inlined: 0,
            pending: HashMap::new(),
        }
    }

    pub fn writer(release: Release) -> Ar<'static> {
        Ar {
            release,
            collected: None,
            open: Vec::new(),
            rd: None,
            pos: 0,
            out: Vec::new(),
            slots: 0,
            class_at: HashMap::new(),
            object_at: HashMap::new(),
            slot_of_class: HashMap::new(),
            slot_of_id: HashMap::new(),
            schemas: HashMap::new(),
            depth: 0,
            inlined: 0,
            pending: HashMap::new(),
        }
    }

    pub fn reading(&self) -> bool {
        self.rd.is_some()
    }

    /// The stream offset: bytes read, or bytes written.
    pub fn pos(&self) -> usize {
        if self.reading() {
            self.pos
        } else {
            self.out.len()
        }
    }

    /// Bytes left to read (0 when writing).
    pub fn remaining(&self) -> usize {
        self.rd
            .map(|d| d.len().saturating_sub(self.pos))
            .unwrap_or(0)
    }

    /// The written stream.
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }

    /// Set a class schema for writing (from the version map).
    pub fn set_schema(&mut self, class: &str, schema: u16) {
        self.schemas.insert(class.to_string(), schema);
    }

    /// Reading: the schema every class was declared with, by name.
    pub fn declared(&self) -> Vec<(String, u16)> {
        let mut v: Vec<(String, u16)> = self.schemas.iter().map(|(k, v)| (k.clone(), *v)).collect();
        v.sort();
        v
    }

    /// Reading: the class of the object a reference names.
    pub fn class_of(&self, slot: u32) -> Option<&'static str> {
        self.object_at.get(&slot).copied()
    }

    // ---- primitives

    /// A fixed run of bytes.
    pub fn bytes(&mut self, v: &mut [u8]) -> R {
        match self.rd {
            Some(d) => {
                let end = self
                    .pos
                    .checked_add(v.len())
                    .filter(|&e| e <= d.len())
                    .ok_or_else(|| Error(format!("stream ends at {:#x}", self.pos)))?;
                v.copy_from_slice(&d[self.pos..end]);
                self.pos = end;
            }
            None => self.out.extend_from_slice(v),
        }
        Ok(())
    }

    /// `n` bytes held in a vector.
    pub fn vec(&mut self, v: &mut Vec<u8>, n: usize) -> R {
        match self.rd {
            Some(d) => {
                let end = self
                    .pos
                    .checked_add(n)
                    .filter(|&e| e <= d.len())
                    .ok_or_else(|| Error(format!("stream ends at {:#x}", self.pos)))?;
                v.clear();
                v.extend_from_slice(&d[self.pos..end]);
                self.pos = end;
            }
            None => {
                if v.len() != n {
                    return err(format!("a {n}-byte block holds {} bytes", v.len()));
                }
                self.out.extend_from_slice(v);
            }
        }
        Ok(())
    }

    /// A `u32` length and that many bytes.
    pub fn blob(&mut self, v: &mut Vec<u8>) -> R {
        let n = self.count(v.len())?;
        self.vec(v, n)
    }

    pub fn u8(&mut self, v: &mut u8) -> R {
        let mut b = [*v];
        self.bytes(&mut b)?;
        *v = b[0];
        Ok(())
    }

    pub fn u16(&mut self, v: &mut u16) -> R {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = u16::from_le_bytes(b);
        Ok(())
    }

    pub fn u32(&mut self, v: &mut u32) -> R {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = u32::from_le_bytes(b);
        Ok(())
    }

    pub fn f64(&mut self, v: &mut f64) -> R {
        let mut b = v.to_le_bytes();
        self.bytes(&mut b)?;
        *v = f64::from_le_bytes(b);
        Ok(())
    }

    pub fn f64s(&mut self, v: &mut [f64]) -> R {
        for x in v {
            self.f64(x)?;
        }
        Ok(())
    }

    /// A `u32` element count: written from `len`, or read and checked
    /// against the bytes that remain (every element takes at least one).
    pub fn count(&mut self, len: usize) -> R<usize> {
        let mut n = u32::try_from(len)
            .map_err(|_| Error(format!("a list of {len} elements exceeds the u32 count")))?;
        self.u32(&mut n)?;
        let n = n as usize;
        if self.reading() && n > self.remaining() {
            return err(format!(
                "count {n} at {:#x} exceeds the {} bytes that remain",
                self.pos - 4,
                self.remaining()
            ));
        }
        Ok(n)
    }

    /// A UTF-16 string record (§2.1): `FF FE FF`, the escalating length,
    /// the code units.
    pub fn str(&mut self, v: &mut S16) -> R {
        if self.reading() {
            let at = self.pos;
            let mut m = [0u8; 3];
            self.bytes(&mut m)?;
            if m != [0xFF, 0xFE, 0xFF] {
                return err(format!("no string record at {at:#x}: {m:02x?}"));
            }
            let mut b = 0u8;
            self.u8(&mut b)?;
            let mut n = b as usize;
            if b == 0xFF {
                let mut w = 0u16;
                self.u16(&mut w)?;
                n = w as usize;
                if w == 0xFFFF {
                    let mut dw = 0u32;
                    self.u32(&mut dw)?;
                    n = dw as usize;
                }
            }
            if n.checked_mul(2)
                .map(|x| x > self.remaining())
                .unwrap_or(true)
            {
                return err(format!(
                    "string of {n} units at {at:#x} runs past the stream"
                ));
            }
            let mut raw = vec![0u8; n * 2];
            self.bytes(&mut raw)?;
            v.0 = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
        } else {
            let n = v.0.len();
            self.out.extend_from_slice(&[0xFF, 0xFE, 0xFF]);
            if n < 0xFF {
                self.out.push(n as u8);
            } else if n < 0xFFFF {
                self.out.push(0xFF);
                self.out.extend_from_slice(&(n as u16).to_le_bytes());
            } else {
                let n32 =
                    u32::try_from(n).map_err(|_| Error("string exceeds the u32 length".into()))?;
                self.out.extend_from_slice(&[0xFF, 0xFF, 0xFF]);
                self.out.extend_from_slice(&n32.to_le_bytes());
            }
            for u in &v.0 {
                self.out.extend_from_slice(&u.to_le_bytes());
            }
        }
        Ok(())
    }

    /// A presence-masked persistent id (§5.1): a mask byte, then the
    /// nonzero bytes of the id, low to high. A release without the field
    /// (§14) holds nothing here: nothing is read or written, and the
    /// value is left as it is.
    pub fn pid(&mut self, v: &mut u32) -> R {
        if !self.release.has_pids() {
            return Ok(());
        }
        if self.reading() {
            let at = self.pos;
            let mut mask = 0u8;
            self.u8(&mut mask)?;
            if mask > 0x0F {
                return err(format!("persistent id mask {mask:#x} at {at:#x}"));
            }
            let mut pid = 0u32;
            for k in 0..4 {
                if mask & (1 << k) != 0 {
                    let mut b = 0u8;
                    self.u8(&mut b)?;
                    if b == 0 {
                        return err(format!("persistent id at {at:#x} stores a zero byte"));
                    }
                    pid |= u32::from(b) << (8 * k);
                }
            }
            *v = pid;
        } else {
            let b = v.to_le_bytes();
            let mut mask = 0u8;
            for (k, x) in b.iter().enumerate() {
                if *x != 0 {
                    mask |= 1 << k;
                }
            }
            self.out.push(mask);
            self.out.extend(b.iter().filter(|x| **x != 0));
        }
        Ok(())
    }

    // ---- objects

    /// An object pointer (§3.1): null, a back-reference, or a new object
    /// with its body.
    pub fn ptr(&mut self, p: &mut Ptr) -> R {
        self.depth += 1;
        let r = if self.depth > MAX_DEPTH {
            err(format!(
                "objects nest deeper than {MAX_DEPTH} at {:#x}",
                self.pos()
            ))
        } else if self.reading() {
            self.read_ptr().map(|v| *p = v)
        } else {
            self.write_ptr(p)
        };
        self.depth -= 1;
        r
    }

    /// A pointer that must be null or name an object of one of `classes`.
    pub fn ptr_to(&mut self, p: &mut Ptr, classes: &[&str]) -> R {
        let at = self.pos();
        self.ptr(p)?;
        let class = match p {
            Ptr::Null => return Ok(()),
            Ptr::New(o) => Some(o.body.class_name()),
            Ptr::Ref(slot) if self.reading() => self.class_of(*slot),
            Ptr::Ref(_) => return Ok(()),
        };
        match class {
            Some(c) if classes.contains(&c) => Ok(()),
            other => err(format!(
                "pointer at {at:#x} names {other:?}, expected one of {classes:?}"
            )),
        }
    }

    fn read_ptr(&mut self) -> R<Ptr> {
        let at = self.pos;
        let mut tag = 0u16;
        self.u16(&mut tag)?;
        let class: &'static str = match tag {
            0 => return Ok(Ptr::Null),
            W_NEW_CLASS => {
                let (mut schema, mut len) = (0u16, 0u16);
                self.u16(&mut schema)?;
                self.u16(&mut len)?;
                if len > 64 {
                    return err(format!("class name of {len} bytes at {at:#x}"));
                }
                let mut name = vec![0u8; len as usize];
                self.bytes(&mut name)?;
                let name = String::from_utf8(name)
                    .map_err(|_| Error(format!("class name at {at:#x} is not ASCII")))?;
                let class = Body::static_name(&name).ok_or_else(|| {
                    Error(format!(
                        "{}: class {name} at {at:#x} has no layout",
                        crate::file17::UNSUPPORTED
                    ))
                })?;
                if self.schemas.insert(name.clone(), schema).is_some() {
                    return err(format!("class {name} declared twice, at {at:#x}"));
                }
                self.slots += 1;
                self.class_at.insert(self.slots, class);
                if let Some(c) = self.collected.as_mut() {
                    c.map.push(crate::carchive::Slot::Class(class.to_string()));
                }
                class
            }
            W_BIG => {
                let mut v = 0u32;
                self.u32(&mut v)?;
                if v & DW_BIG_CLASS == 0 {
                    return self.back_ref(v, at);
                }
                self.class_ref(v & !DW_BIG_CLASS, at)?
            }
            t if t & 0x8000 != 0 => self.class_ref(u32::from(t & 0x7FFF), at)?,
            t => return self.back_ref(u32::from(t), at),
        };
        self.slots += 1;
        let id = self.slots;
        self.object_at.insert(id, class);
        let mut body = Body::new(class)
            .ok_or_else(|| Error(format!("no layout for class {class} at {at:#x}")))?;
        if let Some(c) = self.collected.as_mut() {
            // The slot is taken before the body is read: the objects
            // inside it take the slots that follow.
            c.map
                .try_reserve(1)
                .map_err(|_| Error(crate::carchive::OUT_OF_MEMORY.into()))?;
            c.map
                .push(crate::carchive::Slot::Object(crate::entity::Entity::Other(
                    class.to_string(),
                )));
            self.open.push(Frame {
                id,
                at,
                ..Default::default()
            });
        }
        let read = crate::file17::layout(self, &mut body)
            .map_err(|e| Error(format!("{class} at {at:#x}: {}", e.0)));
        let frame = self.open.pop();
        read?;
        let (Some(c), Some(mut frame)) = (self.collected.as_mut(), frame) else {
            return Ok(Ptr::New(Box::new(Obj { id, body })));
        };
        (frame.end, frame.last) = (self.pos, self.slots);
        c.map[id as usize] =
            crate::carchive::Slot::Object(crate::read17::entity(&body, &frame, c.release));
        if let Some(container) = crate::read17::attributes_of(&body) {
            c.attributes.push((id, container));
        }
        if matches!(body, Body::Def(_)) {
            c.definitions.push(frame);
        }
        if crate::read17::kept(&body) {
            c.records.insert(id, body);
        }
        Ok(Ptr::Ref(id))
    }

    fn class_ref(&self, slot: u32, at: usize) -> R<&'static str> {
        self.class_at.get(&slot).copied().ok_or_else(|| {
            Error(format!(
                "class reference to slot {slot} at {at:#x} names no class"
            ))
        })
    }

    fn back_ref(&self, slot: u32, at: usize) -> R<Ptr> {
        if !self.object_at.contains_key(&slot) {
            return err(format!(
                "back-reference to slot {slot} at {at:#x} names no object"
            ));
        }
        Ok(Ptr::Ref(slot))
    }

    fn write_ptr(&mut self, p: &mut Ptr) -> R {
        match p {
            Ptr::Null => self.out.extend_from_slice(&[0, 0]),
            Ptr::Ref(id) if !self.slot_of_id.contains_key(id) && self.pending.contains_key(id) => {
                let body = self.pending.remove(id).expect("pending");
                let mut p = Ptr::New(Box::new(Obj { id: *id, body }));
                return self.write_ptr(&mut p);
            }
            Ptr::Ref(id) => {
                let slot = *self.slot_of_id.get(id).ok_or_else(|| {
                    Error(format!(
                        "reference to object {id}, which is not written yet"
                    ))
                })?;
                if slot < u32::from(W_BIG) {
                    self.out.extend_from_slice(&(slot as u16).to_le_bytes());
                } else {
                    self.out.extend_from_slice(&W_BIG.to_le_bytes());
                    self.out.extend_from_slice(&slot.to_le_bytes());
                }
            }
            Ptr::New(o) => {
                let class = o.body.class_name();
                match self.slot_of_class.get(class).copied() {
                    None => {
                        let schema = *self
                            .schemas
                            .get(class)
                            .ok_or_else(|| Error(format!("no schema for class {class}")))?;
                        self.out.extend_from_slice(&W_NEW_CLASS.to_le_bytes());
                        self.out.extend_from_slice(&schema.to_le_bytes());
                        self.out
                            .extend_from_slice(&(class.len() as u16).to_le_bytes());
                        self.out.extend_from_slice(class.as_bytes());
                        self.slots += 1;
                        self.slot_of_class.insert(class, self.slots);
                    }
                    Some(c) if c < u32::from(W_BIG) => {
                        self.out
                            .extend_from_slice(&(0x8000 | c as u16).to_le_bytes());
                    }
                    Some(c) => {
                        self.out.extend_from_slice(&W_BIG.to_le_bytes());
                        self.out
                            .extend_from_slice(&(DW_BIG_CLASS | c).to_le_bytes());
                    }
                }
                self.slots = self
                    .slots
                    .checked_add(1)
                    .filter(|s| *s < DW_BIG_CLASS)
                    .ok_or_else(|| Error("the store map is full".into()))?;
                if self.slot_of_id.insert(o.id, self.slots).is_some() {
                    return err(format!("object id {} is written twice", o.id));
                }
                crate::file17::layout(self, &mut o.body)
                    .map_err(|e| Error(format!("{class}: {}", e.0)))?;
            }
        }
        Ok(())
    }

    /// An object held in place of a pointer, by a release whose stream
    /// writes its fields where a later release writes a pointer to it
    /// (§14, [`Release::camera_is_object`]): no tag, no store-map slot.
    /// Read, it gets an id of its own past every slot, so the model
    /// finds it as it finds an object; written, its body is written.
    pub fn inline(&mut self, p: &mut Ptr, class: &'static str) -> R {
        let at = self.pos();
        if self.reading() {
            self.inlined += 1;
            let id = INLINE_IDS + self.inlined;
            let mut body = Body::new(class)
                .ok_or_else(|| Error(format!("no layout for class {class} at {at:#x}")))?;
            crate::file17::layout(self, &mut body)
                .map_err(|e| Error(format!("{class} in place at {at:#x}: {}", e.0)))?;
            if let Some(c) = self.collected.as_mut() {
                c.records.insert(id, body);
                *p = Ptr::Ref(id);
            } else {
                *p = Ptr::New(Box::new(Obj { id, body }));
            }
            return Ok(());
        }
        if let Ptr::Ref(id) = p {
            if let Some(body) = self.pending.remove(id) {
                *p = Ptr::New(Box::new(Obj { id: *id, body }));
            }
        }
        match p {
            Ptr::New(o) if o.body.class_name() == class => crate::file17::layout(self, &mut o.body)
                .map_err(|e| Error(format!("{class} in place: {}", e.0))),
            _ => err(format!("{class} in place at {at:#x} is not one")),
        }
    }

    /// A counted list of pointers.
    pub fn ptrs(&mut self, v: &mut Vec<Ptr>) -> R {
        let n = self.count(v.len())?;
        if self.reading() {
            v.clear();
            v.reserve(n.min(1 << 16));
            for _ in 0..n {
                let mut p = Ptr::Null;
                self.ptr(&mut p)?;
                v.push(p);
            }
        } else {
            for p in v {
                self.ptr(p)?;
            }
        }
        Ok(())
    }

    /// A list of objects ended by a null pointer.
    pub fn ptrs_until_null(&mut self, v: &mut Vec<Ptr>) -> R {
        if self.reading() {
            v.clear();
            loop {
                let mut p = Ptr::Null;
                self.ptr(&mut p)?;
                if p == Ptr::Null {
                    return Ok(());
                }
                v.push(p);
            }
        }
        for p in v.iter_mut() {
            if *p == Ptr::Null {
                return err("a null pointer inside a null-terminated list".into());
            }
            self.ptr(p)?;
        }
        self.out.extend_from_slice(&[0, 0]);
        Ok(())
    }
}
