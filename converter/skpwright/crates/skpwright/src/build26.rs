//! Authoring a 2026 document from a [`Model`] (`docs/WRITING.md`): every
//! top-level section of SKP_FORMAT §16.3 in the order SketchUp writes it,
//! decoded fields from the model and every undecoded field at the value
//! the corpus shows, plus the archive entries the records refer to
//! (material and style documents, texture and watermark images, the
//! thumbnails `meta/meta.dat` names).
//!
//! Ids: an entity keeps its persistent id when the model has one and it
//! is unused so far; everything else (materials, layers, fonts, styles,
//! scenes, vertices, definition containers) is numbered after the
//! highest persistent id. Scenes refer to entities by those ids, so the
//! model's `hidden_entities` and `active_section_planes` resolve.

use std::collections::{HashMap, HashSet};

use crate::doc26::Document;
use crate::model::Error;
use crate::settings::{
    Camera, RenderingOptions, RoKind, RoValue, ShadowInfo, Watermark, RO_FIELDS,
};
use crate::tree26::Record;
use crate::xml26;
use crate::{
    AttrValue, DefinitionKind, GeometryRun, Material, MeshFace, Model, Scene, SceneProperties,
};

const M_PER_INCH: f64 = 0.0254;

/// Shadow flags every observed drawing element carries (casts + receives).
const SHADOW_FLAGS: u8 = 0x06;

/// The shadow bits of a drawing element's flags (§16.4).
fn shadow_flags(casts: bool, receives: bool) -> u8 {
    (casts as u8) << 1 | (receives as u8) << 2
}

struct Ids {
    next: u64,
    used: HashSet<u64>,
}

impl Ids {
    fn fresh(&mut self) -> u64 {
        while self.used.contains(&self.next) {
            self.next += 1;
        }
        let id = self.next;
        self.used.insert(id);
        id
    }
    /// The model's persistent id when it is nonzero and still free, else
    /// a fresh one.
    fn keep(&mut self, pid: u32) -> u64 {
        let pid = pid as u64;
        if pid != 0 && self.used.insert(pid) {
            pid
        } else {
            self.fresh()
        }
    }
}

// ---------------------------------------------------------------- records

fn entity(id: u64) -> Record {
    Record::kids(0x05DC, vec![Record::narrow(0x05DE, id)])
}

fn entity_with(id: u64, attrs: Option<Record>) -> Record {
    let mut kids = vec![Record::narrow(0x05DE, id)];
    if let Some(a) = attrs {
        kids.push(a);
    }
    Record::kids(0x05DC, kids)
}

/// A drawing-element header (§16.4 `0x07d0`).
fn drawing(
    id: u64,
    material: Option<u64>,
    layer: Option<u64>,
    flags: u8,
    attrs: Option<Record>,
) -> Record {
    let mut kids = vec![entity_with(id, attrs)];
    if let Some(m) = material {
        kids.push(Record::narrow(0x07D1, m));
    }
    if let Some(l) = layer {
        kids.push(Record::narrow(0x07D2, l));
    }
    if flags != 0 {
        kids.push(Record::u8(0x07D3, flags));
    }
    Record::kids(0x07D0, kids)
}

/// The header of a drawing element that holds a layer, a material and a
/// hidden flag and casts and receives shadows.
#[allow(clippy::too_many_arguments)]
fn drawn(
    id: u64,
    layer: u16,
    material: Option<u16>,
    hidden: bool,
    shadows: (bool, bool),
    owner: Option<crate::Owner>,
    t: &mut Tables,
) -> Record {
    let attrs = owner.and_then(|o| t.attributes(o, None));
    drawing(
        id,
        material.and_then(|m| t.material_of_slot.get(&m).copied()),
        t.layer_of_slot.get(&layer).copied().flatten(),
        shadow_flags(shadows.0, shadows.1) | hidden as u8,
        attrs,
    )
}

/// Inches of a point the reader divides by [`INCH`]: each the stored
/// value the reader turns back into the very metres (§4c).
fn v3_in(m: [f64; 3]) -> Vec<f64> {
    m.map(inch).to_vec()
}

/// Inches of a point the reader multiplies by 0.0254: vertices, arcs and
/// guide points.
fn v3_in_v(m: [f64; 3]) -> Vec<f64> {
    m.map(inch_v).to_vec()
}

use crate::build17::{inch, inch_v};

fn rgba(tag: u16, c: [u8; 4]) -> Record {
    Record::leaf(tag, c.to_vec())
}

/// A dictionary of the model (§16.8).
fn dictionary_record(d: &crate::Dictionary, lost: &mut usize) -> Record {
    let mut kv = Vec::with_capacity(d.entries.len() * 2);
    for (k, v) in &d.entries {
        match model_value(v, lost) {
            Some(r) => {
                kv.push(Record::string(0x36B6, k));
                kv.push(r);
            }
            None => *lost += 1,
        }
    }
    Record::kids(
        0x36B3,
        vec![
            Record::empty(0x36B0),
            Record::string(0x36B4, &d.name),
            Record::kids(0x36B5, kv),
            Record::u32(0x36B7, d.flag),
        ],
    )
}

/// A value of the model as a typed value record: none for a value of a
/// type the container is not known to hold.
fn model_value(v: &crate::Value, lost: &mut usize) -> Option<Record> {
    use crate::Value;
    let one = |r: Record| Some(Record::kids(0x38A4, vec![r]));
    match v {
        Value::Nil => Some(Record::empty(0x38A4)),
        Value::Int(i) => one(Record::leaf(0x38A7, i.to_le_bytes().to_vec())),
        Value::Double(f) => one(Record::f64(0x38A9, *f)),
        Value::Bool(b) => one(Record::u8(0x38AA, *b as u8)),
        Value::Text(s) => one(Record::string(0x38AD, s)),
        Value::Array(items) => {
            let mut inner = Vec::with_capacity(items.len());
            for i in items {
                match model_value(i, lost) {
                    Some(r) => inner.push(r),
                    None => *lost += 1,
                }
            }
            one(Record::kids(0x38AE, inner))
        }
        Value::Float(_)
        | Value::Time(_)
        | Value::Bytes8(_)
        | Value::Vector(_)
        | Value::Direction(_) => None,
    }
}

/// A typed attribute value (§16.8).
fn value(v: &AttrValue) -> Record {
    Record::kids(
        0x38A4,
        vec![match v {
            AttrValue::Int(i) => Record::leaf(0x38A7, i.to_le_bytes().to_vec()),
            AttrValue::F64(f) => Record::f64(0x38A9, *f),
            AttrValue::Bool(b) => Record::u8(0x38AA, *b as u8),
        }],
    )
}

fn string_value(s: &str) -> Record {
    Record::kids(0x38A4, vec![Record::string(0x38AD, s)])
}

/// An attribute dictionary (`0x36b3`).
fn dictionary(name: &str, pairs: Vec<(&str, Record)>) -> Record {
    let mut kv = Vec::with_capacity(pairs.len() * 2);
    for (k, v) in pairs {
        kv.push(Record::string(0x36B6, k));
        kv.push(v);
    }
    Record::kids(
        0x36B3,
        vec![
            Record::empty(0x36B0),
            Record::string(0x36B4, name),
            Record::kids(0x36B5, kv),
            Record::u32(0x36B7, 0),
        ],
    )
}

/// An option set (`0x61aa`).
fn option_set(name: &str, pairs: Vec<(&str, Record)>) -> Record {
    let mut kv = Vec::with_capacity(pairs.len() * 2);
    for (k, v) in pairs {
        kv.push(Record::string(0x61AD, k));
        kv.push(v);
    }
    Record::kids(
        0x61AA,
        vec![Record::string(0x61AB, name), Record::kids(0x61AC, kv)],
    )
}

// ---------------------------------------------------------------- settings

/// A camera record (§16.9).
/// The camera's up vector at a right angle to its line of sight, unit
/// length: what SketchUp stores, and makes of any other on opening.
pub(crate) fn upright(c: &Camera) -> [f64; 3] {
    let d = [
        c.target_m[0] - c.eye_m[0],
        c.target_m[1] - c.eye_m[1],
        c.target_m[2] - c.eye_m[2],
    ];
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let unit = |v: [f64; 3]| {
        let l = len(v);
        (l.is_finite() && l > 1e-9).then(|| v.map(|x| x / l))
    };
    let Some(d) = unit(d) else {
        // No line of sight to be upright to.
        return unit(c.up).unwrap_or([0.0, 0.0, 1.0]);
    };
    let across = |up: [f64; 3]| {
        let along = up[0] * d[0] + up[1] * d[1] + up[2] * d[2];
        unit([
            up[0] - along * d[0],
            up[1] - along * d[1],
            up[2] - along * d[2],
        ])
    };
    let already = c.up[0] * d[0] + c.up[1] * d[1] + c.up[2] * d[2];
    if already.abs() < 1e-12 && (len(c.up) - 1.0).abs() < 1e-12 {
        // Upright as it stands: the bits of the file are kept.
        return c.up;
    }
    // An up vector along the line of sight says nothing: the world's up
    // stands in, and the world's north for a camera looking straight up
    // or down.
    across(c.up)
        .or_else(|| across([0.0, 0.0, 1.0]))
        .or_else(|| across([0.0, 1.0, 0.0]))
        .unwrap_or([0.0, 0.0, 1.0])
}

fn camera_record(c: &Camera) -> Record {
    let up = upright(c);
    Record::kids(
        0x34BC,
        vec![
            Record::f64s_of(0x34BD, &v3_in(c.eye_m)),
            Record::f64s_of(0x34BE, &v3_in(c.target_m)),
            Record::f64s_of(0x34BF, &up),
            Record::f64(0x34C0, c.undecoded[0]),
            Record::f64(0x34C1, c.undecoded[1]),
            Record::u8(0x34C2, c.perspective as u8),
            Record::f64(0x34C4, c.fov_deg),
            Record::f64(0x34C3, inch(c.parallel_height_m)),
            Record::f64(0x34C5, c.aspect_ratio),
            Record::u8(0x34C6, 1),
            Record::u8(0x34C7, 0),
            Record::empty(0x34C8),
            Record::f64(0x34C9, c.image_width),
            Record::u8(0x34CA, c.two_point_perspective as u8),
            Record::f64(0x34CB, c.scale_2d),
            Record::f64(0x34CC, c.offset_2d[0]),
            Record::f64(0x34CD, c.offset_2d[1]),
            Record::u8(0x34CE, 0),
        ],
    )
}

fn default_camera() -> Camera {
    Camera {
        eye_m: [-4.0, -6.0, 3.5],
        target_m: [0.0, 0.0, 0.5],
        up: [0.0, 0.0, 1.0],
        perspective: true,
        fov_deg: 35.0,
        parallel_height_m: 133.0 * M_PER_INCH,
        two_point_perspective: false,
        description: String::new(),
        undecoded: [1.0, 1000.0],
        aspect_ratio: 0.0,
        scale_2d: 1.0,
        offset_2d: [0.0; 2],
        image_width: 0.0,
    }
}

/// The rendering-options record (§16.10) in its observed order: the
/// decoded fields from `ro` over the constants every corpus file shares
/// (the values of the default template's style). `sketchy`: the style
/// these settings belong to draws its edges with strokes, which sets
/// `0x7342` (§16.10, `style-sketchy`).
fn rendering_record(ro: &RenderingOptions, sketchy: bool) -> Record {
    // (tag, default bytes) in record order.
    let u8v = |t: u16, v: u8| (t, vec![v]);
    let u32v = |t: u16, v: u32| (t, v.to_le_bytes().to_vec());
    let f64v = |t: u16, v: f64| (t, v.to_le_bytes().to_vec());
    let f32v = |t: u16, v: f32| (t, v.to_le_bytes().to_vec());
    let col = |t: u16, c: [u8; 4]| (t, c.to_vec());
    let mut fields: Vec<(u16, Vec<u8>)> = vec![
        u32v(0x733D, 2),
        u8v(0x733E, 0),
        u8v(0x733F, 1),
        u8v(0x7340, 1),
        u32v(0x7341, 1),
        u32v(0x7342, sketchy as u32),
        u8v(0x7343, 1),
        u8v(0x7344, 1),
        u8v(0x7345, 1),
        u8v(0x7346, 0),
        u8v(0x7347, 0),
        u32v(0x7348, 1),
        u8v(0x7349, 0),
        u8v(0x7380, 0),
        u8v(0x7381, 0),
        u8v(0x734B, 0),
        u8v(0x734C, 1),
        u8v(0x734D, 0),
        u32v(0x734E, 2),
        u8v(0x734F, 1),
        u32v(0x7350, 2),
        u8v(0x7351, 0),
        u32v(0x7352, 4),
        u8v(0x7353, 0),
        u32v(0x7354, 7),
        u8v(0x7355, 0),
        u8v(0x7356, 0),
        col(0x7357, [0xCC, 0xCC, 0xC9, 0xFF]),
        col(0x7358, [0, 0, 0, 0xFF]),
        col(0x7359, [0, 1, 0xFF, 0xFF]),
        col(0x735A, [0xFF, 0, 0, 0xFF]),
        col(0x735B, [0, 0, 0, 0xFF]),
        col(0x735C, [0xFF, 0xFF, 0xFF, 0xFF]),
        col(0x735D, [0xA4, 0xB2, 0xBB, 0xFF]),
        u8v(0x735E, 1),
        u8v(0x735F, 0),
        col(0x7360, [0, 0, 0, 0]),
        u8v(0x7361, 1),
        f64v(0x7362, -1.0),
        f64v(0x7363, -1.0),
        u32v(0x7364, 1),
        col(0x7365, [0xC6, 0xCD, 0xD0, 0xFF]),
        u32v(0x7366, 0),
        col(0x7367, [0xD2, 0xD0, 0xB9, 0xFF]),
        u8v(0x7368, 1),
        u8v(0x7369, 0),
        u8v(0x736A, 1),
        u32v(0x736B, 50),
        f64v(0x736C, 0.25),
        f64v(0x736D, 0.5),
        u8v(0x736E, 0),
        u8v(0x736F, 0),
        col(0x7370, [0xFF, 0x87, 0, 0xFF]),
        col(0x7371, [0x70, 0x69, 0x61, 0xFF]),
        col(0x7372, [0, 0, 0, 0xFF]),
        col(0x7373, [0, 0, 0, 0xFF]),
        u32v(0x7374, 3),
        u32v(0x7375, 2),
        u8v(0x7376, 0),
        u32v(0x7377, 0),
        f64v(0x7378, 0.65),
        u8v(0x7379, 1),
        // 20° as SketchUp stores it, one bit above 20.0f64.to_radians().
        f64v(0x737A, f64::from_bits(0x3FD6_5718_4AE7_4488)),
        u8v(0x737B, 1),
        u8v(0x737C, 1),
        f64v(0x737D, 1.0),
        u8v(0x737E, 1),
        f64v(0x737F, 0.8),
        u8v(0x7382, 0),
        u8v(0x7383, 0),
        f32v(0x7385, 10.0),
        u32v(0x7386, 0),
        u8v(0x738B, 0),
        f32v(0x7389, 1.0),
        col(0x738A, [0, 0, 0, 0xFF]),
    ];
    // The default section fill style flag follows the section fill colour
    // when the settings name it (2027 files); older files lack it.
    if ro.default_section_fill_style.is_some() {
        let at = fields
            .iter()
            .position(|(t, _)| *t == 0x7373)
            .map_or(fields.len(), |p| p + 1);
        fields.insert(at, u8v(0x738C, 0));
    }
    for (field, (_, v)) in RO_FIELDS.iter().zip(ro.values()) {
        let Some(v) = v else { continue };
        // Encoded by the record's kind: a bool-typed setting can live in a
        // u32 record (`edges`), so the value's variant is not the width.
        // Distances are metres in the model and inches in the file; a
        // negative value means unset and stays as it is.
        let v = match v {
            RoValue::F64(f) if field.name.ends_with("_m") && f >= 0.0 => RoValue::F64(inch(f)),
            v => v,
        };
        let bytes = ro_bytes(field.kind, v);
        if let Some(slot) = fields.iter_mut().find(|(t, _)| *t == field.tag) {
            slot.1 = bytes;
        }
    }
    // The mode applies while the edges are on (`edges` wins when off).
    if let Some(m) = ro.edges_mode.filter(|_| ro.edges) {
        if let Some(slot) = fields.iter_mut().find(|(t, _)| *t == 0x7341) {
            slot.1 = m.to_le_bytes().to_vec();
        }
    }
    Record::kids(
        0x733C,
        fields
            .into_iter()
            .map(|(t, b)| Record::leaf(t, b))
            .collect(),
    )
}

