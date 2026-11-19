//! Surgical edits on a 2026 [`Document`] (`docs/WRITING.md`): each
//! setter changes the one record or document attribute that holds the
//! value and leaves every other byte of the file as it was. Entities are
//! addressed by persistent id (the `pid` the `Model` reports), materials
//! and layers by name.

use crate::doc26::Document;
use crate::model::Error;
use crate::tree26::{Body, Record};
use crate::xml26::esc;

const M_PER_INCH: f64 = 0.0254;

/// The entity id of a record that starts with an entity base (`0x05dc`)
/// or a drawing-element header (`0x07d0 > 0x05dc`).
fn own_id(r: &Record) -> Option<u64> {
    let base = r
        .get(0x05DC)
        .or_else(|| r.get(0x07D0).and_then(|d| d.get(0x05DC)))?;
    base.get(0x05DE)?.uint()
}

/// Depth-first search for the record whose own entity id is `id`.
fn find_entity_mut(r: &mut Record, id: u64, depth: u32) -> Option<&mut Record> {
    if depth > 64 {
        return None;
    }
    if own_id(r) == Some(id) {
        return Some(r);
    }
    if let Body::Kids(kids) = &mut r.body {
        for k in kids.iter_mut() {
            if let Some(found) = find_entity_mut(k, id, depth + 1) {
                return Some(found);
            }
        }
    }
    None
}

fn missing(what: &str) -> Error {
    Error(format!("no {what} in the document"))
}

/// `r.set(rec)`, or an error when `r` is a leaf and cannot hold it.
fn put(r: &mut Record, rec: Record) -> Result<(), Error> {
    let tag = rec.tag;
    if r.set(rec) {
        Ok(())
    } else {
        Err(Error(format!(
            "record {:#06x} is a leaf and cannot hold {tag:#06x}",
            r.tag
        )))
    }
}

impl Document {
    /// Apply `edit` to the record tree and store the result. The edit
    /// receives the root record; it reports what it could not find.
    fn edit_tree(
        &mut self,
        edit: impl FnOnce(&mut Record) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut root = self.tree()?;
        edit(&mut root)?;
        self.set_tree(&root);
        Ok(())
    }

    /// Edit the record of the entity with persistent id `pid`.
    fn edit_entity(
        &mut self,
        pid: u32,
        edit: impl FnOnce(&mut Record) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.edit_tree(|root| {
            let r = find_entity_mut(root, pid as u64, 0)
                .ok_or_else(|| missing(&format!("entity with id {pid}")))?;
            edit(r)
        })
    }

    /// Move a vertex to `point_m` (metres).
    pub fn set_vertex(&mut self, pid: u32, point_m: [f64; 3]) -> Result<(), Error> {
        self.edit_entity(pid, |r| {
            if r.tag != 0x09C4 {
                return Err(Error(format!("entity {pid} is not a vertex")));
            }
            put(r, Record::f64s_of(0x09C5, &point_m.map(|x| x / M_PER_INCH)))
        })
    }

    /// Replace an instance's 13-value transform (§7.3; translation in
    /// inches, as `Instance::transform` carries it).
    pub fn set_transform(&mut self, pid: u32, transform: [f64; 13]) -> Result<(), Error> {
        self.edit_entity(pid, |r| {
            if r.tag != 0x1964 {
                return Err(Error(format!("entity {pid} is not an instance")));
            }
            put(r, Record::f64s_of(0x1966, &transform))
        })
    }

    /// Hide or show a drawing element (an edge, face, instance, group,
    /// section plane, text or dimension).
    pub fn set_hidden(&mut self, pid: u32, hidden: bool) -> Result<(), Error> {
        self.edit_entity(pid, |r| {
            let h = r
                .get_mut(0x07D0)
                .ok_or_else(|| Error(format!("entity {pid} is not a drawing element")))?;
            let flags = h.get(0x07D3).and_then(|f| f.uint()).unwrap_or(0) as u8;
            let flags = if hidden { flags | 0x01 } else { flags & !0x01 };
            put(h, Record::u8(0x07D3, flags))
        })
    }

    /// Rename a definition (component or group) by its current name.
    pub fn rename_definition(&mut self, name: &str, new_name: &str) -> Result<(), Error> {
        self.edit_tree(|root| {
            let list = root
                .path_mut(&[0x01F9, 0x1770, 0x1771])
                .ok_or_else(|| missing("definition list"))?;
            let def = list
                .children_mut()
                .into_iter()
                .flatten()
                .find(|d| {
                    d.tag == 0x157C && d.get(0x157E).map(|n| n.text()) == Some(name.to_string())
                })
                .ok_or_else(|| missing(&format!("definition {name:?}")))?;
            put(def, Record::string(0x157E, new_name))
        })
    }

