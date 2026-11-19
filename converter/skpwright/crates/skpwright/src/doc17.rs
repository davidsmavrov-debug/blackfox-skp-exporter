//! The stream-container document, tier 1 (`docs/WRITING.md`): a file of
//! any release from 3 to 2020 kept byte for byte, with same-length
//! in-place edits on the fields whose layout the grammar or the
//! continuous walk pins exactly — vertex coordinates, instance
//! transforms, the hidden flag of a drawing element, and a solid
//! material's colour and opacity. Every other byte of the file stays;
//! the store map, class tags and back-references are untouched because
//! no record changes size.
//!
//! [`StreamDocument::from_model`] authors a file of any release the
//! grammar holds from a [`Model`]: the whole file is written object by
//! object through the archive writer (`docs/WRITING.md`), and the result
//! takes the same in-place edits.

use std::collections::{HashMap, HashSet};

use crate::carchive::Slot;
use crate::entity::Entity;
use crate::model::Error;
use crate::Model;

const M_PER_INCH: f64 = 0.0254;

/// Where an entity's editable bytes sit.
#[derive(Debug, Clone, Copy)]
enum Site {
    /// The three coordinates.
    Vertex(usize),
    /// The drawbase (§4q): hidden flag at +2.
    Drawing(usize),
    /// The drawbase and the 13-f64 transform.
    Instance { drawbase: usize, transform: usize },
}

/// The file written anew from its records, by the whole grammar alone:
/// the in-place setters' walk of the file has no part in it. A file of a
/// release the grammar does not hold is refused by its release
/// ([`crate::Release::grammar`]).
pub(crate) fn repacked(d: &[u8]) -> Result<Vec<u8>, Error> {
    crate::file17::File17::read(d)?.write()
}

/// [`StreamDocument::verify`] of a file's bytes.
pub(crate) fn verify(d: &[u8]) -> Result<(), Error> {
    let written = repacked(d)?;
    if written == d {
        return Ok(());
    }
    let at = written
        .iter()
        .zip(d)
        .position(|(a, b)| a != b)
        .unwrap_or(written.len().min(d.len()));
    Err(Error(format!(
        "the file does not reproduce from its records: first difference at {at:#x}"
    )))
}

/// [`StreamDocument::records`] of a file's bytes.
pub(crate) fn records(d: &[u8]) -> Result<String, Error> {
    crate::file17::File17::read(d).map(|f| crate::file17::describe(&f))
}

/// A file of the stream container — any SketchUp release from 3 to 8
/// and 2013 to 2020 (`SKP_FORMAT.md` §2–§14) — byte for byte, with
/// in-place setters. The 2026 container's counterpart is
/// [`Document`](crate::Document).
#[derive(Debug, Clone)]
pub struct StreamDocument {
    bytes: Vec<u8>,
    /// Entity sites by persistent id.
    sites: HashMap<u32, Site>,
    /// Ids that name more than one entity: a file can reuse an id inside
    /// definitions (§10.10). Such an id cannot be edited, since the
    /// setters could not tell which entity is meant.
    ambiguous: HashSet<u32>,
    /// Solid materials: name → file offset of the RGBA bytes.
    materials: HashMap<String, usize>,
    /// What [`from_model`](Self::from_model) could not write.
    notes: Vec<String>,
}