/// The bytes of a rendering-option value in a record of `kind`.
fn ro_bytes(kind: RoKind, v: RoValue) -> Vec<u8> {
    let n = match v {
        RoValue::Bool(b) => b as u32,
        RoValue::U32(x) => x,
        RoValue::Rgba(c) => u32::from_le_bytes(c),
        RoValue::F64(f) => f as u32,
        RoValue::F32(f) => f as u32,
    };
    match (kind, v) {
        (RoKind::Bool, _) => vec![(n != 0) as u8],
        (RoKind::U32, _) | (RoKind::Rgba, _) => n.to_le_bytes().to_vec(),
        (RoKind::F64, RoValue::F64(f)) => f.to_le_bytes().to_vec(),
        (RoKind::F64, _) => (n as f64).to_le_bytes().to_vec(),
        (RoKind::F32, RoValue::F32(f)) => f.to_le_bytes().to_vec(),
        (RoKind::F32, _) => (n as f32).to_le_bytes().to_vec(),
    }
}

/// A shadow-info record (§16.11).
fn shadow_record(s: &ShadowInfo) -> Record {
    Record::kids(
        0x6590,
        vec![
            Record::u32(0x6591, s.time),
            Record::u8(0x6592, s.daylight_saving as u8),
            Record::string(0x6593, &s.city),
            Record::string(0x6594, &s.country),
            Record::f64(0x6595, s.longitude),
            Record::f64(0x6596, s.latitude),
            Record::f64(0x6597, s.tz_offset_h),
            Record::f64s_of(0x6598, &s.north),
            Record::u8(0x6599, s.displayed as u8),
            Record::u8(0x659A, s.north_displayed as u8),
            Record::u8(0x659B, s.on_faces as u8),
            Record::u8(0x659C, s.on_ground as u8),
            Record::u8(0x659D, s.from_edges as u8),
            Record::u32(0x659E, s.light),
            Record::u32(0x659F, s.dark),
            Record::u8(0x65A0, s.use_sun_for_shading as u8),
        ],
    )
}

fn default_shadows() -> ShadowInfo {
    ShadowInfo {
        time: 1794144600,
        city: "Boulder (CO)".into(),
        country: "USA".into(),
        longitude: -105.24213888888889,
        latitude: 40.018_308_333_333_34,
        tz_offset_h: -7.0,
        displayed: false,
        on_faces: true,
        on_ground: true,
        from_edges: false,
        light: 80,
        dark: 45,
        use_sun_for_shading: false,
        daylight_saving: false,
        north: [0.0, 1.0, 0.0],
        north_displayed: false,
    }
}

/// An axes record (§16.3 `0x4650`).
fn axes_record(a: Option<&crate::Axes>) -> Record {
    let (o, x, y, z) = match a {
        Some(a) => (v3_in(a.origin_m), a.x.to_vec(), a.y.to_vec(), a.z.to_vec()),
        None => (
            vec![0.0; 3],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ),
    };
    Record::kids(
        0x4650,
        vec![
            Record::kids(0x07D0, vec![Record::u8(0x07D3, SHADOW_FLAGS)]),
            Record::f64s_of(0x4651, &o),
            Record::f64s_of(0x4652, &x),
            Record::f64s_of(0x4653, &y),
            Record::f64s_of(0x4654, &z),
        ],
    )
}

/// The twelve stock line styles of every file (§16.14).
fn line_styles(ids: &mut Ids) -> Record {
    const STOCK: [(&str, &str); 12] = [
        ("Solid Basic", "16.0"),
        ("Short dash", "6.0, -6.0"),
        ("Dash", "12.0, -6.0"),
        ("Dot", "1.0, -6.0"),
        ("Dash dot", "12.0, -6.0, 1.0, -6.0"),
        ("Dash double-dot", "12.0, -6.0, 1.0, -6.0, 1.0, -6.0"),
        (
            "Dash triple-dot",
            "12.0, -6.0, 1.0, -6.0, 1.0, -6.0, 1.0, -6.0",
        ),
        ("Double-dash dot", "12.0, -6.0, 12.0, -6.0, 1.0, -6.0"),
        (
            "Double-dash double-dot",
            "12.0, -6.0, 12.0, -6.0, 1.0, -6.0, 1.0, -6.0",
        ),
        (
            "Double-dash triple-dot",
            "12.0, -6.0, 12.0, -6.0, 1.0, -6.0, 1.0, -6.0, 1.0, -6.0",
        ),
        ("Long-dash dash", "36.0, -10.0, 12.0, -10.0"),
        (
            "Long-dash double-dash",
            "36.0, -10.0, 12.0, -10.0, 12.0, -10.0",
        ),
    ];
    let styles = STOCK
        .iter()
        .map(|(name, pattern)| {
            Record::kids(
                0x4076,
                vec![
                    entity(ids.fresh()),
                    Record::string(0x4077, name),
                    Record::string(0x4078, pattern),
                    Record::f64(0x407A, 1.0),
                    Record::f64(0x4079, 1.0),
                    rgba(0x407B, [0, 0, 0, 0xFF]),
                    Record::u8(0x407C, 0),
                ],
            )
        })
        .collect();
    Record::kids(
        0x0208,
        vec![Record::kids(0x4074, vec![Record::kids(0x4075, styles)])],
    )
}

/// The archive folder SketchUp derives from a material name: `* < > :`
/// (and path separators) become `_`; a name already taken gets a numeric
/// suffix.
/// A folder name for an entry path: path separators and the characters
/// Windows forbids become `_`, the two dot names become `_`, and names
/// already taken (compared without case, since SketchUp extracts entries
/// onto file systems that may fold it) get a numeric suffix.
/// A sketchy-edge style's stroke images beside its document (§16.14).
fn stroke_entries(doc: &mut Document, folder: &str, strokes: Option<&crate::Strokes>) {
    let Some(st) = strokes else { return };
    for (k, png) in st.images.iter().enumerate() {
        doc.set_entry(
            &format!("styles/{folder}/{}", xml26::stroke_file(st, k)),
            png.clone(),
        );
    }
}