    /// Show or hide a layer (tag) by name.
    pub fn set_layer_visible(&mut self, name: &str, visible: bool) -> Result<(), Error> {
        self.edit_tree(|root| {
            let list = root
                .path_mut(&[0x01F8, 0x3A98, 0x3A99])
                .ok_or_else(|| missing("layer list"))?;
            let layer = list
                .children_mut()
                .into_iter()
                .flatten()
                .find(|l| {
                    l.tag == 0x3C8C && l.get(0x3C8D).map(|n| n.text()) == Some(name.to_string())
                })
                .ok_or_else(|| missing(&format!("layer {name:?}")))?;
            put(layer, Record::u8(0x3C8E, (!visible) as u8))
        })
    }

    /// Set the current view's camera. Only the fields whose value differs
    /// from what the reader sees in the file are rewritten, so a camera
    /// read from the document and set back leaves every record as it was
    /// (the metre ↔ inch conversion is not exact in binary).
    pub fn set_camera(&mut self, camera: &crate::Camera) -> Result<(), Error> {
        self.edit_tree(|root| {
            let c = root
                .path_mut(&[0x01FA, 0x34BC])
                .ok_or_else(|| missing("camera"))?;
            // Points are metres in the model and inches in the file; the
            // reader divides by `INCH`, so the same division decides
            // whether a stored value already means the requested one.
            let stored3 =
                |c: &Record, tag: u16| -> Option<[f64; 3]> { c.get(tag)?.f64s().try_into().ok() };
            let stored1 =
                |c: &Record, tag: u16| -> Option<f64> { c.get(tag)?.f64s().first().copied() };
            let same3 = |c: &Record, tag: u16, want: [f64; 3], in_inches: bool| {
                stored3(c, tag)
                    .map(|s| if in_inches { s.map(|x| x / crate::INCH) } else { s } == want)
                    .unwrap_or(false)
            };
            let v3 = |m: [f64; 3]| m.map(|x| x / M_PER_INCH);
            if !same3(c, 0x34BD, camera.eye_m, true) {
                put(c, Record::f64s_of(0x34BD, &v3(camera.eye_m)))?;
            }
            if !same3(c, 0x34BE, camera.target_m, true) {
                put(c, Record::f64s_of(0x34BE, &v3(camera.target_m)))?;
            }
            let up = crate::build26::upright(camera);
            if !same3(c, 0x34BF, up, false) {
                put(c, Record::f64s_of(0x34BF, &up))?;
            }
            let flag = |c: &Record, tag: u16| c.get(tag).and_then(|f| f.uint()).unwrap_or(0) != 0;
            if flag(c, 0x34C2) != camera.perspective {
                put(c, Record::u8(0x34C2, camera.perspective as u8))?;
            }
            if stored1(c, 0x34C4) != Some(camera.fov_deg) {
                put(c, Record::f64(0x34C4, camera.fov_deg))?;
            }
            if stored1(c, 0x34C3).map(|h| h / crate::INCH) != Some(camera.parallel_height_m) {
                put(
                    c,
                    Record::f64(0x34C3, camera.parallel_height_m / M_PER_INCH),
                )?;
            }
            if flag(c, 0x34CA) != camera.two_point_perspective {
                put(c, Record::u8(0x34CA, camera.two_point_perspective as u8))?;
            }
            Ok(())
        })
    }

    /// Set a solid material's colour and opacity (0–1) by material name.
    /// The record tree is untouched: the values live in the material's
    /// document (§16.6).
    pub fn set_material_color(
        &mut self,
        name: &str,
        rgb: [u8; 3],
        opacity: f64,
    ) -> Result<(), Error> {
        if !opacity.is_finite() {
            return Err(Error(format!("opacity {opacity} is not a number")));
        }
        let entry = self
            .entry_names()
            .into_iter()
            .filter(|n| n.starts_with("materials/") && n.ends_with("/material.xml"))
            .map(str::to_string)
            .find(|n| {
                self.entry(n)
                    .and_then(|x| std::str::from_utf8(x).ok())
                    .map(|x| x.contains(&format!("<mat:material name=\"{}\"", esc(name))))
                    .unwrap_or(false)
            })
            .ok_or_else(|| missing(&format!("material {name:?}")))?;
        let xml = String::from_utf8_lossy(self.entry(&entry).unwrap()).into_owned();
        let mut out = xml.clone();
        let (trans, use_trans) = crate::xml26::trans_attrs(opacity);
        for (attr, value) in [
            ("colorRed", rgb[0].to_string()),
            ("colorGreen", rgb[1].to_string()),
            ("colorBlue", rgb[2].to_string()),
            ("trans", trans),
            ("useTrans", use_trans.to_string()),
        ] {
            out = replace_attr(&out, attr, &value)
                .ok_or_else(|| Error(format!("{entry}: no {attr} attribute")))?;
        }
        self.set_entry(&entry, out.into_bytes());
        Ok(())
    }
}

/// Replace the value of the first `attr="…"` in `xml`.
fn replace_attr(xml: &str, attr: &str, value: &str) -> Option<String> {
    let key = format!(" {attr}=\"");
    let start = xml.find(&key)? + key.len();
    let end = start + xml[start..].find('"')?;
    Some(format!("{}{}{}", &xml[..start], esc(value), &xml[end..]))
}