impl StreamDocument {
    /// Open a file of the stream container. It must read exactly, by the
    /// grammar or by the continuous walk (§4s); a file that does not is
    /// refused, since no field offset would be trustworthy.
    pub fn open(d: &[u8]) -> Result<StreamDocument, Error> {
        if crate::detect_container(d) != crate::Container::Stream {
            return Err(Error("not a .skp file of the stream container".into()));
        }
        // The entities and where each lies: by the grammar, which reads
        // a file of any release it holds whole, else by the walk.
        let map = match crate::read17::read(d) {
            Ok(r) => r.walk.map,
            Err(refused) => crate::walk2::walk(d)
                .map_err(|f| {
                    Error(format!(
                        "the file cannot be read for editing: {}; the walk stops in {} at {:#x} ({})",
                        refused.0, f.stage, f.at, f.detail
                    ))
                })?
                .map,
        };
        let mut sites: HashMap<u32, Site> = HashMap::new();
        let mut ambiguous: HashSet<u32> = HashSet::new();
        let mut note = |pid: u32, site: Site| {
            if pid == 0 {
                return;
            }
            if sites.insert(pid, site).is_some() {
                ambiguous.insert(pid);
            }
        };
        for s in &map {
            match s {
                Slot::Object(Entity::Vertex { pid, at, .. }) => {
                    note(*pid, Site::Vertex(*at as usize))
                }
                Slot::Object(Entity::Edge { pid, at, .. })
                | Slot::Object(Entity::Face { pid, at, .. }) => {
                    note(*pid, Site::Drawing(*at as usize))
                }
                Slot::Object(Entity::InstancePlaced {
                    pid,
                    tf_at,
                    drawbase_at,
                    ..
                }) => note(
                    *pid,
                    Site::Instance {
                        drawbase: *drawbase_at,
                        transform: *tf_at,
                    },
                ),
                _ => {}
            }
        }
        let materials = crate::extract::solid_material_sites(d)
            .into_iter()
            .collect();
        Ok(StreamDocument {
            bytes: d.to_vec(),
            sites,
            ambiguous,
            materials,
            notes: Vec::new(),
        })
    }

    /// A new document from a model, as a file of `release`
    /// (`docs/WRITING.md`, §14): any release the grammar holds
    /// ([`crate::Release::grammar`]), `Release::R2017` for the file
    /// SketchUp 2017 and every later release open; any other release is
    /// refused, naming the releases that are written. The model may come
    /// from either container or from another program. Whatever the model
    /// holds that a file of `release` cannot take from it is listed in
    /// [`notes`](Self::notes).
    pub fn from_model(model: &Model, release: crate::Release) -> Result<StreamDocument, Error> {
        Self::from_model_with(model, release, crate::WriteEnv::system())
    }

    /// [`from_model`](Self::from_model) with the save time and seed taken
    /// from `env` rather than the system: a reproducible file, or one
    /// written where there is no system clock.
    pub fn from_model_with(
        model: &Model,
        release: crate::Release,
        env: crate::WriteEnv,
    ) -> Result<StreamDocument, Error> {
        let built = crate::env::with_env(env, || crate::build17::from_model_for(model, release))?;
        let bytes = built.file.write()?;
        let mut doc = StreamDocument::open(&bytes)
            .map_err(|e| Error(format!("the written file does not read back: {e}")))?;
        doc.notes = built.notes;
        Ok(doc)
    }

    /// Check the file against the whole grammar (`docs/WRITING.md`):
    /// every object and every field between them is read, from the
    /// header to the last byte, and written back through a store map
    /// assigned anew; the result must be the file. An error names the
    /// first structure that does not fit, or an undecoded one
    /// (`SKP_FORMAT.md` §15).
    pub fn verify(&self) -> Result<(), Error> {
        verify(&self.bytes)
    }

    /// Write the file anew from its records: what [`verify`](Self::verify)
    /// compares with the file becomes the document. Refused for a file
    /// that holds an undecoded structure (`SKP_FORMAT.md` §15), which is
    /// named.
    pub fn repack(&mut self) -> Result<(), Error> {
        let written = repacked(&self.bytes)?;
        if written != self.bytes {
            // The document changes only once the new one stands.
            let mut doc = StreamDocument::open(&written)
                .map_err(|e| Error(format!("the written file does not read back: {e}")))?;
            doc.notes = std::mem::take(&mut self.notes);
            *self = doc;
        }
        Ok(())
    }