pub(crate) fn folder_name(name: &str, taken: &mut HashSet<String>) -> String {
    let base: String = name
        .chars()
        .map(|c| match c {
            '*' | '<' | '>' | ':' | '/' | '\\' | '?' | '"' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let base = if base.is_empty() || base == "." || base == ".." {
        "_".to_string()
    } else {
        base
    };
    let mut candidate = base.clone();
    let mut n = 1;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{base}{n}");
        n += 1;
    }
    candidate
}

pub(crate) fn guid_bytes(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub(crate) fn random_guid(seed: &mut u64) -> [u8; 16] {
    let mut g = [0u8; 16];
    for chunk in g.chunks_mut(4) {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = ((*seed >> 33) as u32) ^ u32::from_le_bytes(crate::env::random_bytes());
        chunk.copy_from_slice(&r.to_le_bytes());
    }
    g
}

// ---------------------------------------------------------------- the build

/// What the model-level tables resolve to in the new file.
struct Tables {
    /// Material slot → material id (None for the default material).
    material_of_slot: HashMap<u16, u64>,
    /// Layer slot → layer id, `None` for the default layer.
    layer_of_slot: HashMap<u16, Option<u64>>,
    /// Definition ref (`defref`) → definition container id.
    container_of_defref: HashMap<u32, u64>,
    /// Definition index → definition container id.
    container_of_def: Vec<u64>,
    font_ids: Vec<u64>,
    default_font: u64,
    /// Instance name by persistent id.
    instance_names: HashMap<u32, String>,
    /// Entity ids by persistent id, for scene references and anchors. A
    /// 2017 file can reuse a persistent id across definitions; scenes name
    /// the model's own entities, so the root container's entry wins.
    id_of_pid: HashMap<u32, u64>,
    /// Vertex ids by persistent id (texts and dimensions anchor to them),
    /// kept apart so a vertex never satisfies a scene reference.
    vertex_of_pid: HashMap<u32, u64>,
    /// Instance ids by persistent id, for anchor instance paths.
    instance_of_pid: HashMap<u32, u64>,
    /// The model's dictionaries, by owner.
    dicts: HashMap<crate::Owner, Vec<crate::Dictionary>>,
    /// Dictionary values of a type the 2026 container is not known to
    /// hold, left out.
    lost_values: usize,
    /// Relationships left out: an entity they name is not in their list.
    lost_relationships: usize,
    /// The runs placed from the root (`live_runs`).
    live: HashSet<usize>,
    /// Each run's bounds with its placed children, once computed.
    bounds: HashMap<usize, Option<[[f64; 3]; 2]>>,
    /// Radial dimensions whose edge the container does not hold.
    lost_radial_dimensions: usize,
    /// Radial dimensions holding their arc rather than an edge (§10.12),
    /// a form not observed in the 2026 container.
    lost_edgeless_radials: usize,
    /// Texts and dimensions with an anchor on where two edges meet whose
    /// second edge the file does not hold.
    lost_crossings: usize,
    /// The id of each hatch pattern of the model's features.
    hatch_ids: Vec<u64>,
    /// The hatch pattern of an instance, by its place in its run.
    instance_hatch: HashMap<crate::Owner, usize>,
}

impl Tables {
    /// The attribute record of an owner (§16.8): its dictionaries, and
    /// `more` (a face's texture placement); none when there is nothing.
    fn attributes(&mut self, owner: crate::Owner, more: Option<Record>) -> Option<Record> {
        let dicts = self.dicts.get(&owner).cloned().unwrap_or_default();
        if dicts.is_empty() && more.is_none() {
            return None;
        }
        let mut set: Vec<Record> = dicts
            .iter()
            .map(|d| dictionary_record(d, &mut self.lost_values))
            .collect();
        set.extend(more);
        Some(Record::kids(
            0x05DD,
            vec![Record::kids(0x36B1, vec![Record::kids(0x36B2, set)])],
        ))
    }

    fn note(&mut self, pid: u32, id: u64, is_root: bool) {
        if pid == 0 {
            return;
        }
        if is_root {
            self.id_of_pid.insert(pid, id);
        } else {
            self.id_of_pid.entry(pid).or_insert(id);
        }
    }
}

/// Everything an entity container (§16.4) produces.
#[allow(clippy::too_many_arguments)]
fn container(
    run: &GeometryRun,
    model: &Model,
    t: &mut Tables,
    ids: &mut Ids,
    seed: &mut u64,
    is_root: bool,
    root_attrs: Option<Record>,
    container_id: u64,
) -> Record {
    let m = &run.mesh;
    // The run's place in the model, which its dictionaries are keyed by;
    // a definition without a run has none.
    let ri = model.geometry.iter().position(|r| std::ptr::eq(r, run));
    let mut kids = Vec::new();
    if is_root {
        // The model's own container has no id of its own (§16.4).
        let mut base = Vec::new();
        if let Some(a) = root_attrs {
            base.push(a);
        }
        kids.push(Record::kids(
            0x07D0,
            vec![Record::kids(0x05DC, base), Record::u8(0x07D3, SHADOW_FLAGS)],
        ));
    } else {
        kids.push(drawing(container_id, None, None, SHADOW_FLAGS, root_attrs));
    }

    // Vertices keep their ids: anchors name them.
    let vids: Vec<u64> = (0..m.vertices.len())
        .map(|i| {
            let pid = m.vertex_pids.get(i).copied().unwrap_or(0);
            let id = ids.keep(pid);
            if pid != 0 {
                if is_root {
                    t.vertex_of_pid.insert(pid, id);
                } else {
                    t.vertex_of_pid.entry(pid).or_insert(id);
                }
            }
            id
        })
        .collect();
    let vertices: Vec<Record> = m
        .vertices
        .iter()
        .zip(&vids)
        .enumerate()
        .map(|(i, (_, &id))| {
            let attrs = ri.and_then(|r| t.attributes(crate::Owner::Vertex(r, i), None));
            Record::kids(
                0x09C4,
                vec![
                    entity_with(id, attrs),
                    Record::f64s_of(0x09C5, &m.inches_of(i, inch_v)),
                ],
            )
        })
        .collect();
    if !vertices.is_empty() {
        kids.push(Record::kids(0x1389, vertices));
    }

    // Edges get their ids first (curves name their first/last member).
    let eids: Vec<u64> = m.edges.iter().map(|e| ids.keep(e.pid)).collect();
    for (e, &id) in m.edges.iter().zip(&eids) {
        t.note(e.pid, id, is_root);
    }
    let cids: Vec<u64> = run.curves.iter().map(|c| ids.keep(c.pid)).collect();
    // A dimension can anchor on a curve (an arc's center).
    for (c, &id) in run.curves.iter().zip(&cids) {
        t.note(c.pid, id, is_root);
    }
    // Member edges per curve, in chain order: a curve record names its
    // first and last edge along the chain (§16.4), which SketchUp checks
    // — a curve whose ends do not match is dropped on load.
    let mut member_idx: Vec<Vec<usize>> = vec![Vec::new(); run.curves.len()];
    for (i, e) in m.edges.iter().enumerate() {
        if let Some(c) = e.curve {
            if let Some(list) = member_idx.get_mut(c as usize) {
                list.push(i);
            }
        }
    }
    let members: Vec<Vec<u64>> = member_idx
        .iter()
        .map(|list| {
            chain_edges(list, &m.edges)
                .into_iter()
                .map(|i| eids[i])
                .collect()
        })
        .collect();
    let mut curve_record = |i: usize, c: &crate::Curve| {
        let list = &members[i];
        let attrs = ri.and_then(|r| t.attributes(crate::Owner::Curve(r, i), None));
        Record::kids(
            0x4A38,
            vec![
                entity_with(cids[i], attrs),
                Record::u32(
                    0x4A39,
                    if list.is_empty() {
                        c.members
                    } else {
                        list.len() as u32
                    },
                ),
                Record::u8(0x4A3A, 0),
                Record::narrow(0x4A3B, list.first().copied().unwrap_or(0)),
                Record::narrow(0x4A3C, list.last().copied().unwrap_or(0)),
            ],
        )
    };
    let plain: Vec<Record> = run
        .curves
        .iter()
        .enumerate()
        .filter(|(_, c)| c.arc.is_none())
        .map(|(i, c)| curve_record(i, c))
        .collect();
    if !plain.is_empty() {
        kids.push(Record::kids(0x1396, plain));
    }
    let arcs: Vec<Record> = run
        .curves
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.arc.as_ref().map(|a| (i, c, a)))
        .map(|(i, c, a)| {
            let center = v3_in_v(a.center_m);
            let x = v3_in_v(a.x_axis_m);
            let y = v3_in_v(a.y_axis_m);
            let n = a.normal;
            let offset = -(n[0] * center[0] + n[1] * center[1] + n[2] * center[2]);
            let mut frame = Vec::with_capacity(16);
            frame.extend(center);
            frame.extend(n);
            frame.push(offset);
            frame.extend(x);
            frame.extend(y);
            frame.push(inch_v(a.radius_m));
            frame.push(a.start_rad);
            frame.push(a.end_rad);
            Record::kids(
                0x4C2C,
                vec![curve_record(i, c), Record::f64s_of(0x4C2D, &frame)],
            )
        })
        .collect();
    if !arcs.is_empty() {
        kids.push(Record::kids(0x1397, arcs));
    }

    // Edges.
    let mut edge_of_ends: HashMap<(u32, u32), usize> = HashMap::new();
    for (i, e) in m.edges.iter().enumerate() {
        edge_of_ends
            .entry((e.v0.min(e.v1), e.v0.max(e.v1)))
            .or_insert(i);
    }
    let edges: Vec<Record> = m
        .edges
        .iter()
        .zip(&eids)
        .enumerate()
        .map(|(k, (e, &id))| {
            let mut flags = shadow_flags(e.casts_shadows, e.receives_shadows);
            if e.hidden {
                flags |= 0x01;
            }
            if e.soft {
                flags |= 0x08;
            }
            if e.smooth {
                flags |= 0x10;
            }
            let attrs = ri.and_then(|r| t.attributes(crate::Owner::Edge(r, k), None));
            let mut k = vec![
                drawing(
                    id,
                    e.material.and_then(|m| t.material_of_slot.get(&m).copied()),
                    t.layer_of_slot.get(&e.layer).copied().flatten(),
                    flags,
                    attrs,
                ),
                Record::narrow(0x0BB9, vids[e.v0 as usize]),
                Record::narrow(0x0BBA, vids[e.v1 as usize]),
            ];
            if let Some(c) = e.curve {
                if let Some(&cid) = cids.get(c as usize) {
                    k.push(Record::narrow(0x0BBB, cid));
                }
            }
            Record::kids(0x0BB8, k)
        })
        .collect();
    if !edges.is_empty() {
        kids.push(Record::kids(0x138A, edges));
    }

    // Faces.
    let mut faces = Vec::with_capacity(m.faces.len());
    let mut fids = Vec::with_capacity(m.faces.len());
    for (k, f) in m.faces.iter().enumerate() {
        let id = ids.keep(f.pid);
        t.note(f.pid, id, is_root);
        fids.push(id);
        faces.push(face_record(
            f,
            id,
            m,
            &edge_of_ends,
            &eids,
            t,
            ri.map(|r| crate::Owner::Face(r, k)),
        ));
    }
    if !faces.is_empty() {
        kids.push(Record::kids(0x138B, faces));
    }

    // Instances: components, groups, images.
    let mut instance = |pid: u32,
                        defref: u32,
                        transform: &[f64; 13],
                        material: u16,
                        hidden: bool,
                        layer: u16,
                        owner: Option<crate::Owner>,
                        own: Option<(bool, bool, bool, &str)>,
                        ids: &mut Ids,
                        t: &mut Tables|
     -> (u64, Record) {
        let id = ids.keep(pid);
        t.note(pid, id, is_root);
        if pid != 0 {
            if is_root {
                t.instance_of_pid.insert(pid, id);
            } else {
                t.instance_of_pid.entry(pid).or_insert(id);
            }
        }
        // A placed image receives shadows and casts none (§16.4, as
        // the 2017 drawbase of §5.2).
        let mut flags = match own {
            Some((casts, receives, locked, _)) => {
                (casts as u8) << 1 | (receives as u8) << 2 | (locked as u8) << 5
            }
            None => 0x04,
        };
        if hidden {
            flags |= 0x01;
        }
        let def_id = t.container_of_defref.get(&defref).copied().unwrap_or(0);
        let attrs = owner.and_then(|o| t.attributes(o, None));
        let rec = Record::kids(
            0x1964,
            vec![
                drawing(
                    id,
                    t.material_of_slot.get(&material).copied(),
                    t.layer_of_slot.get(&layer).copied().flatten(),
                    flags,
                    attrs,
                ),
                Record::string(
                    0x1965,
                    t.instance_names.get(&pid).map(String::as_str).unwrap_or(""),
                ),
                Record::f64s_of(0x1966, transform),
                Record::narrow(0x1967, def_id),
                Record::leaf(
                    0x1968,
                    own.and_then(|(_, _, _, g)| guid_bytes(g))
                        .unwrap_or_else(|| random_guid(seed))
                        .to_vec(),
                ),
            ]
            .into_iter()
            .chain(
                owner
                    .and_then(|o| t.instance_hatch.get(&o))
                    .and_then(|&h| t.hatch_ids.get(h))
                    .map(|&id| Record::narrow(0x1969, id)),
            )
            .collect(),
        );
        (id, rec)
    };
    let mut components = Vec::new();
    let mut groups = Vec::new();
    let mut comp_ids = Vec::new();
    let mut group_ids = Vec::new();
    // This container's own ids by persistent id: a 2017 file can reuse a
    // persistent id in another definition, and relationships name
    // entities of their own list.
    let mut local: HashMap<u32, u64> = HashMap::new();
    for (k, p) in run.placed.iter().enumerate() {
        let owner = ri.map(|r| crate::Owner::Instance(r, k));
        let own = Some((
            p.casts_shadows,
            p.receives_shadows,
            p.locked,
            p.guid.as_str(),
        ));
        let (id, rec) = instance(
            p.pid,
            p.defref,
            &p.transform,
            p.material,
            p.hidden,
            p.layer,
            owner,
            own,
            ids,
            t,
        );
        local.entry(p.pid).or_insert(id);
        if p.is_group {
            group_ids.push(id);
            groups.push(Record::kids(0x1D4C, vec![rec]));
        } else {
            comp_ids.push(id);
            components.push(rec);
        }
    }
    let mut image_ids = Vec::new();
    let images: Vec<Record> = run
        .images
        .iter()
        .map(|im| {
            let owner = ri.map(|r| crate::Owner::Image(r, image_ids.len()));
            let (id, rec) = instance(
                im.pid,
                im.defref,
                &im.transform,
                0,
                im.hidden,
                im.layer,
                owner,
                Some((im.casts_shadows, im.receives_shadows, false, "")),
                ids,
                t,
            );
            image_ids.push(id);
            local.entry(im.pid).or_insert(id);
            Record::kids(0x1F40, vec![rec])
        })
        .collect();
    if !components.is_empty() {
        kids.push(Record::kids(0x138C, components));
    }
    if !groups.is_empty() {
        kids.push(Record::kids(0x138D, groups));
    }
    if !images.is_empty() {
        kids.push(Record::kids(0x1390, images));
    }

    // Annotations belong to the container whose definition index they
    // carry; a definition without a geometry run has none (its stand-in
    // run carries `None`, which names the model).
    let owns = |def_index: Option<usize>| {
        (is_root || run.def_index.is_some()) && def_index == run.def_index
    };

    // Guide lines and points.
    let mut guide_ids = Vec::new();
    let guides: Vec<Record> = model
        .guides
        .iter()
        .enumerate()
        .filter(|(_, g)| owns(g.def_index))
        .map(|(gi, g)| {
            let id = ids.keep(g.pid);
            t.note(g.pid, id, is_root);
            guide_ids.push(id);
            let mut v = v3_in(g.point_m);
            v.extend(g.direction);
            v.extend(g.bounds);
            Record::kids(
                0x4269,
                vec![
                    Record::kids(
                        0x4268,
                        vec![drawn(
                            id,
                            g.layer,
                            g.material,
                            g.hidden,
                            (g.casts_shadows, g.receives_shadows),
                            Some(crate::Owner::Guide(gi)),
                            t,
                        )],
                    ),
                    Record::f64s_of(0x426A, &v),
                    Record::u16(0x426B, g.stipple),
                ],
            )
        })
        .collect();
    if !guides.is_empty() {
        kids.push(Record::kids(0x1391, guides));
    }
    let mut point_ids = Vec::new();
    let points: Vec<Record> = run
        .guide_points
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let id = ids.keep(p.pid);
            t.note(p.pid, id, is_root);
            point_ids.push(id);
            Record::kids(
                0x426C,
                vec![
                    Record::kids(
                        0x4268,
                        vec![drawn(
                            id,
                            p.layer,
                            p.material,
                            p.hidden,
                            (p.casts_shadows, p.receives_shadows),
                            ri.map(|r| crate::Owner::GuidePoint(r, k)),
                            t,
                        )],
                    ),
                    Record::f64s_of(0x426D, &v3_in_v(p.point_m)),
                    Record::f64s_of(0x426E, &v3_in_v(p.anchor_m)),
                    Record::u8(0x426F, p.flag),
                ],
            )
        })
        .collect();
    if !points.is_empty() {
        kids.push(Record::kids(0x1392, points));
    }

    // 3D polylines (§16.4 `0x1395`), between the guide points and the
    // section planes as SketchUp lists them.
    let mut polyline_ids = Vec::new();
    let polylines: Vec<Record> = run
        .polylines
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let id = ids.keep(p.pid);
            t.note(p.pid, id, is_root);
            polyline_ids.push(id);
            let points: Vec<f64> = p.points_m.iter().flat_map(|q| v3_in(*q)).collect();
            Record::kids(
                0x4844,
                vec![
                    drawn(
                        id,
                        p.layer,
                        p.material,
                        p.hidden,
                        (p.casts_shadows, p.receives_shadows),
                        ri.map(|r| crate::Owner::Polyline(r, k)),
                        t,
                    ),
                    Record::f64s_of(0x4845, &points),
                ],
            )
        })
        .collect();
    if !polylines.is_empty() {
        kids.push(Record::kids(0x1395, polylines));
    }

    // Section planes.
    let mut section_ids = Vec::new();
    let sections: Vec<Record> = run
        .sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let id = ids.keep(s.pid);
            t.note(s.pid, id, is_root);
            section_ids.push(id);
            let name = if s.name.is_empty() {
                format!("Section {}", i + 1)
            } else {
                s.name.clone()
            };
            let symbol = if s.symbol.is_empty() {
                (i + 1).to_string()
            } else {
                s.symbol.clone()
            };
            Record::kids(
                0x445C,
                vec![
                    drawn(
                        id,
                        s.layer,
                        s.material,
                        s.hidden,
                        (s.casts_shadows, s.receives_shadows),
                        ri.map(|r| crate::Owner::SectionPlane(r, i)),
                        t,
                    ),
                    Record::f64s_of(0x445D, &s.plane),
                    Record::string(0x445E, &name),
                    Record::string(0x445F, &symbol),
                ]
                .into_iter()
                .chain(
                    s.hatch
                        .and_then(|h| t.hatch_ids.get(h))
                        .map(|&id| Record::narrow(0x4460, id)),
                )
                .collect(),
            )
        })
        .collect();
    if !sections.is_empty() {
        kids.push(Record::kids(0x1393, sections));
    }

    // Texts and dimensions of this container.
    let mut text_ids = Vec::new();
    let mut dim_ids = Vec::new();
    let mut radial_ids = Vec::new();
    {
        let texts: Vec<Record> = model
            .texts
            .iter()
            .enumerate()
            .filter(|(_, x)| owns(x.def_index))
            .filter_map(|(xi, x)| {
                if !crossing_held(&x.anchor, t) {
                    t.lost_crossings += 1;
                    return None;
                }
                let id = ids.keep(x.pid);
                t.note(x.pid, id, is_root);
                text_ids.push(id);
                Some(text_record(x, id, xi, t, model))
            })
            .collect();
        if !texts.is_empty() {
            kids.push(Record::kids(0x1398, texts));
        }
        let dims: Vec<Record> = model
            .dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| owns(d.def_index))
            .filter_map(|(di, d)| {
                if !(crossing_held(&d.start, t) && crossing_held(&d.end, t)) {
                    t.lost_crossings += 1;
                    return None;
                }
                let id = ids.keep(d.pid);
                t.note(d.pid, id, is_root);
                dim_ids.push(id);
                Some(dimension_record(d, id, di, t, model))
            })
            .collect();
        if !dims.is_empty() {
            kids.push(Record::kids(0x1399, dims));
        }
        // Radial dimensions, each naming an edge of this container.
        let edge_id: HashMap<u32, u64> = m
            .edges
            .iter()
            .map(|e| e.pid)
            .zip(eids.iter().copied())
            .collect();
        let radials: Vec<Record> = model
            .radial_dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| owns(d.def_index))
            .filter_map(|(di, d)| {
                let Some(&edge) = d.edge.and_then(|p| edge_id.get(&p)) else {
                    if d.edge.is_none() && d.arc.is_some() {
                        t.lost_edgeless_radials += 1;
                    } else {
                        t.lost_radial_dimensions += 1;
                    }
                    return None;
                };
                let id = ids.keep(d.pid);
                t.note(d.pid, id, is_root);
                radial_ids.push(id);
                Some(radial_dimension_record(d, id, di, edge, t))
            })
            .collect();
        if !radials.is_empty() {
            kids.push(Record::kids(0x139A, radials));
        }
    }

    // Snaps (§16.19), of the container that holds them.
    let mut snap_ids = Vec::new();
    {
        let here = model
            .definitions
            .iter()
            .position(|d| run.def_index.is_some() && d.map_index == run.def_index);
        let snaps: Vec<Record> = model
            .features
            .snaps
            .iter()
            .filter(|sn| {
                if is_root {
                    sn.definition.is_none()
                } else {
                    sn.definition.is_some() && sn.definition == here
                }
            })
            .map(|sn| {
                let id = ids.keep(sn.pid);
                t.note(sn.pid, id, is_root);
                snap_ids.push(id);
                Record::kids(
                    0x7530,
                    vec![
                        drawing(id, None, None, SHADOW_FLAGS, None),
                        Record::f64s_of(0x7531, &v3_in(sn.position_m)),
                        Record::f64s_of(0x7532, &sn.normal),
                        Record::f64s_of(0x7533, &sn.third_axis),
                        Record::f64s_of(0x7534, &sn.x_axis),
                        Record::u32(0x7535, sn.kind),
                    ],
                )
            })
            .collect();
        if !snaps.is_empty() {
            kids.push(Record::kids(0x139D, snaps));
        }
    }

    kids.push(Record::empty(0x139E));
    let mut order: Vec<u64> = Vec::new();
    order.extend(&eids);
    order.extend(&fids);
    order.extend(&comp_ids);
    order.extend(&group_ids);
    order.extend(&image_ids);
    order.extend(&guide_ids);
    order.extend(&point_ids);
    order.extend(&polyline_ids);
    order.extend(&section_ids);
    order.extend(&text_ids);
    order.extend(&dim_ids);
    order.extend(&radial_ids);
    order.extend(&snap_ids);
    // An empty container lists no entities and holds no `0x138e`.
    if !order.is_empty() {
        kids.push(Record::id_list(0x138E, &order));
    }
    if let Some(active) = run.active_section {
        if let Some(&id) = t.id_of_pid.get(&active) {
            if section_ids.contains(&id) {
                kids.push(Record::narrow(0x1394, id));
            }
        }
    }
    // Relationships (§16.4): pairs of ids of this container's entities.
    for (e, &id) in m.edges.iter().zip(&eids) {
        local.entry(e.pid).or_insert(id);
    }
    for (f, &id) in m.faces.iter().zip(&fids) {
        local.entry(f.pid).or_insert(id);
    }
    for (s, &id) in run.sections.iter().zip(&section_ids) {
        local.entry(s.pid).or_insert(id);
    }
    let relationships: Vec<Record> = run
        .relationships
        .iter()
        .filter_map(|[a, b]| {
            let (a, b) = (local.get(a)?, local.get(b)?);
            Some(Record::kids(
                0x639C,
                vec![Record::narrow(0x639D, *a), Record::narrow(0x639E, *b)],
            ))
        })
        .collect();
    t.lost_relationships += run.relationships.len() - relationships.len();
    kids.push(Record::kids(
        0x139B,
        vec![Record::kids(
            0x639F,
            vec![if relationships.is_empty() {
                Record::empty(0x63A0)
            } else {
                Record::kids(0x63A0, relationships)
            }],
        )],
    ));
    // Bit 0 of the flags: the container is in the model — the root, or a
    // definition placed from it through placements — and holds entities.
    // SketchUp caches the bounds of exactly those (§16.4). The other bits
    // record history and are kept as the model carries them.
    let live = !order.is_empty() && (is_root || ri.is_some_and(|r| t.live.contains(&r)));
    kids.push(Record::u8(
        0x139F,
        (run.container_flags.unwrap_or(0) & !1) | live as u8,
    ));
    let bounds = match ri {
        Some(r) if live => run_bounds(model, r, &mut t.bounds, 0),
        _ => None,
    };
    if let Some([lo, hi]) = bounds {
        kids.push(Record::f64s_of(
            0x13A0,
            &[lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]],
        ));
    }
    // The result of the procedure that made this definition (§16.19).
    let procedure = model.features.procedures.iter().find(|p| {
        !is_root
            && run.def_index.is_some()
            && model.definitions[p.definition].map_index == run.def_index
    });
    if let Some(p) = procedure.filter(|p| !p.results.is_empty()) {
        let mut comps = Vec::new();
        let mut groups = Vec::new();
        let mut result_ids = Vec::new();
        let mut placements = Vec::new();
        for r in &p.results {
            let defref = defref_of(model, r.definition).unwrap_or(0);
            let (id, rec) = instance(
                r.pid,
                defref,
                &r.transform,
                r.material,
                r.hidden,
                r.layer,
                None,
                Some((
                    r.casts_shadows,
                    r.receives_shadows,
                    r.locked,
                    r.guid.as_str(),
                )),
                ids,
                t,
            );
            result_ids.push(id);
            placements.push((defref, r.transform));
            if r.is_group {
                groups.push(Record::kids(0x1D4C, vec![rec]));
            } else {
                comps.push(rec);
            }
        }
        let mut inner = vec![Record::kids(0x07D0, vec![Record::u8(0x07D3, SHADOW_FLAGS)])];
        if !comps.is_empty() {
            inner.push(Record::kids(0x138C, comps));
        }
        if !groups.is_empty() {
            inner.push(Record::kids(0x138D, groups));
        }
        inner.push(Record::empty(0x139E));
        inner.push(Record::id_list(0x138E, &result_ids));
        inner.push(Record::kids(
            0x139B,
            vec![Record::kids(0x639F, vec![Record::empty(0x63A0)])],
        ));
        inner.push(Record::u8(0x139F, 1));
        if let Some([lo, hi]) = add_placements(model, None, &placements, &mut t.bounds, 0) {
            inner.push(Record::f64s_of(
                0x13A0,
                &[lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]],
            ));
        }
        kids.push(Record::kids(
            0x13A4,
            vec![Record::kids(0x13EC, vec![Record::kids(0x1388, inner)])],
        ));
    }
    Record::kids(0x1388, kids)
}

/// The reference a placement of definition `index` names it by.
fn defref_of(model: &Model, index: usize) -> Option<u32> {
    model
        .definitions
        .get(index)?
        .map_index
        .map(|m| m as u32)
        .or_else(|| {
            model
                .definition_links
                .iter()
                .find(|l| l.1 == index)
                .map(|l| l.0)
        })
}

/// The run a placement's definition ref names.
fn run_of_ref(model: &Model, defref: u32) -> Option<usize> {
    let i = model
        .definition_links
        .binary_search_by_key(&defref, |l| l.0)
        .ok()?;
    let map_index = model.definitions[model.definition_links[i].1].map_index?;
    model
        .geometry
        .iter()
        .position(|r| r.def_index == Some(map_index))
}

/// The runs of the definitions placed from the root, directly or
/// through other placements (§16.4 `0x139f` bit 0).
fn live_runs(model: &Model) -> HashSet<usize> {
    let mut live = HashSet::new();
    let mut stack: Vec<usize> = model
        .geometry
        .iter()
        .position(|r| r.def_index.is_none())
        .into_iter()
        .collect();
    while let Some(ri) = stack.pop() {
        let run = &model.geometry[ri];
        // The placements in the wrapper of a procedure's result count.
        let results = model
            .features
            .procedures
            .iter()
            .filter(|p| {
                run.def_index.is_some()
                    && model
                        .definitions
                        .get(p.definition)
                        .and_then(|d| d.map_index)
                        == run.def_index
            })
            .flat_map(|p| p.results.iter())
            .filter_map(|r| defref_of(model, r.definition));
        let refs = run
            .placed
            .iter()
            .map(|p| p.defref)
            .chain(run.images.iter().map(|i| i.defref))
            .chain(results);
        for r in refs.filter_map(|d| run_of_ref(model, d)) {
            if live.insert(r) {
                stack.push(r);
            }
        }
    }
    live
}

/// A run's bounds with its placed children (§16.4 `0x13a0`): its own
/// (`bounds_in`) and, for each placed instance, group or image, the
/// eight corners of the placed definition's bounds under the placement
/// transform. Memoised by run; a cycle or a depth past 64 counts nothing.
fn run_bounds(
    model: &Model,
    ri: usize,
    cache: &mut HashMap<usize, Option<[[f64; 3]; 2]>>,
    depth: usize,
) -> Option<[[f64; 3]; 2]> {
    if let Some(b) = cache.get(&ri) {
        return *b;
    }
    if depth > 64 {
        return None;
    }
    cache.insert(ri, None);
    let run = &model.geometry[ri];
    let b = bounds_in(run, model, &|d| d == run.def_index);
    let placements: Vec<(u32, [f64; 13])> = run
        .placed
        .iter()
        .map(|p| (p.defref, p.transform))
        .chain(run.images.iter().map(|i| (i.defref, i.transform)))
        .collect();
    let b = add_placements(model, b, &placements, cache, depth);
    cache.insert(ri, b);
    b
}

/// `b` grown by the eight corners of each placed definition's bounds
/// under its placement transform.
fn add_placements(
    model: &Model,
    mut b: Option<[[f64; 3]; 2]>,
    placements: &[(u32, [f64; 13])],
    cache: &mut HashMap<usize, Option<[[f64; 3]; 2]>>,
    depth: usize,
) -> Option<[[f64; 3]; 2]> {
    for &(defref, t) in placements {
        let Some(child) =
            run_of_ref(model, defref).and_then(|c| run_bounds(model, c, cache, depth + 1))
        else {
            continue;
        };
        for corner in 0..8 {
            let p = [0, 1, 2].map(|k| child[(corner >> k) & 1][k]);
            let w = if t[12] == 0.0 { 1.0 } else { t[12] };
            let q = [0, 1, 2].map(|r| {
                (t[3 * r] * p[0] + t[3 * r + 1] * p[1] + t[3 * r + 2] * p[2] + t[9 + r]) / w
            });
            b = Some(match b {
                None => [q, q],
                Some([lo, hi]) => [
                    std::array::from_fn(|k| lo[k].min(q[k])),
                    std::array::from_fn(|k| hi[k].max(q[k])),
                ],
            });
        }
    }
    b
}

/// A container's own contribution to its cached bounds (§16.4
/// `0x13a0`), inches: its vertices, its 3D polylines, its guide points
/// (the point, not the tape anchor), its guide lines' anchor points, the
/// leader end of each radial dimension and of each pushpin text anchored
/// to one of its vertices, and both ends of each linear dimension's line,
/// as SketchUp counts them. Section planes and screen text do not count;
/// placed children are added by `run_bounds`. `None` when nothing
/// counts.
fn bounds_in(
    run: &GeometryRun,
    model: &Model,
    owns: &dyn Fn(Option<usize>) -> bool,
) -> Option<[[f64; 3]; 2]> {
    let m = &run.mesh;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut add = |p: [f64; 3]| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    let vin = |v: [f64; 3]| v.map(|x| x / M_PER_INCH);
    for v in &m.vertices {
        add(vin(*v));
    }
    for p in &run.polylines {
        for q in &p.points_m {
            add(q.map(inch));
        }
    }
    for g in &run.guide_points {
        add(g.point_m.map(inch_v));
    }
    for g in model.guides.iter().filter(|g| owns(g.def_index)) {
        add(g.point_m.map(inch));
    }
    // A radial dimension's leader ends on the radius at the leader's
    // angle in the curve's frame, `r − leader[1]` from the centre (§10.12).
    for d in model.radial_dimensions.iter().filter(|d| owns(d.def_index)) {
        let arc = d
            .edge
            .and_then(|pid| m.edges.iter().find(|e| e.pid == pid))
            .and_then(|e| e.curve)
            .and_then(|c| run.curves.get(c as usize))
            .and_then(|c| c.arc.as_ref());
        let Some(a) = arc else { continue };
        if a.radius_m <= 0.0 {
            continue;
        }
        let (c, s) = (d.leader[0].cos(), d.leader[0].sin());
        let dist = a.radius_m / M_PER_INCH - d.leader[1];
        let centre = vin(a.center_m);
        add(std::array::from_fn(|k| {
            centre[k] + dist * (c * a.x_axis_m[k] + s * a.y_axis_m[k]) / a.radius_m
        }));
    }
    // A linear dimension's line runs through its two anchors moved by the
    // offset: along normal × x axis when the x axis follows the measured
    // line (variant 1), along −x axis when the frame is turned a quarter
    // turn (variant 2) (§16.13).
    let anchor_at = |a: &crate::Anchor| -> Option<[f64; 3]> {
        if !a.path.is_empty() {
            return None;
        }
        let vertex = |pid: u32| {
            m.vertex_pids
                .iter()
                .position(|&v| v == pid)
                .and_then(|i| m.vertices.get(i))
                .map(|v| vin(*v))
        };
        match a.kind {
            2 => vertex(a.entity?),
            5 => {
                let e = m.edges.iter().find(|e| Some(e.pid) == a.entity)?;
                let (p, q) = (
                    vin(*m.vertices.get(e.v0 as usize)?),
                    vin(*m.vertices.get(e.v1 as usize)?),
                );
                let t = a.point_m[0];
                Some(std::array::from_fn(|k| p[k] + t * (q[k] - p[k])))
            }
            _ => Some(a.point_m.map(inch)),
        }
    };
    for d in model.dimensions.iter().filter(|d| owns(d.def_index)) {
        let (Some(p), Some(q)) = (anchor_at(&d.start), anchor_at(&d.end)) else {
            continue;
        };
        let (n, x) = (d.normal, d.x_axis);
        let dir = if d.variant == 2 {
            x.map(|v| -v)
        } else {
            [
                n[1] * x[2] - n[2] * x[1],
                n[2] * x[0] - n[0] * x[2],
                n[0] * x[1] - n[1] * x[0],
            ]
        };
        let off = inch(d.offset_m);
        add(std::array::from_fn(|k| p[k] + off * dir[k]));
        add(std::array::from_fn(|k| q[k] + off * dir[k]));
    }
    // A pushpin text's leader ends at its anchor plus the model-space
    // leader offset (§16.13).
    for x in model.texts.iter().filter(|x| owns(x.def_index)) {
        if x.leader != crate::Leader::Pushpin || x.anchor.kind != 2 || !x.anchor.path.is_empty() {
            continue;
        }
        let at = x
            .anchor
            .entity
            .and_then(|pid| m.vertex_pids.iter().position(|&v| v == pid))
            .and_then(|i| m.vertices.get(i));
        if let Some(v) = at {
            let v = vin(*v);
            add(std::array::from_fn(|k| v[k] + inch(x.leader_offset[k])));
        }
    }
    lo[0].is_finite().then_some([lo, hi])
}

/// Order a curve's member edges along the chain they form, in the edges'
/// own start-to-end direction (§16.4: a curve names its first and last
/// edge, and SketchUp drops a curve whose chain does not run that way):
/// the chain starts at the edge whose start vertex is a chain end, and
/// each next edge is the one starting where the previous ended. A closed
/// chain starts at the first member; edges that do not connect keep
/// their order at the end.
fn chain_edges(members: &[usize], edges: &[crate::MeshEdge]) -> Vec<usize> {
    if members.len() < 2 {
        return members.to_vec();
    }
    let mut degree: HashMap<u32, usize> = HashMap::new();
    for &i in members {
        *degree.entry(edges[i].v0).or_default() += 1;
        *degree.entry(edges[i].v1).or_default() += 1;
    }
    let start = members
        .iter()
        .copied()
        .find(|&i| degree[&edges[i].v0] == 1)
        .or_else(|| members.iter().copied().find(|&i| degree[&edges[i].v1] == 1))
        .unwrap_or(members[0]);
    let mut used = vec![false; edges.len()];
    let mut out = Vec::with_capacity(members.len());
    let mut cur = start;
    let mut at = if degree[&edges[start].v0] == 1 || degree[&edges[start].v1] != 1 {
        edges[start].v0
    } else {
        edges[start].v1
    };
    loop {
        used[cur] = true;
        out.push(cur);
        let e = &edges[cur];
        at = if e.v0 == at { e.v1 } else { e.v0 };
        // Prefer the edge that starts here; accept one that ends here.
        let next = members
            .iter()
            .copied()
            .find(|&i| !used[i] && edges[i].v0 == at)
            .or_else(|| {
                members
                    .iter()
                    .copied()
                    .find(|&i| !used[i] && edges[i].v1 == at)
            });
        match next {
            Some(n) => cur = n,
            None => break,
        }
    }
    for &i in members {
        if !used[i] {
            out.push(i);
        }
    }
    out
}

/// A face (§16.4): plane, loops rebuilt from the rings, materials, and
/// the texture placement in its attribute set.
fn face_record(
    f: &MeshFace,
    id: u64,
    m: &crate::Mesh,
    edge_of_ends: &HashMap<(u32, u32), usize>,
    eids: &[u64],
    t: &mut Tables,
    owner: Option<crate::Owner>,
) -> Record {
    let mut flags = shadow_flags(f.casts_shadows, f.receives_shadows);
    if f.hidden {
        flags |= 0x01;
    }
    let texture = f.texture.as_ref().map(|tx| {
        let side = |k: &[f64; 9], extra: &[f64; 3], pins: &[[f64; 4]], fl: u32| {
            let pins: Vec<Record> = pins
                .iter()
                .map(|p| {
                    Record::kids(
                        0x2718,
                        vec![
                            Record::f64s_of(0x2719, &p[..2]),
                            Record::f64s_of(0x271A, &p[2..]),
                        ],
                    )
                })
                .collect();
            Record::kids(
                0x2713,
                vec![
                    Record::u32(0x2714, fl),
                    Record::f64s_of(0x2715, k),
                    Record::f64s_of(0x2716, extra),
                    if pins.is_empty() {
                        Record::empty(0x2717)
                    } else {
                        Record::kids(0x2717, pins)
                    },
                ],
            )
        };
        Record::kids(
            0x2710,
            vec![
                Record::empty(0x36B0),
                Record::kids(
                    0x2711,
                    vec![side(
                        &tx.front,
                        &tx.front_extra,
                        &tx.front_pins,
                        tx.flags[0],
                    )],
                ),
                Record::kids(
                    0x2712,
                    vec![side(&tx.back, &tx.back_extra, &tx.back_pins, tx.flags[1])],
                ),
            ],
        )
    });
    // The texture placement sits in the attribute set with the
    // dictionaries.
    let attrs = match owner {
        Some(o) => t.attributes(o, texture),
        None => texture.map(|tx| {
            Record::kids(
                0x05DD,
                vec![Record::kids(0x36B1, vec![Record::kids(0x36B2, vec![tx])])],
            )
        }),
    };
    let mut kids = vec![drawing(
        id,
        f.front_material
            .and_then(|s| t.material_of_slot.get(&s).copied()),
        t.layer_of_slot.get(&f.layer).copied().flatten(),
        flags,
        attrs,
    )];
    if let Some(b) = f
        .back_material
        .and_then(|s| t.material_of_slot.get(&s).copied())
    {
        kids.push(Record::narrow(0x0DAF, b));
    }
    let n = f.normal;
    let d = plane_offset(f, m, inch_v);
    kids.push(Record::f64s_of(0x0DAD, &[n[0], n[1], n[2], d]));
    // Use `k` names the edge that arrives at ring[k] from ring[k-1], and
    // is forward when that edge's start vertex is ring[k] — the storage
    // rule the reader inverts (§16.4: a use contributes its start vertex
    // when forward, else its end vertex).
    let loop_of = |ring: &[u32]| -> Record {
        let uses: Vec<Record> = (0..ring.len())
            .filter_map(|k| {
                let (a, b) = (ring[(k + ring.len() - 1) % ring.len()], ring[k]);
                let ei = *edge_of_ends.get(&(a.min(b), a.max(b)))?;
                let forward = m.edges[ei].v0 == b;
                Some(Record::kids(
                    0x0FA0,
                    vec![
                        Record::narrow(0x0FA1, eids[ei]),
                        Record::u8(0x0FA2, forward as u8),
                    ],
                ))
            })
            .collect();
        Record::kids(0x1194, vec![Record::kids(0x1195, uses)])
    };
    let mut loops = vec![loop_of(&f.outer)];
    for h in &f.holes {
        // The reader reverses inner loops into its hole convention;
        // storage order is the opposite.
        let mut r = h.clone();
        r.reverse();
        loops.push(loop_of(&r));
    }
    kids.push(Record::kids(0x0DAE, loops));
    Record::kids(0x0DAC, kids)
}

/// The offset D of a face's plane `[A, B, C, D]`, inches: the one that
/// puts the plane midway between the face's vertices along its normal.
/// On a flat face that is the offset of any vertex; on a face that is
/// flat only within SketchUp's tolerance it halves the distance of the
/// farthest vertex, where the offset of one vertex can put the others
/// past the tolerance and have SketchUp split the face.
pub(crate) fn plane_offset(f: &MeshFace, m: &crate::Mesh, inches: impl Fn(f64) -> f64) -> f64 {
    let n = f.normal;
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &i in f.outer.iter().chain(f.holes.iter().flatten()) {
        let Some(p) = m.vertices.get(i as usize) else {
            continue;
        };
        let h = n[0] * inches(p[0]) + n[1] * inches(p[1]) + n[2] * inches(p[2]);
        lo = lo.min(h);
        hi = hi.max(h);
    }
    if lo.is_finite() && hi.is_finite() {
        -(lo + hi) / 2.0
    } else {
        0.0
    }
}