    /// The file's records as text, one structure after another in stream
    /// order: the analysis view of the whole grammar, for reading a file
    /// SketchUp wrote next to one written here. Image and schema payloads
    /// are shown by length.
    pub fn records(&self) -> Result<String, Error> {
        records(&self.bytes)
    }

    /// What the model held that the written file does not: entities left
    /// out, with the reason. Empty for a document that was opened.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// [`open`](Self::open) from a path.
    pub fn read(path: impl AsRef<std::path::Path>) -> Result<StreamDocument, Error> {
        let d = std::fs::read(path).map_err(|e| Error(e.to_string()))?;
        StreamDocument::open(&d)
    }

    /// The file's bytes.
    pub fn write(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    pub fn write_to(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, &self.bytes)
    }

    /// The document as a [`Model`].
    pub fn model(&self) -> Result<Model, Error> {
        Model::parse(&self.bytes)
    }

    fn site(&self, pid: u32) -> Result<Site, Error> {
        if self.ambiguous.contains(&pid) {
            return Err(Error(format!(
                "id {pid} names more than one entity in this file and cannot be edited"
            )));
        }
        self.sites
            .get(&pid)
            .copied()
            .ok_or_else(|| Error(format!("no entity with id {pid} in the document")))
    }

    fn put(&mut self, at: usize, bytes: &[u8]) -> Result<(), Error> {
        let slot = self
            .bytes
            .get_mut(at..at + bytes.len())
            .ok_or_else(|| Error(format!("field at {at:#x} lies outside the file")))?;
        slot.copy_from_slice(bytes);
        Ok(())
    }

    /// Move a vertex to `point_m` (metres).
    pub fn set_vertex(&mut self, pid: u32, point_m: [f64; 3]) -> Result<(), Error> {
        let Site::Vertex(at) = self.site(pid)? else {
            return Err(Error(format!("entity {pid} is not a vertex")));
        };
        let bytes: Vec<u8> = point_m
            .iter()
            .flat_map(|x| (x / M_PER_INCH).to_le_bytes())
            .collect();
        self.put(at, &bytes)
    }

    /// Replace an instance's 13-value transform (§7.3; translation in
    /// inches, as `Instance::transform` carries it).
    pub fn set_transform(&mut self, pid: u32, transform: [f64; 13]) -> Result<(), Error> {
        let Site::Instance { transform: at, .. } = self.site(pid)? else {
            return Err(Error(format!("entity {pid} is not an instance")));
        };
        let bytes: Vec<u8> = transform.iter().flat_map(|x| x.to_le_bytes()).collect();
        self.put(at, &bytes)
    }

    /// Hide or show an edge, face, group or component instance.
    pub fn set_hidden(&mut self, pid: u32, hidden: bool) -> Result<(), Error> {
        let at = match self.site(pid)? {
            Site::Drawing(at) | Site::Instance { drawbase: at, .. } => at,
            Site::Vertex(_) => return Err(Error(format!("entity {pid} is a vertex"))),
        };
        self.put(at + 2, &[hidden as u8])
    }

    /// Set a solid material's colour and opacity (0–1) by name (§8.1):
    /// the RGBA bytes, the stored transparency and its use flag.
    pub fn set_material_color(
        &mut self,
        name: &str,
        rgb: [u8; 3],
        opacity: f64,
    ) -> Result<(), Error> {
        if !opacity.is_finite() {
            return Err(Error(format!("opacity {opacity} is not a number")));
        }
        let at = *self
            .materials
            .get(name)
            .ok_or_else(|| Error(format!("no solid material {name:?} in the document")))?;
        let alpha = crate::build17::alpha_byte(opacity);
        self.put(at, &[rgb[0], rgb[1], rgb[2], alpha])?;
        self.put(at + 16, &(1.0 - opacity.clamp(0.0, 1.0)).to_le_bytes())?;
        self.put(at + 24, &[(opacity < 1.0) as u8])
    }
}