/// An anchor (§16.13 `0x5208`).
/// The persistent ids of the instances leading from the model to the
/// definition holding the entity `pid` (empty when it is a model entity
/// or cannot be found); the first placement wins when a definition is
/// placed more than once.
pub(crate) fn instance_path(model: &Model, pid: u32) -> Vec<u32> {
    let holds = |r: &GeometryRun| {
        r.mesh.vertex_pids.contains(&pid)
            || r.mesh.edges.iter().any(|e| e.pid == pid)
            || r.mesh.faces.iter().any(|f| f.pid == pid)
            || r.placed.iter().any(|p| p.pid == pid)
    };
    // A model entity needs no path; the root is checked first because a
    // 2017 file can reuse its persistent id inside a definition.
    if model
        .geometry
        .iter()
        .any(|r| r.def_index.is_none() && holds(r))
    {
        return Vec::new();
    }
    let Some(target) = model
        .geometry
        .iter()
        .find(|r| r.def_index.is_some() && holds(r))
        .and_then(|r| r.def_index)
    else {
        return Vec::new();
    };
    let run_of_def = |d: usize| model.geometry.iter().find(|r| r.def_index == Some(d));
    let def_of_ref = |defref: u32| -> Option<usize> {
        model
            .definition_links
            .binary_search_by_key(&defref, |l| l.0)
            .ok()
            .and_then(|i| model.definitions[model.definition_links[i].1].map_index)
    };
    fn dfs<'m>(
        run: &'m GeometryRun,
        target: usize,
        run_of_def: &dyn Fn(usize) -> Option<&'m GeometryRun>,
        def_of_ref: &dyn Fn(u32) -> Option<usize>,
        path: &mut Vec<u32>,
        depth: usize,
    ) -> bool {
        if depth > 64 {
            return false;
        }
        for p in &run.placed {
            let Some(d) = def_of_ref(p.defref) else {
                continue;
            };
            path.push(p.pid);
            if d == target {
                return true;
            }
            if let Some(child) = run_of_def(d) {
                if dfs(child, target, run_of_def, def_of_ref, path, depth + 1) {
                    return true;
                }
            }
            path.pop();
        }
        false
    }
    let mut path = Vec::new();
    if let Some(root) = model.geometry.iter().find(|r| r.def_index.is_none()) {
        if dfs(root, target, &run_of_def, &def_of_ref, &mut path, 0) {
            return path;
        }
    }
    Vec::new()
}

/// Whether an anchor that names two entities (kind 9, where two edges
/// meet) can be written whole: SketchUp does not open a file holding one
/// that lacks its second.
fn crossing_held(a: &crate::Anchor, t: &Tables) -> bool {
    a.kind != 9 || a.second.is_some_and(|p| t.id_of_pid.contains_key(&p))
}

fn anchor_record(tag: u16, a: &crate::Anchor, t: &Tables, model: &Model) -> Record {
    let point = if a.kind == 5 {
        vec![a.point_m[0], inch(a.point_m[1]), inch(a.point_m[2])]
    } else {
        v3_in(a.point_m)
    };
    // An entity and the instances leading to it: the model's path, else
    // derived from the placement tree (an entity inside a definition
    // needs the instances leading to it).
    let target = |entity: Option<u32>, path: &[u32], vertex: bool| -> Record {
        let pids: Vec<u32> = if !path.is_empty() {
            path.to_vec()
        } else {
            entity.map(|p| instance_path(model, p)).unwrap_or_default()
        };
        let path: Vec<u64> = pids
            .iter()
            .filter_map(|p| t.instance_of_pid.get(p).copied())
            .collect();
        let resolved = entity.and_then(|p| {
            let general = t.id_of_pid.get(&p).copied();
            if vertex {
                t.vertex_of_pid.get(&p).copied().or(general)
            } else {
                general.or_else(|| t.vertex_of_pid.get(&p).copied())
            }
        });
        let mut kids = Vec::new();
        if let Some(e) = resolved {
            kids.push(Record::narrow(0x53FD, e));
        }
        kids.push(Record::id_list(0x53FE, &path));
        Record::kids(0x53FC, kids)
    };
    let first = target(a.entity, &a.path, a.kind == 2);
    let second = match a.second {
        Some(_) => target(a.second, &a.second_path, false),
        None => Record::kids(0x53FC, vec![Record::empty(0x53FE)]),
    };
    Record::kids(
        tag,
        vec![Record::kids(
            0x5208,
            vec![
                Record::u32(0x5209, a.kind),
                Record::f64s_of(0x520A, &point),
                Record::kids(0x520B, vec![first]),
                Record::kids(0x520C, vec![second]),
            ],
        )],
    )
}

fn font_id(t: &Tables, font: Option<usize>) -> u64 {
    font.and_then(|i| t.font_ids.get(i).copied())
        .unwrap_or(t.default_font)
}

fn text_record(x: &crate::Text, id: u64, index: usize, t: &mut Tables, model: &Model) -> Record {
    let leader = match x.leader {
        crate::Leader::None => 0,
        crate::Leader::ViewBased => 1,
        crate::Leader::Pushpin => 2,
    };
    let offset = if x.leader == crate::Leader::Pushpin {
        v3_in(x.leader_offset)
    } else {
        x.leader_offset.to_vec()
    };
    Record::kids(
        0x55F0,
        vec![
            drawn(
                id,
                x.layer,
                x.material,
                x.hidden,
                (x.casts_shadows, x.receives_shadows),
                Some(crate::Owner::Text(index)),
                t,
            ),
            Record::string(0x55F1, &x.content),
            Record::string(0x55FF, &x.placeholder),
            Record::f64(0x55F2, x.screen_position[0]),
            Record::f64(0x55F3, x.screen_position[1]),
            anchor_record(0x55F4, &x.anchor, t, model),
            Record::f64s_of(0x55F5, &offset),
            Record::u8(0x55F6, 1),
            Record::f64s_of(0x55F7, &x.view_direction),
            Record::u8(0x55F8, 0),
            Record::narrow(0x55F9, font_id(t, x.font)),
            Record::u32(0x55FA, x.arrow),
            Record::u32(0x55FB, 1),
            Record::u8(0x55FC, !x.leader_hidden as u8),
            Record::u32(0x55FD, leader),
            Record::u32(0x55FE, x.leader_hidden as u32),
        ],
    )
}

/// A `0x5dc0` radial dimension (§16.13): the dimension base, then the
/// measured edge, the two leader numbers and the diameter flag.
fn radial_dimension_record(
    d: &crate::RadialDimension,
    id: u64,
    index: usize,
    edge: u64,
    t: &mut Tables,
) -> Record {
    Record::kids(
        0x5DC0,
        vec![
            Record::kids(
                0x59D8,
                vec![
                    drawn(
                        id,
                        d.layer,
                        d.material,
                        d.hidden,
                        (d.casts_shadows, d.receives_shadows),
                        Some(crate::Owner::RadialDimension(index)),
                        t,
                    ),
                    Record::string(0x59D9, &d.text_override),
                    Record::narrow(0x59DA, font_id(t, d.font)),
                    Record::u8(0x59DB, d.aligned as u8),
                    Record::u32(0x59DC, d.arrow),
                ],
            ),
            Record::narrow(0x5DC1, edge),
            Record::f64(0x5DC3, d.leader[0]),
            Record::f64(0x5DC4, d.leader[1]),
            Record::u8(0x5DC5, d.diameter as u8),
        ],
    )
}

fn dimension_record(
    d: &crate::Dimension,
    id: u64,
    index: usize,
    t: &mut Tables,
    model: &Model,
) -> Record {
    Record::kids(
        0x5BCC,
        vec![
            Record::kids(
                0x59D8,
                vec![
                    drawn(
                        id,
                        d.layer,
                        d.material,
                        d.hidden,
                        (d.casts_shadows, d.receives_shadows),
                        Some(crate::Owner::Dimension(index)),
                        t,
                    ),
                    Record::string(0x59D9, &d.text_override),
                    Record::narrow(0x59DA, font_id(t, d.font)),
                    Record::u8(0x59DB, d.aligned as u8),
                    Record::u32(0x59DC, d.arrow),
                ],
            ),
            anchor_record(0x5BCD, &d.start, t, model),
            anchor_record(0x5BCE, &d.end, t, model),
            Record::f64s_of(0x5BCF, &d.normal),
            Record::f64s_of(0x5BD0, &d.x_axis),
            Record::u32(0x5BD1, d.variant),
            Record::f64(0x5BD3, d.undecoded),
            Record::f64(0x5BD2, inch(d.offset_m)),
            Record::u32(0x5BD4, d.text_position),
        ],
    )
}

/// Build a 2026 document from `model`.
/// The units as written (§16.12): engineering length format (2) exists
/// only in 2017 files, and SketchUp's conversion writes decimal feet for
/// it, so the option record and `meta.dat` both carry that.
fn written_units(model: &Model) -> crate::settings::Units {
    let mut units = model.units.clone();
    if units.length_format == 2 {
        units.length_format = 0;
        units.length_unit = 1;
    }
    units
}

/// Refuse a model whose indices do not fit together: every edge and ring
/// index must name a vertex of its run, every ring must be closed by
/// edges of its run (each consecutive pair, including the last to the
/// first, must be an edge — SketchUp reads a face as loops of edge uses),
/// every curve index a curve, every placement a linked definition, and
/// every definition index a definition. A `Model` the readers produce
/// passes; one assembled by another program is checked here rather than
/// written with dangling references or indexed out of bounds.
///
/// A model of a stream-container file read by the run-based path
/// (`Diagnostic::ContinuousFallback`) is refused: there that path is
/// the fallback, which finds geometry and leaves out what it cannot
/// place, and what it reads of the document's settings is not the
/// file's, so a file written from it would not be the model. A file of
/// releases 3 to 8 or 2013 to 2016 has no other reader and is written as
/// read.
pub(crate) fn validate(model: &Model) -> Result<(), Error> {
    crate::build26x::validate(model)?;
    let bad = |msg: String| Err(Error(format!("model cannot be written: {msg}")));
    let release: u32 = model
        .version
        .trim_start_matches('{')
        .split('.')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    for d in model.diagnostics.iter().filter(|_| release >= 17) {
        if let crate::Diagnostic::ContinuousFallback { stage, at, .. } = d {
            return bad(format!(
                "the file it was read from was read in part only (the walk of the whole \
                 file stopped in {stage} at {at:#x})"
            ));
        }
    }
    for (ri, r) in model.geometry.iter().enumerate() {
        let m = &r.mesh;
        let nv = m.vertices.len();
        let mut ends: HashSet<(u32, u32)> = HashSet::with_capacity(m.edges.len());
        for (ei, e) in m.edges.iter().enumerate() {
            if e.v0 as usize >= nv || e.v1 as usize >= nv {
                return bad(format!(
                    "run {ri}: edge {ei} (id {}) refers to vertex {}/{} of {nv}",
                    e.pid, e.v0, e.v1
                ));
            }
            if let Some(c) = e.curve {
                if c as usize >= r.curves.len() {
                    return bad(format!(
                        "run {ri}: edge {ei} (id {}) refers to curve {c} of {}",
                        e.pid,
                        r.curves.len()
                    ));
                }
            }
            ends.insert((e.v0.min(e.v1), e.v0.max(e.v1)));
        }
        for (fi, f) in m.faces.iter().enumerate() {
            let length = f.normal.iter().map(|x| x * x).sum::<f64>().sqrt();
            if !length.is_finite() || (length - 1.0).abs() > 1e-6 {
                return bad(format!(
                    "run {ri}: face {fi} (id {}) has the normal {:?}, which is not a unit vector",
                    f.pid, f.normal
                ));
            }
            for (which, ring) in std::iter::once(&f.outer).chain(&f.holes).enumerate() {
                let what = if which == 0 {
                    "outer loop".to_string()
                } else {
                    format!("hole {which}")
                };
                if ring.len() < 3 {
                    return bad(format!(
                        "run {ri}: face {fi} (id {}) {what} has {} vertices",
                        f.pid,
                        ring.len()
                    ));
                }
                for &i in ring {
                    if i as usize >= nv {
                        return bad(format!(
                            "run {ri}: face {fi} (id {}) {what} refers to vertex {i} of {nv}",
                            f.pid
                        ));
                    }
                }
                for k in 0..ring.len() {
                    let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                    if a == b || !ends.contains(&(a.min(b), a.max(b))) {
                        return bad(format!(
                            "run {ri}: face {fi} (id {}) {what} has no edge between vertices {a} and {b}",
                            f.pid
                        ));
                    }
                }
            }
        }
        for p in &r.placed {
            if model
                .definition_links
                .binary_search_by_key(&p.defref, |l| l.0)
                .is_err()
            {
                return bad(format!(
                    "run {ri}: instance {} places definition ref {} which no definition link resolves",
                    p.pid, p.defref
                ));
            }
        }
    }
    let nd = model.definitions.len();
    for l in &model.definition_links {
        if l.1 >= nd {
            return bad(format!(
                "definition link {} refers to definition {} of {nd}",
                l.0, l.1
            ));
        }
    }
    // An annotation names its owner by the definition's map index; one
    // that names no definition would be written nowhere.
    let owners = model
        .texts
        .iter()
        .map(|x| ("text", x.pid, x.def_index))
        .chain(
            model
                .dimensions
                .iter()
                .map(|d| ("dimension", d.pid, d.def_index)),
        )
        .chain(model.guides.iter().map(|g| ("guide", g.pid, g.def_index)));
    for (kind, pid, def_index) in owners {
        if let Some(d) = def_index {
            if !model.definitions.iter().any(|x| x.map_index == Some(d)) {
                return bad(format!(
                    "{kind} {pid} belongs to definition map index {d}, which no definition has"
                ));
            }
        }
    }
    Ok(())
}

/// The revision `target` names for `model` (§16.27), or the error for
/// one that is not written.
fn target_revision(model: &Model, target: crate::ArchiveTarget) -> Result<crate::Revision, Error> {
    use crate::{ArchiveTarget, Revision};
    match target {
        // The lowest revision that holds the content, and no lower than
        // the one the model was read with (or, for a model built in code,
        // the one its version label names): a re-save keeps the revision
        // of its file unless an edit added a feature that needs more.
        ArchiveTarget::ByContent => {
            let own = model
                .revision
                .or_else(|| model.version.starts_with("{27.").then_some(Revision::R18));
            Ok(own
                .unwrap_or(Revision::R17)
                .max(model.min_revision())
                .clamp(Revision::R17, Revision::NEWEST))
        }
        ArchiveTarget::Revision(r) if Revision::WRITTEN.contains(&r) => Ok(r),
        ArchiveTarget::Revision(r) => Err(Error(format!(
            "revision {r} is not written: a 2026-container file is written at revision {} or {}",
            Revision::R17,
            Revision::R18
        ))),
    }
}

pub(crate) fn from_model(model: &Model, target: crate::ArchiveTarget) -> Result<Document, Error> {
    validate(model)?;
    let revision = target_revision(model, target)?;
    if revision == crate::Revision::R17 {
        if let Some((kept, notes)) = crate::build26x::strip_to_r17(model) {
            let mut doc = build(&kept, revision)?;
            doc.notes.splice(0..0, notes);
            return Ok(doc);
        }
    }
    build(model, revision)
}

/// The document of `model` at `revision`, which holds everything the
/// model has: the version label and the meta fields are the ones of the
/// writer line of that revision. `model` is validated.
fn build(model: &Model, revision: crate::Revision) -> Result<Document, Error> {
    let version = if revision.desktop() {
        "{27.0.267}".to_string()
    } else if model.version.starts_with("{26.") {
        model.version.clone()
    } else {
        "{26.2.0}".to_string()
    };
    let mut doc = Document::blank(&version, revision);
    let mut seed: u64 = u32::from_le_bytes(crate::env::random_bytes()) as u64 | 1;

    // Ids start past every persistent id the model carries.
    let max_pid = model
        .geometry
        .iter()
        .flat_map(|r| {
            r.mesh
                .vertex_pids
                .iter()
                .copied()
                .chain(r.mesh.edges.iter().map(|e| e.pid))
                .chain(r.mesh.faces.iter().map(|f| f.pid))
                .chain(r.placed.iter().map(|p| p.pid))
                .chain(r.sections.iter().map(|s| s.pid))
                .chain(r.curves.iter().map(|c| c.pid))
                .chain(r.guide_points.iter().map(|g| g.pid))
                .chain(r.polylines.iter().map(|g| g.pid))
                .chain(r.images.iter().map(|i| i.pid))
        })
        .chain(model.texts.iter().map(|x| x.pid))
        .chain(model.dimensions.iter().map(|d| d.pid))
        .chain(model.radial_dimensions.iter().map(|d| d.pid))
        .chain(model.guides.iter().map(|g| g.pid))
        .chain(model.features.snaps.iter().map(|s| s.pid))
        .chain(
            model
                .features
                .procedures
                .iter()
                .flat_map(|p| p.results.iter().map(|r| r.pid)),
        )
        .max()
        .unwrap_or(0);
    let mut ids = Ids {
        next: max_pid as u64 + 1,
        used: HashSet::new(),
    };
    let mut tables = Tables {
        material_of_slot: HashMap::new(),
        layer_of_slot: HashMap::new(),
        container_of_defref: HashMap::new(),
        container_of_def: Vec::new(),
        font_ids: Vec::new(),
        default_font: 0,
        instance_names: model
            .instances
            .iter()
            .filter_map(|i| i.name.clone().map(|n| (i.pid, n)))
            .collect(),
        id_of_pid: HashMap::new(),
        vertex_of_pid: HashMap::new(),
        instance_of_pid: HashMap::new(),
        dicts: {
            let mut by_owner: HashMap<crate::Owner, Vec<crate::Dictionary>> = HashMap::new();
            for a in &model.dictionaries {
                by_owner
                    .entry(a.owner)
                    .or_default()
                    .extend(a.dictionaries.iter().cloned());
            }
            by_owner
        },
        lost_values: 0,
        lost_relationships: 0,
        live: live_runs(model),
        bounds: HashMap::new(),
        lost_radial_dimensions: 0,
        lost_edgeless_radials: 0,
        lost_crossings: 0,
        hatch_ids: Vec::new(),
        instance_hatch: HashMap::new(),
    };
    // ---- hatch patterns: their ids come first, as SketchUp allocates them
    tables.hatch_ids = model
        .features
        .hatch_patterns
        .iter()
        .map(|_| ids.fresh())
        .collect();
    {
        // An instance is named by its place in `Model::instances`, which
        // lists the runs' placements in run order.
        let mut first = 0usize;
        for (r, run) in model.geometry.iter().enumerate() {
            for &(i, h) in &model.features.instance_hatch {
                if (first..first + run.placed.len()).contains(&i) {
                    tables
                        .instance_hatch
                        .insert(crate::Owner::Instance(r, i - first), h);
                }
            }
            first += run.placed.len();
        }
    }

    // ---- materials: records, XML, image entries
    let mut taken = HashSet::new();
    let mut material_ids = Vec::with_capacity(model.materials.len());
    let mut material_records = Vec::with_capacity(model.materials.len());
    let mut image_owner: HashMap<Vec<u8>, String> = HashMap::new();
    let image_materials = crate::build17::image_materials(model);
    for mat in &model.materials {
        let id = ids.fresh();
        material_ids.push(id);
        let name = match mat {
            Material::Solid { name, .. } | Material::Textured { name, .. } => name.as_str(),
        };
        let folder = folder_name(name, &mut taken);
        let mut rec = vec![
            entity_with(
                id,
                tables.attributes(crate::Owner::Material(material_ids.len() - 1), None),
            ),
            Record::string(0x32CC, &folder),
            // An image's own material is marked as such (§16.6).
            Record::u8(
                0x32CA,
                image_materials.contains(&material_ids.len().wrapping_sub(1)) as u8,
            ),
            Record::empty(0x32CB),
        ];
        let pbr = model
            .features
            .material_pbr
            .iter()
            .find(|(m, _)| *m == material_ids.len() - 1)
            .map(|(_, p)| p);
        if let Some(p) = pbr {
            crate::build26x::set_pbr_images(p, &folder, &mut doc);
        }
        match mat {
            Material::Solid {
                rgba,
                opacity,
                colorize_type,
                ..
            } => {
                doc.set_entry(
                    &format!("materials/{folder}/material.xml"),
                    xml26::solid_material_xml(
                        name,
                        [rgba[0], rgba[1], rgba[2]],
                        *opacity,
                        *colorize_type,
                        pbr,
                    )
                    .into_bytes(),
                );
            }
            Material::Textured {
                texture,
                applied_size_in,
                jpeg_quality,
                image_bytes,
                avg_rgba,
                colorized,
                texture_avg_rgba,
                colorize_type,
                opacity,
                ..
            } => {
                let ext = image_bytes
                    .as_deref()
                    .map(xml26::image_ext)
                    .unwrap_or("jpg");
                // The texture name can be the full path the image was
                // loaded from (stream-container files keep it); the archive entry is
                // its basename, as SketchUp's conversion writes it.
                let texture_name = texture.clone().filter(|s| !s.is_empty());
                let file_name = texture_name
                    .as_deref()
                    .and_then(|t| t.rsplit(['/', '\\']).next())
                    .filter(|b| !b.is_empty())
                    .map(|b| b.replace([':', '*', '?', '"', '<', '>', '|'], "_"))
                    .unwrap_or_else(|| format!("{folder}.{ext}"));
                let path = match image_bytes {
                    Some(b) => match image_owner.get(b) {
                        Some(p) => p.clone(),
                        None => {
                            let p = format!("materials/{folder}/{file_name}");
                            doc.set_entry(&p, b.clone());
                            image_owner.insert(b.clone(), p.clone());
                            p
                        }
                    },
                    None => format!("materials/{folder}/{file_name}"),
                };
                let avg = avg_rgba.unwrap_or([128, 128, 128, 255]);
                doc.set_entry(
                    &format!("materials/{folder}/material.xml"),
                    xml26::textured_material_xml(
                        name,
                        &folder,
                        texture_name.as_deref().unwrap_or(&file_name),
                        &file_name,
                        &path,
                        [avg[0], avg[1], avg[2]],
                        // The reader takes the average for the colour
                        // of a texture that is not colourised.
                        if *colorized {
                            texture_avg_rgba.unwrap_or(avg)
                        } else {
                            avg
                        },
                        *colorized,
                        applied_size_in.unwrap_or((1.0, 1.0)),
                        *opacity,
                        *colorize_type,
                        pbr,
                    )
                    .into_bytes(),
                );
                if let Some(q) = jpeg_quality {
                    rec.push(Record::u32(0x32CD, *q));
                } else if ext == "jpg" {
                    rec.push(Record::u32(0x32CD, 70));
                }
            }
        }
        // The hatch pattern the material uses (`0x32cf`), last.
        if let Some(&(_, h)) = model
            .features
            .material_hatch
            .iter()
            .find(|(m, _)| *m == material_ids.len() - 1)
        {
            rec.push(Record::narrow(0x32CF, tables.hatch_ids[h]));
        }
        material_records.push(Record::kids(0x32C8, rec));
    }
    for (slot, index) in &model.material_links {
        if let Some(&id) = material_ids.get(*index) {
            tables.material_of_slot.insert(*slot, id);
        }
    }

    // ---- layers
    let mut layer_ids = Vec::with_capacity(model.layers.len());
    let mut layer_records = Vec::with_capacity(model.layers.len());
    let mut layer_names_taken = HashSet::new();
    let mut layers: Vec<crate::Layer> = model.layers.clone();
    if layers.is_empty() {
        layers.push(crate::Layer {
            name: "Layer0".into(),
            visible: true,
            rgba: [255, 84, 84, 255],
        });
    }
    for l in &layers {
        let id = ids.fresh();
        let colour_id = ids.fresh();
        layer_ids.push(id);
        let folder = folder_name(&format!("Layer_{}", l.name), &mut layer_names_taken);
        let folder = folder_name(&folder, &mut taken);
        doc.set_entry(
            &format!("materials/{folder}/material.xml"),
            xml26::layer_material_xml(
                &format!("Layer_{}", l.name),
                [l.rgba[0], l.rgba[1], l.rgba[2]],
            )
            .into_bytes(),
        );
        let layer_attrs = tables.attributes(crate::Owner::Layer(layer_ids.len() - 1), None);
        layer_records.push(Record::kids(
            0x3C8C,
            vec![
                entity_with(id, layer_attrs),
                Record::string(0x3C8D, &l.name),
                Record::u8(0x3C8E, (!l.visible) as u8),
                Record::kids(
                    0x3C8F,
                    vec![Record::kids(
                        0x32C8,
                        vec![
                            entity(colour_id),
                            Record::string(0x32CC, &folder),
                            Record::u8(0x32CA, 1),
                            Record::empty(0x32CB),
                        ],
                    )],
                ),
                Record::u32(0x3C90, 0),
            ],
        ));
    }
    tables.layer_of_slot.insert(0, None);
    for (slot, index) in &model.layer_links {
        tables.layer_of_slot.insert(
            *slot,
            if *index == 0 {
                None
            } else {
                layer_ids.get(*index).copied()
            },
        );
    }

    // ---- fonts
    let mut fonts = model.fonts.clone();
    if fonts.is_empty() {
        fonts.push(crate::Font {
            family: "Tahoma".into(),
            bold: false,
            italic: false,
            size_pt: 12,
            model_height: false,
            height_m: crate::Font::DEFAULT_HEIGHT_M,
        });
    }
    let font_records: Vec<Record> = fonts
        .iter()
        .map(|f| {
            let id = ids.fresh();
            tables.font_ids.push(id);
            Record::kids(
                0x5014,
                vec![
                    entity(id),
                    Record::string(0x5015, &f.family),
                    Record::u8(0x5016, f.bold as u8),
                    Record::u8(0x5017, f.italic as u8),
                    Record::u32(0x5018, f.size_pt),
                    Record::u8(0x5019, f.model_height as u8),
                    Record::f64(0x501A, inch(f.height_m)),
                ],
            )
        })
        .collect();
    tables.default_font = tables.font_ids[0];

    // ---- definitions: container ids first, so instances anywhere link
    for d in &model.definitions {
        let cid = ids.fresh();
        tables.container_of_def.push(cid);
        if let Some(mi) = d.map_index {
            tables.container_of_defref.insert(mi as u32, cid);
        }
    }
    for (defref, index) in &model.definition_links {
        if let Some(&cid) = tables.container_of_def.get(*index) {
            tables.container_of_defref.entry(*defref).or_insert(cid);
        }
    }

    // ---- styles and scenes get ids now (scenes refer to styles, styles
    // list their scenes).
    let mut styles: Vec<crate::Style> = model.styles.clone();
    if styles.is_empty() {
        styles.push(crate::Style {
            name: "[Default Style]".into(),
            description: String::new(),
            guid: String::new(),
            settings: model
                .rendering
                .clone()
                .unwrap_or_else(RenderingOptions::sketchup_default),
            watermarks: model.watermarks.clone(),
            strokes: None,
            items: Vec::new(),
        });
    }
    // A style keeps one identifier: the current settings name the active
    // style by it.
    for s in styles.iter_mut() {
        if guid_bytes(&s.guid).is_none() {
            s.guid = random_guid(&mut seed)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
        }
    }
    let style_ids: Vec<u64> = styles.iter().map(|_| ids.fresh()).collect();
    let active_style = model
        .active_style
        .filter(|&i| i < styles.len())
        .unwrap_or(0);
    let scene_ids: Vec<u64> = model.scenes.iter().map(|_| ids.fresh()).collect();
    let style_index_of_name: HashMap<&str, usize> = styles
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), i))
        .collect();
    let scene_style = |s: &Scene| -> usize {
        s.style
            .as_deref()
            .and_then(|n| style_index_of_name.get(n).copied())
            .unwrap_or(active_style)
    };

    // ---- entity containers
    // The model's own dictionaries: `ModelProperties` under the model
    // record, the rest with the root entity container (§16.8), as a
    // stream-container file keeps them apart. A model without them takes a
    // `GeoReference` built from its settings and the properties of a
    // new document.
    let own = tables
        .dicts
        .remove(&crate::Owner::Model)
        .unwrap_or_default();
    let (mut properties, mut rest): (Vec<_>, Vec<_>) =
        own.into_iter().partition(|d| d.name == "ModelProperties");
    // Every 2026 file's properties say whether the model is live; a 2017
    // model's lack the entry, which SketchUp adds as false, in key order.
    for d in &mut properties {
        if !d.entries.iter().any(|(k, _)| k == "IsLive") {
            let at = d
                .entries
                .iter()
                .position(|(k, _)| k.as_str() > "IsLive")
                .unwrap_or(d.entries.len());
            d.entries
                .insert(at, ("IsLive".into(), crate::Value::Bool(false)));
        }
    }
    // A model built without dictionaries takes a `GeoReference` from its
    // geo-location settings; one read from a file holds its own.
    if model.dictionaries.is_empty() {
        rest.extend(geo_reference(model));
    }
    let geo_attrs = if rest.is_empty() {
        None
    } else {
        tables.dicts.insert(crate::Owner::Model, rest);
        tables.attributes(crate::Owner::Model, None)
    };
    let mut thumbnail_names_taken = HashSet::new();
    let mut previews_left_out = 0usize;
    let model_own: Vec<Record> = properties
        .iter()
        .map(|d| dictionary_record(d, &mut tables.lost_values))
        .collect();
    let mut def_records = Vec::with_capacity(model.definitions.len());
    // Each definition after the definitions it places, as SketchUp writes
    // them: SketchUp's web app drops a definition that places one written
    // after it.
    for i in crate::build17::definition_order(model)? {
        let d = &model.definitions[i];
        let run = model
            .geometry
            .iter()
            .find(|r| r.def_index.is_some() && r.def_index == d.map_index);
        let empty = GeometryRun {
            start: 0,
            end: 0,
            top_level: 0,
            frame: None,
            def_index: d.map_index,
            topology: Default::default(),
            resolved: (0, 0),
            mesh: Default::default(),
            placed: vec![],
            curve_members: vec![],
            sections: vec![],
            curves: vec![],
            guide_points: vec![],
            polylines: vec![],
            images: vec![],
            active_section: None,
            relationships: vec![],
            container_flags: None,
        };
        let run = run.unwrap_or(&empty);
        let container_id = tables.container_of_def[i];
        let def_attrs = tables.attributes(crate::Owner::Definition(i), None);
        // The preview image: an entry named after the definition
        // (§16.5), a PNG or a JPEG, named in the record; a name unsafe
        // as a path, or one taken, gets another. Every definition takes
        // a name, preview or not, so that a reader can assign the names
        // the same way when the record is missing.
        let name = folder_name(&d.name, &mut thumbnail_names_taken);
        let mut thumbnail_kids = vec![Record::kids(
            0x251D,
            vec![camera_record(
                d.thumbnail_camera.as_ref().unwrap_or(&thumbnail_camera()),
            )],
        )];
        match &d.thumbnail {
            Some(bytes) if matches!(xml26::image_type(bytes), 1 | 4) => {
                let path = format!("thumbnails/{name}.{}", xml26::image_ext(bytes));
                doc.set_entry(&path, bytes.clone());
                let mut image = vec![
                    Record::u32(0x2329, xml26::image_type(bytes)),
                    Record::string(0x232A, &path),
                ];
                if xml26::image_type(bytes) == 1 {
                    image.push(Record::u32(0x232C, 70));
                }
                thumbnail_kids.push(Record::kids(0x251E, vec![Record::kids(0x2328, image)]));
            }
            Some(_) => previews_left_out += 1,
            None => {}
        }
        let cont = container(
            run,
            model,
            &mut tables,
            &mut ids,
            &mut seed,
            false,
            def_attrs,
            container_id,
        );
        let guid = guid_bytes(&d.guid).unwrap_or_else(|| random_guid(&mut seed));
        let b = &d.behaviour;
        def_records.push(Record::kids(
            0x157C,
            vec![
                cont,
                Record::leaf(0x157D, guid.to_vec()),
                Record::string(0x157E, &d.name),
                Record::string(0x157F, &d.description),
                Record::string(0x1580, &d.source_path),
                Record::u8(0x1582, d.flag as u8),
            ]
            .into_iter()
            .chain(
                model
                    .features
                    .procedures
                    .iter()
                    .find(|p| p.definition == i)
                    .map(|p| {
                        Record::kids(
                            0x1588,
                            vec![Record::kids(
                                0x7EC2,
                                vec![
                                    Record::string(0x7EC3, &p.name),
                                    Record::u32(0x7EC4, p.version),
                                    Record::u32(0x7EC5, p.flags),
                                    Record::string(0x7EC6, &p.parameters),
                                ],
                            )],
                        )
                    }),
            )
            .chain([
                Record::u32(
                    0x1583,
                    match d.kind {
                        DefinitionKind::Component => 0,
                        DefinitionKind::Group => 1,
                        DefinitionKind::Image => 2,
                    },
                ),
                Record::u32(0x1581, d.timestamp),
                behaviour_record(b),
                Record::kids(0x1585, vec![Record::kids(0x251C, thumbnail_kids)]),
            ])
            .collect(),
        ));
    }
    let root_run = model.geometry.iter().find(|r| r.def_index.is_none());
    let empty_root = GeometryRun {
        start: 0,
        end: 0,
        top_level: 0,
        frame: None,
        def_index: None,
        topology: Default::default(),
        resolved: (0, 0),
        mesh: Default::default(),
        placed: vec![],
        curve_members: vec![],
        sections: vec![],
        curves: vec![],
        guide_points: vec![],
        polylines: vec![],
        images: vec![],
        active_section: None,
        relationships: vec![],
        container_flags: None,
    };
    let root_id = ids.fresh();
    let root = container(
        root_run.unwrap_or(&empty_root),
        model,
        &mut tables,
        &mut ids,
        &mut seed,
        true,
        geo_attrs,
        root_id,
    );

    // ---- styles: records and documents. Each distinct watermark (by
    // name and image) gets one image entry, one `0x2ee0` record and one
    // id; a style record lists its watermarks' ids in `0x6b70`.
    let mut wm_entries: HashMap<(String, u32), (String, u64)> = HashMap::new(); // (name, image crc) -> (path, id)
    let mut wm_taken: HashSet<String> = HashSet::new();
    let mut watermark_paths =
        |doc: &mut Document, ids: &mut Ids, ws: &[Watermark]| -> Vec<(Watermark, String, u64)> {
            ws.iter()
                .filter_map(|w| {
                    let bytes = w.image_bytes.as_ref()?;
                    let (path, id) = wm_entries
                        .entry((w.name.clone(), crate::zip::crc32(bytes)))
                        .or_insert_with(|| {
                            let base = folder_name(&w.name, &mut wm_taken);
                            let p = format!("watermarks/{base}.{}", xml26::image_ext(bytes));
                            doc.set_entry(&p, bytes.clone());
                            (p, ids.fresh())
                        })
                        .clone();
                    Some((w.clone(), path, id))
                })
                .collect()
        };
    let mut style_records = Vec::with_capacity(styles.len());
    let mut style_folders = HashSet::new();
    for (i, s) in styles.iter().enumerate() {
        let guid = guid_bytes(&s.guid).unwrap_or_else(|| random_guid(&mut seed));
        let wms = watermark_paths(&mut doc, &mut ids, &s.watermarks);
        let wm_ids: Vec<u64> = wms.iter().map(|w| w.2).collect();
        let wms: Vec<(Watermark, String)> = wms.into_iter().map(|w| (w.0, w.1)).collect();
        let folder = folder_name(&s.name, &mut style_folders);
        // The active style carries the document's current settings, as
        // SketchUp's own conversion writes it (§16.14).
        let settings = if i == active_style {
            model.rendering.as_ref().unwrap_or(&s.settings)
        } else {
            &s.settings
        };
        doc.set_entry(
            &format!("styles/{folder}/style.xml"),
            xml26::style_xml(
                &s.name,
                &s.description,
                settings,
                &wms,
                &s.items,
                s.strokes.as_ref(),
            )
            .into_bytes(),
        );
        stroke_entries(&mut doc, &folder, s.strokes.as_ref());
        style_records.push(Record::kids(
            0x6B6C,
            vec![
                entity(style_ids[i]),
                Record::leaf(0x6B6D, guid.to_vec()),
                Record::string(0x6B6E, &s.description),
                Record::string(0x6B6F, &folder),
                Record::id_list(0x6B70, &wm_ids),
            ],
        ));
    }
    // The current settings: the active style's identity with `_1`.
    let current = &styles[active_style];
    let current_name = format!("{}_1", current.name);
    let current_guid = guid_bytes(&current.guid).unwrap_or_else(|| random_guid(&mut seed));
    let current_wms: Vec<(Watermark, String)> =
        watermark_paths(&mut doc, &mut ids, &model.watermarks)
            .into_iter()
            .map(|w| (w.0, w.1))
            .collect();
    let current_name_default = current_name.clone();
    let current_style = model.features.current_style.as_ref();
    let current_name = current_style.map_or(current_name, |c| c.name.clone());
    let current_folder = folder_name(&current_name, &mut style_folders);
    // The current-settings document carries the style's own name: in every
    // observed file it is byte-identical to the active style's document,
    // only its folder ends in `_1`. Its item 1016 is the dashes switch of
    // the current settings (`n-ro-dashes-off`, §16.10).
    let mut current_items = current.items.clone();
    if let Some(on) = model.rendering.as_ref().and_then(|r| r.dashes) {
        match current_items.iter_mut().find(|i| i.id == 1016) {
            Some(it) => it.value = crate::StyleValue::Bool(on),
            None => current_items.push(crate::StyleItem {
                id: 1016,
                value: crate::StyleValue::Bool(on),
            }),
        }
    }
    doc.set_entry(
        &format!("styles/{current_folder}/style.xml"),
        xml26::style_xml(
            current_style
                .filter(|c| c.name != current_name_default)
                .map_or(&current.name, |c| &c.name),
            current_style.map_or(&current.description, |c| &c.description),
            model.rendering.as_ref().unwrap_or(&current.settings),
            &current_wms,
            &current_items,
            current.strokes.as_ref(),
        )
        .into_bytes(),
    );
    stroke_entries(&mut doc, &current_folder, current.strokes.as_ref());
    let style_section = Record::kids(
        0x0206,
        vec![Record::kids(
            0x6978,
            vec![
                Record::kids(0x6979, style_records),
                Record::narrow(0x697A, style_ids[active_style]),
                Record::kids(
                    0x697B,
                    vec![Record::kids(
                        0x6B6C,
                        vec![
                            Record::leaf(0x6B6D, current_guid.to_vec()),
                            Record::string(0x6B6E, &current.description),
                            Record::string(0x6B6F, &current_folder),
                        ],
                    )],
                ),
                Record::u8(0x697C, current_style.is_some_and(|c| c.modified) as u8),
            ],
        )],
    );

    // Every watermark record (`0x0203`): the styles' and the current
    // view's, one per distinct watermark (name and image), in first-use
    // order.
    let mut all_wms: Vec<(Watermark, String, u64)> = Vec::new();
    for w in styles
        .iter()
        .flat_map(|s| s.watermarks.iter())
        .chain(model.watermarks.iter())
    {
        let Some(bytes) = w.image_bytes.as_ref() else {
            continue;
        };
        if let Some((path, id)) = wm_entries.get(&(w.name.clone(), crate::zip::crc32(bytes))) {
            if !all_wms.iter().any(|(_, _, x)| x == id) {
                all_wms.push((w.clone(), path.clone(), *id));
            }
        }
    }
    let mut wm_records = Vec::new();
    for (w, path, wm_id) in &all_wms {
        let (fitting, stretch) = xml26::fitting_of(w);
        let bytes = w.image_bytes.as_deref().unwrap_or(&[]);
        let wm_id = *wm_id;
        wm_records.push(Record::kids(
            0x2EE0,
            vec![
                entity(wm_id),
                Record::string(0x2EE1, &w.source_file),
                Record::u8(0x2EE2, 0),
                Record::u32(0x2EE3, xml26::watermark_time(w)),
                Record::string(0x2EE4, &w.name),
                Record::u32(0x2EE5, w.position),
                Record::kids(
                    0x2EE6,
                    vec![Record::kids(
                        0x2328,
                        vec![
                            Record::u32(0x2329, xml26::image_type(bytes)),
                            Record::string(0x232A, path),
                        ],
                    )],
                ),
                Record::u8(0x2EE7, w.tiled as u8),
                Record::u8(0x2EE8, w.stretched as u8),
                Record::u8(0x2EE9, w.keep_aspect_ratio as u8),
                Record::u32(0x2EEE, fitting),
                Record::u32(0x2EEF, stretch),
                Record::u8(0x2EEA, w.background as u8),
                Record::f64(0x2EEB, w.scale),
                Record::u8(0x2EEC, w.mask as u8),
                Record::f64(0x2EED, w.blend),
            ],
        ));
    }
    let watermark_section = Record::kids(
        0x0203,
        vec![Record::kids(
            0x2CEC,
            vec![Record::kids(0x2CED, wm_records), Record::u32(0x2CEE, model.native_watermark_section_value.unwrap_or(0))],
        )],
    );

    // ---- scenes
    let mut scene_thumbnail_names_taken = HashSet::new();
    // Matched photos (§16.15 `0x0201`): one record per scene that has
    // one, its image an archive entry, named by the scene's `0x7156`.
    let mut photo_records = Vec::new();
    let mut photo_ids: Vec<Option<u64>> = Vec::with_capacity(model.scenes.len());
    let mut photo_names = HashSet::new();
    for s in &model.scenes {
        let Some(p) = &s.match_photo else {
            photo_ids.push(None);
            continue;
        };
        let id = ids.fresh();
        photo_ids.push(Some(id));
        let stem = p
            .path
            .rsplit(['/', '\\'])
            .next()
            .and_then(|f| f.rsplit_once('.').map(|x| x.0).or(Some(f)))
            .filter(|f| !f.is_empty())
            .unwrap_or(&s.name);
        let mut image = Vec::new();
        if !p.image.is_empty() {
            let path = format!(
                "matched_photos/{}.{}",
                folder_name(stem, &mut photo_names),
                xml26::image_ext(&p.image)
            );
            doc.set_entry(&path, p.image.clone());
            image.push(Record::kids(
                0x2AFB,
                vec![Record::kids(
                    0x2328,
                    vec![
                        Record::u32(0x2329, xml26::image_type(&p.image)),
                        Record::string(0x232A, &path),
                    ],
                )],
            ));
        }
        let points: Vec<f64> = p.points.iter().flatten().copied().collect();
        let mut desc = vec![Record::string(0x2AF9, &p.path), Record::u32(0x2AFA, 0)];
        desc.extend(image);
        desc.extend([
            Record::u32(0x2AFC, p.width),
            Record::u32(0x2AFD, p.height),
            Record::u32(0x2AFE, 0),
            Record::u32(0x2AFF, p.time),
        ]);
        photo_records.push(Record::kids(
            0x2904,
            vec![
                entity(id),
                Record::kids(0x2905, vec![Record::kids(0x2AF8, desc)]),
                Record::u8(0x2906, 1),
                Record::f64(0x2907, p.scale),
                Record::f64s_of(0x2908, &points),
                Record::f64s_of(0x2909, &[0.0; 3]),
                Record::f64(0x290A, 0.0),
                Record::u32(0x290B, 0),
            ],
        ));
    }
    // The document names a photo in `0x0202`: the only one observed.
    let current_photo = photo_ids.iter().rev().flatten().next().copied();
    let scene_records: Vec<Record> = model
        .scenes
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let mut bits = s.saved.0 | 512;
            // A page that does not save the environment property.
            if model
                .features
                .scene_environment
                .iter()
                .any(|(i, p)| *i == k && !p.saved)
            {
                bits &= !512;
            }
            // A 2017 page saves one hidden-geometry property; 2026 splits
            // it in two. A page that already holds a bit past the 2017 ones
            // (a page read from a 2026 file) has its own.
            if s.saved.has(SceneProperties::HIDDEN_GEOMETRY) && s.saved.0 < 128 {
                bits |= SceneProperties::HIDDEN_GEOMETRY_2 | SceneProperties::HIDDEN_OBJECTS;
            }
            let mut kids = vec![
                Record::kids(
                    0x6F54,
                    vec![
                        entity(scene_ids[k]),
                        Record::string(0x6F55, &s.name),
                        Record::string(0x6F56, &s.description),
                    ],
                ),
                Record::u32(0x7149, bits),
            ];
            if let Some(c) = &s.camera {
                kids.push(Record::kids(0x714A, vec![camera_record(c)]));
            }
            if bits
                & (SceneProperties::HIDDEN_GEOMETRY
                    | SceneProperties::HIDDEN_GEOMETRY_2
                    | SceneProperties::HIDDEN_OBJECTS)
                != 0
            {
                let hidden: Vec<u64> = s
                    .hidden_entities
                    .iter()
                    .filter_map(|p| tables.id_of_pid.get(p).copied())
                    .collect();
                kids.push(Record::id_list(0x714B, &hidden));
            }
            // A scene that saves no style names none, as a Match Photo
            // scene holds it (§16.15).
            if s.style.is_some() || s.saved.has(SceneProperties::STYLE_AND_FOG) {
                kids.push(Record::narrow(0x714C, style_ids[scene_style(s)]));
            }
            if let Some(r) = &s.rendering {
                let sketchy = styles[scene_style(s)].strokes.is_some();
                kids.push(Record::kids(0x714D, vec![rendering_record(r, sketchy)]));
            }
            if let Some(sh) = &s.shadows {
                kids.push(Record::kids(0x714E, vec![shadow_record(sh)]));
            }
            if bits & 512 != 0 {
                kids.push(Record::kids(
                    0x715B,
                    vec![
                        Record::u8(0x7B0F, 0),
                        Record::u8(0x7B10, 0),
                        Record::u32(0x7B11, 0),
                        Record::f32(0x7B12, 1.0),
                        Record::f32(0x7B13, 1.0),
                        Record::u8(0x7B14, 0),
                        Record::u32(0x7B15, 0),
                        Record::u32(0x7B16, 0),
                    ],
                ));
            }
            if let Some(a) = &s.axes {
                kids.push(Record::kids(0x714F, vec![axes_record(Some(a))]));
            }
            if s.saved.has(SceneProperties::VISIBLE_LAYERS) {
                let hidden: Vec<u64> = s
                    .hidden_layers
                    .iter()
                    .filter_map(|&i| layer_ids.get(i).copied())
                    .collect();
                kids.push(Record::id_list(0x7150, &hidden));
                kids.push(Record::empty(0x7159));
            }
            if s.saved.has(SceneProperties::ACTIVE_SECTION_PLANES) {
                let active: Vec<u64> = s
                    .active_section_planes
                    .iter()
                    .filter_map(|p| tables.id_of_pid.get(p).copied())
                    .collect();
                kids.push(Record::id_list(0x7151, &active));
            }
            kids.push(Record::u8(0x7152, s.in_animation as u8));
            kids.push(Record::string(
                0x7153,
                &if s.in_animation {
                    s.name.clone()
                } else {
                    format!("({})", s.name)
                },
            ));
            kids.push(Record::f64(0x7154, -1.0));
            kids.push(Record::f64(0x7155, -1.0));
            if let Some(id) = photo_ids[k] {
                kids.push(Record::narrow(0x7156, id));
            }
            kids.push(Record::empty(0x715C));
            match &s.thumbnail {
                Some(bytes) if bytes.starts_with(b"\x89PNG") => {
                    let name = folder_name(&s.name, &mut scene_thumbnail_names_taken);
                    let path = format!("scene_thumbnails/{name}.png");
                    doc.set_entry(&path, bytes.clone());
                    kids.push(Record::u8(0x7157, 1));
                    kids.push(Record::kids(0x7158, vec![Record::string(0x2134, &path)]));
                }
                Some(_) => {
                    previews_left_out += 1;
                    kids.push(Record::u8(0x7157, 0));
                }
                None => kids.push(Record::u8(0x7157, 0)),
            }
            Record::kids(0x7148, kids)
        })
        .collect();
    let mut scene_kids = vec![Record::kids(0x6D61, scene_records)];
    if let Some(&selected) = model
        .selected_scene
        .and_then(|i| scene_ids.get(i))
        .or(scene_ids.last())
    {
        scene_kids.push(Record::narrow(0x6D62, selected));
    }
    let scene_section = Record::kids(0x0207, vec![Record::kids(0x6D60, scene_kids)]);

    // ---- option sets and the model attribute dictionaries
    let units = written_units(model);
    let u = &units;
    let a = &model.animation;
    let options = Record::kids(
        0x0200,
        vec![Record::kids(
            0x61A8,
            vec![Record::kids(
                0x61A9,
                vec![
                    option_set(
                        "PageOptions",
                        vec![
                            ("ShowTransition", value(&AttrValue::Bool(a.transitions))),
                            ("TransitionTime", value(&AttrValue::F64(a.transition_s))),
                        ],
                    ),
                    option_set("NamedOptions", vec![]),
                    option_set(
                        "SlideshowOptions",
                        vec![
                            ("LoopSlideshow", value(&AttrValue::Bool(a.loop_slideshow))),
                            ("SlideTime", value(&AttrValue::F64(a.delay_s))),
                        ],
                    ),
                    option_set(
                        "UnitsOptions",
                        vec![
                            (
                                "LengthPrecision",
                                value(&AttrValue::Int(u.length_precision as i32)),
                            ),
                            (
                                "LengthFormat",
                                value(&AttrValue::Int(u.length_format as i32)),
                            ),
                            ("LengthUnit", value(&AttrValue::Int(u.length_unit as i32))),
                            ("LengthSnapEnabled", value(&AttrValue::Bool(u.length_snap))),
                            (
                                "LengthSnapLength",
                                value(&AttrValue::F64(u.length_snap_length)),
                            ),
                            (
                                "AnglePrecision",
                                value(&AttrValue::Int(u.angle_precision as i32)),
                            ),
                            ("AngleSnapEnabled", value(&AttrValue::Bool(u.angle_snap))),
                            ("SnapAngle", value(&AttrValue::F64(u.snap_angle))),
                            (
                                "SuppressUnitsDisplay",
                                value(&AttrValue::Bool(u.suppress_units_display)),
                            ),
                            (
                                "ForceInchDisplay",
                                value(&AttrValue::Bool(u.force_inch_display)),
                            ),
                            (
                                "AreaUnit",
                                value(&AttrValue::Int(u.area_unit.unwrap_or(0) as i32)),
                            ),
                            (
                                "VolumeUnit",
                                value(&AttrValue::Int(u.volume_unit.unwrap_or(0) as i32)),
                            ),
                            (
                                "AreaPrecision",
                                value(&AttrValue::Int(u.area_precision.unwrap_or(2) as i32)),
                            ),
                            (
                                "VolumePrecision",
                                value(&AttrValue::Int(u.volume_precision.unwrap_or(2) as i32)),
                            ),
                        ],
                    ),
                ],
            )],
        )],
    );
    let model_attrs = Record::kids(
        0x0209,
        vec![Record::kids(
            0x36B1,
            vec![Record::kids(
                0x36B2,
                if model_own.is_empty() {
                    vec![dictionary(
                        "ModelProperties",
                        vec![
                            ("Description", string_value("")),
                            ("IsClassified", value(&AttrValue::Bool(false))),
                            ("IsDynamic", value(&AttrValue::Bool(false))),
                            ("IsLive", value(&AttrValue::Bool(false))),
                            ("Name", string_value("")),
                        ],
                    )]
                } else {
                    model_own
                },
            )],
        )],
    );

    // ---- defaults for texts and dimensions
    let td = model.text_defaults.clone().unwrap_or(crate::TextDefaults {
        font: None,
        screen_font: None,
        arrow: 3,
        leader_text_color: [0x3F, 0x3F, 0x3F, 0xFF],
        screen_text_color: [0x3F, 0x3F, 0x3F, 0xFF],
        leader: 2,
    });
    let dd = model
        .dimension_defaults
        .clone()
        .unwrap_or(crate::DimensionDefaults {
            font: None,
            aligned: false,
            arrow: 1,
            text_position: 3,
            color: [0x3F, 0x3F, 0x3F, 0xFF],
            highlight_non_associated: false,
            radius_prefix: true,
            hide_foreshortened: false,
            hide_small: false,
        });
    let text_defaults = Record::kids(
        0x01FE,
        vec![Record::kids(
            0x57E4,
            vec![
                Record::narrow(0x57E5, font_id(&tables, td.font)),
                Record::narrow(0x57E6, font_id(&tables, td.screen_font)),
                Record::u32(0x57E7, td.arrow),
                Record::u32(0x57E8, 1),
                Record::u8(0x57E9, 0),
                Record::u32(0x57EA, td.leader),
                Record::u8(0x57EB, 1),
                rgba(0x57EC, td.leader_text_color),
                rgba(0x57ED, td.screen_text_color),
            ],
        )],
    );
    let dimension_defaults = Record::kids(
        0x01FF,
        vec![Record::kids(
            0x5FB4,
            vec![
                Record::narrow(0x5FB5, font_id(&tables, dd.font)),
                Record::u8(0x5FB6, dd.aligned as u8),
                Record::u8(0x5FB7, 1),
                Record::u32(0x5FB8, 5),
                Record::u32(0x5FB9, 10),
                Record::u32(0x5FBA, 1),
                Record::u32(0x5FBB, dd.arrow),
                Record::u32(0x5FBC, 10),
                Record::u8(0x5FBD, dd.highlight_non_associated as u8),
                rgba(0x5FBE, [0xFF, 0, 0, 0xFF]),
                Record::u8(0x5FBF, dd.radius_prefix as u8),
                Record::u8(0x5FC0, dd.hide_foreshortened as u8),
                Record::f64(0x5FC1, 0.6),
                Record::u8(0x5FC2, dd.hide_small as u8),
                Record::f64(0x5FC3, 10.0),
                rgba(0x5FC4, dd.color),
                rgba(0x5FC5, [0, 0, 0, 0xFF]),
                Record::u32(0x5FC6, dd.text_position),
            ],
        )],
    );

    // ---- assemble model.dat
    let next_id = ids.fresh();
    let default_ro = RenderingOptions::sketchup_default();
    let top = Record::kids(
        0x01F4,
        vec![
            Record::u32(0x0063, 0),
            Record::string(0x020A, "meta/meta.dat"),
            model_attrs,
            options,
            Record::kids(
                0x01F5,
                vec![Record::kids(0x03E8, vec![Record::narrow(0x03E9, next_id)])],
            ),
            Record::kids(
                0x01FA,
                vec![camera_record(
                    model.camera.as_ref().unwrap_or(&default_camera()),
                )],
            ),
            Record::kids(0x01FB, {
                let sketchy = styles[active_style].strokes.is_some();
                let mut ro =
                    rendering_record(model.rendering.as_ref().unwrap_or(&default_ro), sketchy);
                // The display settings' own dictionaries lead the record in
                // an entity base without an id (§16.10).
                if let Some(a) = tables.attributes(crate::Owner::RenderingOptions, None) {
                    if let crate::tree26::Body::Kids(k) = &mut ro.body {
                        k.insert(0, Record::kids(0x05DC, vec![a]));
                    }
                }
                if let Some(h) = &model.features.render_hatch {
                    crate::build26x::insert_render_hatch(&mut ro, h);
                }
                vec![ro]
            }),
            Record::kids(
                0x0204,
                vec![shadow_record(
                    model.shadows.as_ref().unwrap_or(&default_shadows()),
                )],
            ),
            Record::kids(0x01FC, vec![axes_record(model.axes.as_ref())]),
            line_styles(&mut ids),
            Record::kids(
                0x01F7,
                vec![Record::kids(0x30D4, {
                    let mut kids = vec![Record::kids(0x30D5, material_records)];
                    // The current paint material (§16.6).
                    if let Some(&id) = model.current_material.and_then(|i| material_ids.get(i)) {
                        kids.push(Record::narrow(0x30D6, id));
                    }
                    kids
                })],
            ),
            Record::kids(
                0x01F8,
                vec![Record::kids(
                    0x3A98,
                    vec![
                        Record::kids(0x3A99, layer_records),
                        Record::narrow(
                            0x3A9A,
                            model
                                .active_layer
                                .and_then(|i| layer_ids.get(i).copied())
                                .unwrap_or(layer_ids[0]),
                        ),
                        Record::kids(
                            0x3A9B,
                            vec![Record::kids(
                                0x3E80,
                                vec![
                                    Record::empty(0x3E81),
                                    Record::empty(0x3E83),
                                    Record::empty(0x3E84),
                                    Record::u8(0x3E82, 0),
                                    Record::u8(0x3E85, 0),
                                ],
                            )],
                        ),
                    ],
                )],
            ),
            Record::kids(
                0x01FD,
                vec![Record::kids(
                    0x4E20,
                    vec![Record::kids(0x4E21, font_records)],
                )],
            ),
            text_defaults,
            dimension_defaults,
            if photo_records.is_empty() {
                Record::empty(0x0201)
            } else {
                Record::kids(0x0201, photo_records)
            },
            watermark_section,
            Record::kids(
                0x01F9,
                vec![
                    Record::u32(0x0063, 0),
                    Record::kids(
                        0x1770,
                        vec![
                            Record::u32(0x0063, 0),
                            Record::kids(0x1771, {
                                let mut v = vec![Record::u32(0x0063, 0)];
                                v.extend(def_records);
                                v
                            }),
                        ],
                    ),
                ],
            ),
            Record::kids(0x01F6, vec![root]),
            Record::u8(0x020C, model.anti_aliased_textures.unwrap_or(true) as u8),
            Record::u8(0x0214, 0),
            Record::u32(0x020D, model.app_session_count.unwrap_or(1)),
            style_section,
            Record::kids(
                0x0210,
                vec![Record::kids(0x7918, vec![Record::empty(0x7919)])],
            ),
            Record::kids(
                0x0213,
                vec![Record::kids(0x7D64, vec![Record::empty(0x7D65)])],
            ),
            scene_section,
            Record::kids(
                0x0205,
                model
                    .classifications
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let entry =
                            format!("classifications/{}.skc", folder_name(&c.name, &mut taken));
                        let _ = i;
                        doc.set_entry(&entry, c.data.clone());
                        Record::kids(
                            0x6784,
                            vec![
                                Record::string(0x6785, &c.source_path),
                                Record::string(0x6786, &c.name),
                                Record::string(0x6787, &entry),
                            ],
                        )
                    })
                    .collect(),
            ),
            Record::kids(0x020E, vec![behaviour_record(&Default::default())]),
            Record::u32(0x020F, 0),
        ],
    );
    // `0x03e9` is the next id to allocate: past the line styles too.
    let mut top = top;
    if let Some(list) = crate::build26x::hatch_list(model, &tables.hatch_ids, &mut doc) {
        crate::build26x::insert_hatch_list(&mut top, list);
    }
    if let (Some(id), crate::tree26::Body::Kids(kids)) = (current_photo, &mut top.body) {
        if let Some(at) = kids.iter().position(|r| r.tag == 0x0201) {
            kids.insert(at + 1, Record::narrow(0x0202, id));
        }
    }
    let folder_ids =
        crate::build26x::apply_layer_features(model, &mut top, &layer_ids, &mut || ids.fresh());
    crate::build26x::apply_page_extras(model, &mut top, &folder_ids);
    let lost_owners = crate::owners26::write(model, &mut top, &mut |o| tables.attributes(o, None));
    crate::build26x::apply_environments(model, &mut top, &mut doc, &mut || ids.fresh());
    if let (false, crate::tree26::Body::Kids(kids)) =
        (model.features.procedure_kinds.is_empty(), &mut top.body)
    {
        kids.push(crate::build26x::procedure_registry(
            &model.features.procedure_kinds,
        ));
    }
    if let (Some(c), crate::tree26::Body::Kids(kids)) = (&model.features.crs, &mut top.body) {
        kids.push(crate::build26x::crs_section(c));
    }
    let last = ids.fresh();
    if let Some(r) = top.path_mut(&[0x01F5, 0x03E8, 0x03E9]) {
        *r = Record::narrow(0x03E9, last);
    }

    // ---- entries the model refers to
    let unit_name = match written_units(model).length_unit {
        1 => "Feet",
        2 => "Millimeters",
        3 => "Centimeters",
        4 => "Meters",
        _ => "Inches",
    };
    // The model's previews as it carries them, else flat images.
    let flat = xml26::flat_png(128, 96, [0xF0, 0xF0, 0xF0]);
    let mut png_or_flat = |b: &Option<Vec<u8>>| match b {
        Some(bytes) if bytes.starts_with(b"\x89PNG") => bytes.clone(),
        Some(_) => {
            previews_left_out += 1;
            flat.clone()
        }
        None => flat.clone(),
    };
    doc.set_entry("meta/model_thumbnail.png", png_or_flat(&model.thumbnail));
    doc.set_entry("meta/preview_thumbnail.png", png_or_flat(&model.preview));
    let guid = model.model_guid.as_deref().and_then(guid_bytes).unwrap_or_else(|| random_guid(&mut seed));
    let temp: String = guid.iter().map(|b| format!("{b:02x}")).collect();
    doc.set_entry(
        "meta/meta.dat",
        xml26::meta_dat(
            &guid,
            unit_name,
            &temp,
            xml26::now_secs(),
            revision,
            &model.save_history,
        ),
    );
    doc.set_tree(&top);
    if previews_left_out > 0 {
        doc.notes.push(format!(
            "{previews_left_out} previews are not PNGs and are left out: a 2026 file holds PNG previews"
        ));
    }
    if tables.lost_crossings > 0 {
        doc.notes.push(format!(
            "{} texts and dimensions are anchored where two edges meet, on an edge the file does not hold, and are left out",
            tables.lost_crossings
        ));
    }
    if tables.lost_radial_dimensions > 0 {
        doc.notes.push(format!(
            "{} radial dimensions measure an edge their entity list does not hold and are left out",
            tables.lost_radial_dimensions
        ));
    }
    if tables.lost_edgeless_radials > 0 {
        doc.notes.push(format!(
            "{} radial dimensions hold their arc instead of naming an edge, which this writer cannot express in the 2026 container, and are left out",
            tables.lost_edgeless_radials
        ));
    }
    if tables.lost_relationships > 0 {
        doc.notes.push(format!(
            "{} relationships name entities their list does not hold and are left out",
            tables.lost_relationships
        ));
    }
    if lost_owners > 0 {
        doc.notes.push(format!(
            "{lost_owners} settings or lists with dictionaries have no record in the file and are left out"
        ));
    }
    if tables.lost_values > 0 {
        doc.notes.push(format!(
            "{} dictionary values are of types the 2026 container is not known to hold and are left out",
            tables.lost_values
        ));
    }
    doc.reorder_entries();
    Ok(doc)
}

fn behaviour_record(b: &crate::Behaviour) -> Record {
    Record::kids(
        0x1B58,
        vec![
            Record::u32(0x1B59, b.glue_plane),
            Record::u32(0x1B5A, b.extra),
            Record::u8(0x1B5B, b.glues_to_surface as u8),
            Record::u8(0x1B5C, b.cuts_opening as u8),
            Record::u8(0x1B5D, b.always_faces_camera as u8),
            Record::u8(0x1B5E, b.shadows_face_sun as u8),
        ],
    )
}

/// The camera every definition thumbnail record carries when no image
/// is stored.
fn thumbnail_camera() -> Camera {
    Camera {
        eye_m: [0.0, 1.0 * M_PER_INCH, 0.0],
        target_m: [0.0, 187.42544670603 * M_PER_INCH, 0.0],
        up: [0.0, 0.0, 1.0],
        perspective: true,
        fov_deg: 35.0,
        parallel_height_m: 133.0 * M_PER_INCH,
        two_point_perspective: false,
        description: String::new(),
        undecoded: [0.0, 0.0],
        aspect_ratio: 0.0,
        scale_2d: 1.0,
        offset_2d: [0.0; 2],
        image_width: 0.0,
    }
}

/// The root container's `GeoReference` dictionary, from the model's
/// attributes when it carries a geo-location.
/// The `GeoReference` dictionary the model's settings give (§16.8): the
/// geo-location keys among its attributes, with the flag; none for a
/// model without any.
fn geo_reference(model: &Model) -> Option<crate::Dictionary> {
    let keys = [
        "GeoReferenceNorthAngle",
        "Latitude",
        "Longitude",
        "ModelTranslationX",
        "ModelTranslationY",
        "UsesGeoReferencing",
    ];
    let mut entries: Vec<(String, crate::Value)> = keys
        .iter()
        .filter_map(|k| {
            model.attributes.iter().find(|a| a.key == *k).map(|a| {
                (
                    k.to_string(),
                    match &a.value {
                        AttrValue::Int(i) => crate::Value::Int(*i),
                        AttrValue::F64(f) => crate::Value::Double(*f),
                        AttrValue::Bool(b) => crate::Value::Bool(*b),
                    },
                )
            })
        })
        .collect();
    if entries.is_empty() && !model.geo_located {
        return None;
    }
    if !entries.iter().any(|(k, _)| k == "UsesGeoReferencing") {
        entries.push((
            "UsesGeoReferencing".into(),
            crate::Value::Bool(model.geo_located),
        ));
    }
    Some(crate::Dictionary {
        name: "GeoReference".into(),
        entries,
        flag: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_safe_and_distinct() {
        let mut taken = HashSet::new();
        assert_eq!(folder_name(".", &mut taken), "_");
        assert_eq!(folder_name("..", &mut taken), "_1");
        assert_eq!(folder_name("a/b:c", &mut taken), "a_b_c");
        assert_eq!(folder_name("Wall", &mut taken), "Wall");
        assert_eq!(folder_name("wall", &mut taken), "wall1");
        assert_eq!(folder_name("x\u{1}y", &mut taken), "x_y");
    }

    #[test]
    fn guid_bytes_takes_only_32_hex_digits() {
        assert_eq!(
            guid_bytes("000102030405060708090a0b0c0d0e0f"),
            Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])
        );
        assert_eq!(guid_bytes(&format!("{}ab", "€".repeat(10))), None);
        assert_eq!(guid_bytes("+f0102030405060708090a0b0c0d0e0f"), None);
        assert_eq!(guid_bytes("abc"), None);
    }
}
