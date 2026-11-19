//! Authoring a stream-container file from a [`Model`] (`docs/WRITING.md`): the typed
//! document of [`crate::file17`] filled from the model, every decoded
//! field from the model's value and every undecoded field at the value
//! SketchUp writes in a default document (`corpus/2017/empty.skp`).
//!
//! The document is built in stream order, because an object is written
//! where it is first mentioned and referred back to afterwards: a texture
//! image inside the first material that uses it, a vertex inside its
//! first edge, a font inside the first annotation, a style inside the
//! first scene that names it. Definitions are written before the
//! definitions that place them, as SketchUp writes them.
//!
//! What a `Model` does not carry is not written (`docs/WRITING.md` lists
//! every gap).

use std::collections::{HashMap, HashSet};

use crate::ar17::S16;
use crate::file17::*;
use crate::model::Error;
use crate::settings::{RenderingOptions, RoKind, RoValue, RO_FIELDS};
use crate::{
    AttrValue, DefinitionKind, GeometryRun, Material, Model, Release, Scene, SceneProperties, INCH,
};

const M_PER_INCH: f64 = 0.0254;

/// The display settings block of a default document (§10.7).
pub(crate) const RENDERING: [u8; RENDERING_LEN] = [
    0x02, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0xcc, 0xcc, 0xc9, 0xff, 0x00,
    0x00, 0x00, 0xff, 0x00, 0x01, 0xff, 0xff, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00,
    0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff,
    0xff, 0xa4, 0xb2, 0xbb, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xd0, 0x3f, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0xe0, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0xf0, 0xbf, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0xbf, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00, 0xc6, 0xcd, 0xd0, 0xff, 0x00, 0x00, 0x00,
    0x00, 0xd2, 0xd0, 0xb9, 0xff, 0x01, 0x00, 0x01, 0x32, 0x00, 0x00, 0x00, 0xff, 0x87, 0x00, 0xff,
    0x70, 0x69, 0x61, 0xff, 0x00, 0x00, 0x00, 0xff, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x88, 0x44, 0xe7, 0x4a, 0x18, 0x57, 0xd6, 0x3f, 0x01, 0xff, 0x00,
    0x00, 0xff, 0x01, 0xcd, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xe4, 0x3f, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0xf0, 0x3f, 0x01, 0x9a, 0x99, 0x99, 0x99, 0x99, 0x99, 0xe9, 0x3f,
];

/// Rendering offset of "shadows displayed" (§10.7), which lives in the
/// display settings and not in the shadow record.
const SHADOWS_DISPLAYED: usize = 27;

/// The dimension defaults of a default document (§10.11).
const DIMENSION_DEFAULTS: [u8; 61] = [
    0, 1, 5, 0, 0, 0, 10, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 10, 0, 0, 0, 0, 255, 0, 0, 255, 1, 0,
    51, 51, 51, 51, 51, 51, 227, 63, 0, 0, 0, 0, 0, 0, 0, 36, 64, 63, 63, 63, 255, 0, 0, 0, 255, 3,
    0, 0, 0, 0, 0, 0,
];

/// The text defaults of a default document (§10.11).
const TEXT_DEFAULTS: [u8; 22] = [
    2, 0, 0, 0, 1, 0, 0, 0, 0, 3, 0, 0, 0, 1, 63, 63, 63, 255, 63, 63, 63, 255,
];

/// The items of a style (§10.8) with the values of the default style, in
/// the order every style lists them. Colours are u32 values.
const STYLE_ITEMS: &[(u32, StyleDefault)] = &[
    (1000, StyleDefault::U32(1)),
    (1001, StyleDefault::U32(0)),
    (1002, StyleDefault::U32(1)),
    (1004, StyleDefault::U8(0)),
    (1005, StyleDefault::U32(2)),
    (1006, StyleDefault::U8(1)),
    (1007, StyleDefault::U32(2)),
    (1008, StyleDefault::U8(0)),
    (1009, StyleDefault::U32(4)),
    (1010, StyleDefault::U8(0)),
    (1011, StyleDefault::U32(7)),
    (1012, StyleDefault::U8(0)),
    (1014, StyleDefault::U32(0xFF00_0000)),
    (1015, StyleDefault::U8(0)),
    (2001, StyleDefault::U32(2)),
    (2002, StyleDefault::U32(0xFFFF_FFFF)),
    (2003, StyleDefault::U32(0xFFBB_B2A4)),
    (2004, StyleDefault::U8(0)),
    (2005, StyleDefault::U8(1)),
    (2006, StyleDefault::U32(0)),
    (2007, StyleDefault::U8(1)),
    (2008, StyleDefault::F64(0.65)),
    (4000, StyleDefault::U32(0xFFC9_CCCC)),
    (4001, StyleDefault::U32(0xFFD0_CDC6)),
    (4002, StyleDefault::U8(1)),
    (4003, StyleDefault::U32(0xFFB9_D0D2)),
    (4004, StyleDefault::U8(0)),
    (4005, StyleDefault::U32(50)),
    (4006, StyleDefault::U8(1)),
    (5000, StyleDefault::U8(1)),
    (ITEM_WATERMARKS, StyleDefault::Watermarks),
    (7000, StyleDefault::U32(0xFFFF_0100)),
    (7001, StyleDefault::U32(0xFF00_00FF)),
    (7002, StyleDefault::U32(0xFF00_0000)),
    (7003, StyleDefault::U32(0xFF00_87FF)),
    (7004, StyleDefault::U32(0xFF61_6970)),
    (7005, StyleDefault::U32(0xFF00_0000)),
    (7008, StyleDefault::U8(1)),
    (7010, StyleDefault::U8(0)),
    (7011, StyleDefault::U8(0)),
    (7012, StyleDefault::U8(0)),
    (7013, StyleDefault::U32(2)),
    (7014, StyleDefault::U32(3)),
    (8000, StyleDefault::U8(1)),
    (8001, StyleDefault::F64(1.0)),
    (8002, StyleDefault::U8(1)),
    (8003, StyleDefault::F64(0.8)),
];

#[derive(Clone, Copy)]
enum StyleDefault {
    U8(u8),
    U32(u32),
    F64(f64),
    Watermarks,
}

/// What the model could not give the file, for the caller to see.
#[derive(Debug, Clone, Default)]
pub(crate) struct Built {
    pub file: File17,
    pub notes: Vec<String>,
}

/// What makes two watermarks one object: the name, the file, the image
/// and every setting.
type WatermarkKey = (String, String, u32, [u64; 8]);

/// An object written where it is first mentioned.
struct Once {
    id: u32,
    body: Option<Body>,
}

impl Once {
    fn ptr(&mut self) -> Ptr {
        match self.body.take() {
            Some(b) => Ptr::new(self.id, b),
            None => Ptr::Ref(self.id),
        }
    }
}

/// The objects of one run, by the persistent id the model gives them.
#[derive(Default)]
struct RunTable {
    entities: HashMap<u32, u32>,
    vertices: HashMap<u32, u32>,
    instances: HashMap<u32, u32>,
}

/// Persistent ids: an entity keeps the model's when it is nonzero and
/// free, else it takes the next unused one.
struct Pids {
    next: u32,
    used: HashSet<u32>,
    max: u32,
}

impl Pids {
    fn fresh(&mut self) -> u32 {
        while self.used.contains(&self.next) {
            self.next += 1;
        }
        let pid = self.next;
        self.used.insert(pid);
        self.max = self.max.max(pid);
        pid
    }
    fn keep(&mut self, pid: u32) -> u32 {
        if pid != 0 && self.used.insert(pid) {
            self.max = self.max.max(pid);
            pid
        } else {
            self.fresh()
        }
    }
}

/// The dash patterns every file of 2019 and later lists (§14): name and
/// pattern.
const LINE_STYLES: [(&str, &str); 12] = [
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

struct Builder<'m> {
    model: &'m Model,
    /// The release the file is written as.
    release: Release,
    next_id: u32,
    pids: Pids,
    seed: u64,
    notes: Vec<String>,
    /// Material slot → object id.
    material_of_slot: HashMap<u16, u32>,
    /// Layer slot → object id (the default layer has none).
    layer_of_slot: HashMap<u16, u32>,
    /// Layer index → object id.
    layer_ids: Vec<u32>,
    stock_line_styles: Vec<Ptr>,
    /// Definition index → object id, once written.
    def_ids: Vec<Option<u32>>,
    fonts: Vec<Once>,
    styles: Vec<Once>,
    instance_names: HashMap<u32, String>,
    /// Dictionaries and texture placements a release without attributes
    /// left out (§14, 3 → 4).
    lost_dictionaries: usize,
    /// Dictionaries of a scene's sub-record the scene does not save.
    lost_unsaved: usize,
    lost_placements: usize,
    /// Objects by persistent id, for scenes; the model's own entity wins
    /// over one inside a definition.
    obj_of_pid: HashMap<u32, u32>,
    /// Objects by persistent id per run of the model, for anchors: a
    /// 2017 file can reuse an id across definitions, so an anchor is
    /// resolved in the run that holds it.
    runs: Vec<RunTable>,
    /// Images already written, by content.
    dib_of_image: HashMap<Vec<u8>, u32>,
    /// The model's dictionaries, by owner.
    dicts: HashMap<crate::Owner, Vec<crate::Dictionary>>,
}

/// The stored value `x` (inches) the reader turns into `m` (metres) by
/// `read`: the quotient, or a neighbour of it when the quotient misses
/// by the last bit. A value that came from a file is written back as the
/// very bits the file held.
pub(crate) fn unread(m: f64, guess: f64, read: impl Fn(f64) -> f64) -> f64 {
    if !guess.is_finite() || read(guess) == m {
        return guess;
    }
    let bits = guess.to_bits();
    for d in 1u64..=4 {
        for c in [bits.wrapping_add(d), bits.wrapping_sub(d)] {
            let x = f64::from_bits(c);
            if x.is_finite() && read(x) == m {
                return x;
            }
        }
    }
    guess
}

/// Inches of a length the reader multiplies by 0.0254: vertices.
pub(crate) fn inch_v(m: f64) -> f64 {
    unread(m, m / M_PER_INCH, |x| x * M_PER_INCH)
}

/// Inches of a length the reader divides by [`INCH`]: every other
/// length.
pub(crate) fn inch(m: f64) -> f64 {
    unread(m, m * INCH, |x| x / INCH)
}

fn inches(m: [f64; 3]) -> [f64; 3] {
    m.map(inch)
}

/// An arc as the 14 f64 of a `CArcCurve` (§6.4), inches: centre,
/// normal, x axis, start and end angle, y axis.
fn arc_params(a: &crate::Arc) -> [f64; 14] {
    let mut p = [0.0f64; 14];
    p[..3].copy_from_slice(&inches(a.center_m));
    p[3..6].copy_from_slice(&a.normal);
    p[6..9].copy_from_slice(&inches(a.x_axis_m));
    p[9] = a.start_rad;
    p[10] = a.end_rad;
    p[11..].copy_from_slice(&inches(a.y_axis_m));
    p
}

fn put(dst: &mut [u8], at: usize, src: &[u8]) {
    dst[at..at + src.len()].copy_from_slice(src);
}

/// A camera (§10.6).
fn camera(c: &crate::Camera) -> Camera17 {
    let mut cam = Camera17::default();
    let b = &mut cam.body;
    let up = crate::build26::upright(c);
    for (k, v) in inches(c.eye_m)
        .iter()
        .chain(&inches(c.target_m))
        .chain(&up)
        .enumerate()
    {
        put(b, k * 8, &v.to_le_bytes());
    }
    put(b, 72, &c.undecoded[0].to_le_bytes());
    put(b, 80, &c.undecoded[1].to_le_bytes());
    b[88] = c.perspective as u8;
    put(b, 89, &c.fov_deg.to_le_bytes());
    put(b, 97, &inch(c.parallel_height_m).to_le_bytes());
    cam.description = S16::new(&c.description);
    put(b, 129, &c.aspect_ratio.to_le_bytes());
    put(&mut cam.tail, 0, &c.image_width.to_le_bytes());
    cam.tail[8] = c.two_point_perspective as u8;
    put(&mut cam.tail, 9, &c.scale_2d.to_le_bytes());
    put(&mut cam.tail, 17, &c.offset_2d[0].to_le_bytes());
    put(&mut cam.tail, 25, &c.offset_2d[1].to_le_bytes());
    cam
}

fn default_camera() -> crate::Camera {
    crate::Camera {
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

/// The camera of a definition that stores no thumbnail image: at
/// (0, 0, 1) looking along +Z, 30° field of view, as every group's
/// definition holds it.
fn empty_camera() -> Camera17 {
    let mut cam = Camera17::default();
    let b = &mut cam.body;
    for (k, v) in [0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 1.0, 0.0]
        .iter()
        .enumerate()
    {
        put(b, k * 8, &f64::to_le_bytes(*v));
    }
    b[88] = 1;
    put(b, 89, &30.0f64.to_le_bytes());
    put(b, 97, &1.0f64.to_le_bytes());
    put(&mut cam.tail, 9, &1.0f64.to_le_bytes());
    cam
}

/// One rendering value as the bytes of a field of `kind`.
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

/// A distance of the model (metres, negative for unset) as the file
/// stores it (inches).
fn stored(name: &str, v: RoValue) -> RoValue {
    match v {
        RoValue::F64(f) if name.ends_with("_m") && f >= 0.0 => RoValue::F64(inch(f)),
        v => v,
    }
}

/// The display settings a release's block has no byte for (§14,
/// [`Release::rendering_parts`]) that `ro` sets otherwise than the
/// default document, by name: what a file of the release cannot keep.
fn rendering_lost(ro: &RenderingOptions, release: Release) -> Vec<&'static str> {
    let held = |o: usize| {
        release
            .rendering_parts()
            .iter()
            .any(|&(a, b)| (a..b).contains(&o))
    };
    let default = RenderingOptions::sketchup_default();
    RO_FIELDS
        .iter()
        .zip(ro.values().into_iter().zip(default.values()))
        .filter(|(f, ((_, a), (_, b)))| {
            (f.off17.is_some_and(|o| !held(o)) && a.is_some() && a != b)
                || (f.off17.is_none()
                    && f.item.is_none()
                    && a.as_ref().is_some_and(|v| only_2026_set(f.name, v)))
        })
        .map(|(f, _)| f.name)
        .chain(
            // A release before 2020 holds one switch for hidden geometry
            // and hidden objects (§14.5).
            (release < Release::R2020 && ro.hidden_objects != ro.hidden_geometry)
                .then_some("hidden_objects"),
        )
        // Dashes off has no byte before 2019 (§14.5).
        .chain((release < Release::R2019 && ro.dashes == Some(false)).then_some("dashes"))
        .collect()
}

/// Whether a setting no 2017 block byte and no style item holds is set
/// (`corpus/2027`).
fn only_2026_set(name: &str, v: &RoValue) -> bool {
    matches!((name, v), ("profiles_only", RoValue::Bool(true)))
}

/// The display settings block (§10.7) over the default document's, as
/// `release` holds it.
fn rendering(
    ro: Option<&RenderingOptions>,
    shadows_displayed: bool,
    release: Release,
) -> Rendering17 {
    let mut r = rendering_full(ro, shadows_displayed);
    r.fields.0 = release.rendering_of(&r.fields.0);
    r
}

fn rendering_full(ro: Option<&RenderingOptions>, shadows_displayed: bool) -> Rendering17 {
    let mut fields = RENDERING.to_vec();
    if let Some(ro) = ro {
        for (f, (_, v)) in RO_FIELDS.iter().zip(ro.values()) {
            // `hidden_objects` shares its byte with `hidden_geometry`,
            // which is the setting a 2017 file has.
            if f.name == "hidden_objects" {
                continue;
            }
            if let (Some(off), Some(v)) = (f.off17, v) {
                put(&mut fields, off, &ro_bytes(f.kind, stored(f.name, v)));
            }
        }
    }
    if let Some(m) = ro.and_then(|r| r.edges_mode.filter(|_| r.edges)) {
        put(&mut fields, 7, &m.to_le_bytes());
    }
    fields[SHADOWS_DISPLAYED] = shadows_displayed as u8;
    // The bytes releases after 2017 add (§14.5): the section fill colour
    // and switch, the dashes switch of 2019, the hidden-objects switch of
    // 2020.
    let colour = ro
        .and_then(|r| r.section_fill_color)
        .unwrap_or([0xff, 0, 0, 0]);
    fields.extend(colour);
    fields.push(ro.and_then(|r| r.section_fill).unwrap_or(false) as u8);
    fields.push(ro.and_then(|r| r.dashes).unwrap_or(true) as u8);
    fields.push(ro.is_some_and(|r| r.hidden_objects) as u8);
    Rendering17 {
        pre: Pre::default(),
        fields: Blob(fields),
    }
}

fn default_shadows() -> crate::ShadowInfo {
    crate::ShadowInfo {
        time: 1794144600,
        city: "Boulder (CO)".into(),
        country: "USA".into(),
        longitude: -105.242139,
        latitude: 40.018309,
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

/// A shadow record (§10.9).
fn shadow(s: &crate::ShadowInfo) -> Shadow17 {
    let mut tail = [0u8; 14];
    tail[0] = s.north_displayed as u8;
    tail[1] = s.on_faces as u8;
    tail[2] = s.on_ground as u8;
    tail[3] = s.from_edges as u8;
    put(&mut tail, 4, &s.light.to_le_bytes());
    put(&mut tail, 8, &s.dark.to_le_bytes());
    tail[12] = s.use_sun_for_shading as u8;
    Shadow17 {
        pre: Pre::default(),
        time: s.time,
        a: s.daylight_saving as u8,
        city: S16::new(&s.city),
        country: S16::new(&s.country),
        longitude: s.longitude,
        latitude: s.latitude,
        tz_offset: s.tz_offset_h,
        north: s.north,
        tail,
    }
}

/// The axes (§10.10); `shown` is whether a scene displays them, and 0
/// on the model's.
fn axes(a: Option<&crate::Axes>, shown: u8) -> Axes17 {
    let mut out = Axes17 {
        shown,
        ..Default::default()
    };
    if let Some(a) = a {
        out.origin = inches(a.origin_m);
        out.x = a.x;
        out.y = a.y;
        out.z = a.z;
    }
    out
}

fn entry(key: &str, value: AttrVal) -> Entry {
    Entry {
        key: S16::new(key),
        value,
    }
}

/// A dictionary as the 2017 record.
/// A dictionary as the 2017 record. An entry the record cannot hold, an
/// empty key or a value nested deeper than eight arrays, is left out and
/// counted.
fn named(d: &crate::Dictionary, lost: &mut usize) -> AttrNamed {
    AttrNamed {
        name: S16::new(&d.name),
        entries: d
            .entries
            .iter()
            .filter_map(|(k, v)| {
                if k.is_empty() || depth(v) > 8 {
                    *lost += 1;
                    return None;
                }
                Some(entry(k, value17(v)))
            })
            .collect(),
        tail: d.flag,
        ..Default::default()
    }
}

/// How deep a value nests arrays.
fn depth(v: &crate::Value) -> usize {
    match v {
        crate::Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn value17(v: &crate::Value) -> AttrVal {
    use crate::Value;
    match v {
        Value::Nil => AttrVal::Nil,
        Value::Int(x) => AttrVal::I32(*x),
        Value::Float(x) => AttrVal::F32(*x),
        Value::Double(x) => AttrVal::F64(*x),
        Value::Bool(b) => AttrVal::Bool(*b as u8),
        Value::Time(t) => AttrVal::Time(*t),
        Value::Text(s) => AttrVal::Str(S16::new(s)),
        Value::Array(items) => AttrVal::Array(items.iter().map(value17).collect()),
        Value::Bytes8(b) => AttrVal::Raw8(*b),
        Value::Vector(p) => AttrVal::Vec3(*p),
        Value::Direction(p) => AttrVal::Dir3(*p),
    }
}

fn value_of(v: &AttrValue) -> crate::Value {
    match v {
        AttrValue::Int(i) => crate::Value::Int(*i),
        AttrValue::F64(f) => crate::Value::Double(*f),
        AttrValue::Bool(b) => crate::Value::Bool(*b),
    }
}

/// A font (§10.1).
fn font(f: &crate::Font) -> Font17 {
    let mut tail = [0u8; 15];
    tail[0] = f.bold as u8;
    tail[1] = f.italic as u8;
    put(&mut tail, 2, &f.size_pt.to_le_bytes());
    tail[6] = f.model_height as u8;
    put(&mut tail, 7, &inch(f.height_m).to_le_bytes());
    Font17 {
        pre: Pre::default(),
        name: S16::new(&f.family),
        tail,
    }
}

/// The definitions in the order they can be written: each after the
/// definitions it places. Both containers need it: SketchUp's web app drops
/// a 2026 definition that places one written after it, and a 2017 object
/// is written where it is first mentioned. A file SketchUp wrote is already
/// in this order, and keeps it.
pub(crate) fn definition_order(model: &Model) -> Result<Vec<usize>, Error> {
    let n = model.definitions.len();
    let index_of_ref = |defref: u32| -> Option<usize> {
        model
            .definition_links
            .binary_search_by_key(&defref, |l| l.0)
            .ok()
            .map(|i| model.definition_links[i].1)
    };
    let mut needs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, d) in model.definitions.iter().enumerate() {
        let Some(run) = run_of(model, d.map_index) else {
            continue;
        };
        for defref in run
            .placed
            .iter()
            .map(|p| p.defref)
            .chain(run.images.iter().map(|im| im.defref))
        {
            if let Some(j) = index_of_ref(defref) {
                if j != i {
                    needs[i].push(j);
                }
            }
        }
    }
    // The placements in the wrapper of a procedure's result count too.
    for p in &model.features.procedures {
        for r in &p.results {
            if p.definition < n && r.definition < n && r.definition != p.definition {
                needs[p.definition].push(r.definition);
            }
        }
    }
    // Depth-first, iteratively: a definition is placed after all it needs.
    let mut state = vec![0u8; n]; // 0 new, 1 open, 2 done
    let mut order = Vec::with_capacity(n);
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some((i, k)) = stack.pop() {
            if let Some(&j) = needs[i].get(k) {
                stack.push((i, k + 1));
                match state[j] {
                    0 => {
                        state[j] = 1;
                        stack.push((j, 0));
                    }
                    1 => {
                        return Err(Error(format!(
                            "model cannot be written: definitions {:?} and {:?} place each other",
                            model.definitions[i].name, model.definitions[j].name
                        )))
                    }
                    _ => {}
                }
            } else {
                state[i] = 2;
                order.push(i);
            }
        }
    }
    Ok(order)
}

/// The materials of placed images, by index: an image's own material,
/// marked in both containers (§8.1, §16.6).
pub(crate) fn image_materials(model: &Model) -> HashSet<usize> {
    model
        .definitions
        .iter()
        .filter(|d| d.kind == DefinitionKind::Image)
        .filter_map(|d| run_of(model, d.map_index))
        .flat_map(|r| r.mesh.faces.iter())
        .filter_map(|f| f.front_material)
        .filter_map(|slot| {
            model
                .material_links
                .iter()
                .find(|l| l.0 == slot)
                .map(|l| l.1)
        })
        .collect()
}

fn run_of(model: &Model, map_index: Option<usize>) -> Option<&GeometryRun> {
    map_index?;
    model.geometry.iter().find(|r| r.def_index == map_index)
}

/// The Newell normal of a ring of vertices.
fn newell(ring: &[u32], vertices: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0f64; 3];
    for k in 0..ring.len() {
        let a = vertices[ring[k] as usize];
        let b = vertices[ring[(k + 1) % ring.len()] as usize];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

/// The order the edges of a run are written in: the model's, except
/// that the member edges of an arc take the places they hold in the
/// order of their angle from the arc's start. SketchUp takes the start
/// vertex of the first member it reads for the arc's start and turns the
/// frame of an arc whose first member lies elsewhere.
fn arc_order(run: &GeometryRun) -> Vec<usize> {
    let m = &run.mesh;
    let mut order: Vec<usize> = (0..m.edges.len()).collect();
    for (ci, c) in run.curves.iter().enumerate() {
        let Some(arc) = &c.arc else { continue };
        let places: Vec<usize> = (0..m.edges.len())
            .filter(|&i| m.edges[i].curve == Some(ci as u32))
            .collect();
        if places.len() < 2 {
            continue;
        }
        let angle = |v: u32| -> f64 {
            let p = m.vertices[v as usize];
            let d = [
                p[0] - arc.center_m[0],
                p[1] - arc.center_m[1],
                p[2] - arc.center_m[2],
            ];
            let dot = |a: [f64; 3]| d[0] * a[0] + d[1] * a[1] + d[2] * a[2];
            dot(arc.y_axis_m).atan2(dot(arc.x_axis_m))
        };
        let turn = std::f64::consts::TAU;
        // An edge by the angle of its middle, counted from the start.
        // The ends are taken as a pair, so an edge stored against the
        // arc sorts where the same edge stored along it does; the last
        // member of a full circle, whose ends lie either side of the
        // start, has its middle on the far side of them.
        let from_start = |v: u32| (angle(v) - arc.start_rad).rem_euclid(turn);
        let key = |i: usize| -> f64 {
            let (a, b) = (from_start(m.edges[i].v0), from_start(m.edges[i].v1));
            let middle = (a + b) / 2.0;
            if (a - b).abs() > turn / 2.0 + 1e-9 {
                (middle + turn / 2.0).rem_euclid(turn)
            } else {
                middle
            }
        };
        let mut members: Vec<(f64, usize)> = places.iter().map(|&i| (key(i), i)).collect();
        if members.iter().any(|(k, _)| !k.is_finite()) {
            continue;
        }
        members.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (place, (_, i)) in places.iter().zip(members) {
            order[*place] = i;
        }
    }
    order
}

/// Whether a ring turns the same way at every corner, by a clear margin:
/// what the loop's convexity byte says (§6.3). A loop with a straight or
/// reflex corner is written as not convex, the general case.
fn convex(ring: &[u32], vertices: &[[f64; 3]], normal: [f64; 3]) -> bool {
    let n = ring.len();
    let mut sign = 0.0f64;
    for k in 0..n {
        let (a, b, c) = (
            vertices[ring[k] as usize],
            vertices[ring[(k + 1) % n] as usize],
            vertices[ring[(k + 2) % n] as usize],
        );
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let w = [c[0] - b[0], c[1] - b[1], c[2] - b[2]];
        let cross = [
            u[1] * w[2] - u[2] * w[1],
            u[2] * w[0] - u[0] * w[2],
            u[0] * w[1] - u[1] * w[0],
        ];
        let len =
            (u.iter().map(|x| x * x).sum::<f64>() * w.iter().map(|x| x * x).sum::<f64>()).sqrt();
        if len == 0.0 {
            return false;
        }
        let turn = (cross[0] * normal[0] + cross[1] * normal[1] + cross[2] * normal[2]) / len;
        if turn.abs() < 1e-6 || (sign != 0.0 && turn.signum() != sign) {
            return false;
        }
        sign = turn.signum();
    }
    n >= 3
}

impl<'m> Builder<'m> {
    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn guid(&mut self, hex: &str) -> [u8; 16] {
        crate::build26::guid_bytes(hex)
            .unwrap_or_else(|| crate::build26::random_guid(&mut self.seed))
    }

    /// The attribute container of an owner (§10.5): its dictionaries and
    /// `more` (a face's texture placement); null when there is nothing.
    fn container(&mut self, owner: crate::Owner, more: Vec<Ptr>) -> Ptr {
        let dicts = self.dicts.get(&owner).cloned().unwrap_or_default();
        if dicts.is_empty() && more.is_empty() {
            return Ptr::Null;
        }
        // A release without attributes (§14, 3 → 4) holds no container;
        // the notes count what is left out.
        if !self.release.has_attributes() {
            self.lost_dictionaries += dicts.len();
            self.lost_placements += more.len();
            return Ptr::Null;
        }
        let mut lost = 0;
        let mut children: Vec<Ptr> = dicts
            .iter()
            .map(|d| {
                let id = self.id();
                Ptr::new(id, Body::AttrNamed(named(d, &mut lost)))
            })
            .collect();
        if lost > 0 {
            self.notes.push(format!(
                "{lost} dictionary entries with an empty key or arrays nested past eight are left out"
            ));
        }
        children.extend(more);
        let id = self.id();
        Ptr::new(
            id,
            Body::AttrContainer(AttrContainer {
                pre: Pre::default(),
                dictionaries: children,
            }),
        )
    }

    fn material_ptr(&self, slot: u16) -> Ptr {
        match self.material_of_slot.get(&slot) {
            Some(&id) if slot != 0 => Ptr::Ref(id),
            _ => Ptr::Null,
        }
    }

    fn layer_ptr(&self, slot: u16) -> Ptr {
        match self.layer_of_slot.get(&slot) {
            Some(&id) if slot != 0 => Ptr::Ref(id),
            _ => Ptr::Null,
        }
    }

    fn note_pid(&mut self, pid: u32, id: u32, is_root: bool, run: Option<usize>) {
        if pid == 0 {
            return;
        }
        if is_root {
            self.obj_of_pid.insert(pid, id);
        } else {
            self.obj_of_pid.entry(pid).or_insert(id);
        }
        if let Some(t) = run.and_then(|r| self.runs.get_mut(r)) {
            t.entities.entry(pid).or_insert(id);
        }
    }

    /// The object an anchor of an annotation in run `owner` names, and
    /// the instances leading to it from that run: through the placements
    /// `through` names (persistent ids, from that run down) when it names
    /// any, else the run itself first, then the definitions it places,
    /// depth first.
    fn locate(
        &self,
        owner: Option<usize>,
        pid: u32,
        vertex: bool,
        through: &[u32],
    ) -> Option<(u32, Vec<u32>)> {
        let model = self.model;
        let find = |r: usize| -> Option<u32> {
            let t = self.runs.get(r)?;
            let (first, second) = if vertex {
                (&t.vertices, &t.entities)
            } else {
                (&t.entities, &t.vertices)
            };
            first.get(&pid).or_else(|| second.get(&pid)).copied()
        };
        let run_of_ref = |defref: u32| -> Option<usize> {
            let i = model
                .definition_links
                .binary_search_by_key(&defref, |l| l.0)
                .ok()?;
            let map_index = model.definitions[model.definition_links[i].1].map_index?;
            model
                .geometry
                .iter()
                .position(|r| r.def_index == Some(map_index))
        };
        fn dfs(
            b: &Builder,
            r: usize,
            find: &dyn Fn(usize) -> Option<u32>,
            run_of_ref: &dyn Fn(u32) -> Option<usize>,
            path: &mut Vec<u32>,
            depth: usize,
        ) -> Option<u32> {
            if let Some(id) = find(r) {
                return Some(id);
            }
            if depth > 64 {
                return None;
            }
            for p in &b.model.geometry[r].placed {
                let (Some(child), Some(&inst)) = (
                    run_of_ref(p.defref),
                    b.runs.get(r).and_then(|t| t.instances.get(&p.pid)),
                ) else {
                    continue;
                };
                path.push(inst);
                if let Some(id) = dfs(b, child, find, run_of_ref, path, depth + 1) {
                    return Some(id);
                }
                path.pop();
            }
            None
        }
        // A definition placed more than once holds the entity once per
        // placement; the path tells which of them the anchor is on. A
        // path the file left short (a step it does not resolve) is
        // followed as far as it goes, and the search continues below
        // the placement it reached rather than from the owner.
        let r = owner?;
        let mut path = Vec::with_capacity(through.len());
        let mut at = r;
        for &step in through {
            let Some(p) = model
                .geometry
                .get(at)
                .and_then(|g| g.placed.iter().find(|p| p.pid == step))
            else {
                break;
            };
            let Some(&inst) = self.runs.get(at).and_then(|t| t.instances.get(&step)) else {
                break;
            };
            let Some(child) = run_of_ref(p.defref) else {
                break;
            };
            path.push(inst);
            at = child;
        }
        if let Some(id) = find(at) {
            return Some((id, path));
        }
        if let Some(id) = dfs(self, at, &find, &run_of_ref, &mut path, 0) {
            return Some((id, path));
        }
        let mut path = Vec::new();
        let id = dfs(self, r, &find, &run_of_ref, &mut path, 0)?;
        Some((id, path))
    }

    /// An image object, written once per distinct content: a material's
    /// texture, which materials share (§8.1).
    fn dib(&mut self, bytes: &[u8], quality: Option<u32>) -> Ptr {
        if let Some(&id) = self.dib_of_image.get(bytes) {
            return Ptr::Ref(id);
        }
        let id = self.id();
        self.dib_of_image.insert(bytes.to_vec(), id);
        self.own_dib(id, bytes, quality)
    }

    /// An image object of its own, shared with nothing: a preview, a
    /// stroke or a photo. SketchUp frees such an image with its owner,
    /// and one owned twice is freed twice.
    fn dib_alone(&mut self, bytes: &[u8]) -> Ptr {
        let id = self.id();
        self.own_dib(id, bytes, None)
    }

    fn own_dib(&mut self, id: u32, bytes: &[u8], quality: Option<u32>) -> Ptr {
        let subtype = crate::xml26::image_type(bytes);
        Ptr::new(
            id,
            Body::Dib(Dib {
                subtype,
                data: Blob(bytes.to_vec()),
                quality: if subtype == 1 {
                    quality.unwrap_or(70)
                } else {
                    0
                },
            }),
        )
    }

    /// A fresh image object that is not shared (thumbnails).
    fn png(&mut self, w: u32, h: u32) -> Ptr {
        let id = self.id();
        Ptr::new(
            id,
            Body::Dib(Dib {
                subtype: 4,
                data: Blob(crate::xml26::flat_png(w, h, [0xF0, 0xF0, 0xF0])),
                quality: 0,
            }),
        )
    }

    /// A dimension's font, where the release holds one (§14,
    /// [`Release::dimension_has_font`]).
    fn dimension_font(&mut self, index: Option<usize>) -> Ptr {
        if self.release.dimension_has_font() {
            self.font_ptr(index)
        } else {
            Ptr::Null
        }
    }

    fn font_ptr(&mut self, index: Option<usize>) -> Ptr {
        let i = index.filter(|&i| i < self.fonts.len()).unwrap_or(0);
        self.fonts[i].ptr()
    }

    fn materials(&mut self) -> Vec<Ptr> {
        let model = self.model;
        let image_materials = image_materials(model);
        let mut ids = Vec::with_capacity(model.materials.len());
        let mut out = Vec::with_capacity(model.materials.len());
        for (index, mat) in model.materials.iter().enumerate() {
            let id = self.id();
            ids.push(id);
            let mut m = Material17 {
                opacity: 1.0,
                ..Default::default()
            };
            let (name, opacity) = match mat {
                Material::Solid {
                    name,
                    rgba,
                    opacity,
                    colorize_type,
                } => {
                    // The colour's alpha byte carries the opacity too,
                    // truncated (§8.1).
                    m.rgba = [rgba[0], rgba[1], rgba[2], alpha_byte(*opacity)];
                    m.colorize = *colorize_type;
                    (name, *opacity)
                }
                Material::Textured {
                    name,
                    texture,
                    applied_size_in,
                    jpeg_quality,
                    image_bytes,
                    avg_rgba,
                    colorized,
                    texture_avg_rgba,
                    colorize_type,
                    opacity,
                } => {
                    m.colorize = *colorize_type;
                    let avg = avg_rgba.unwrap_or([128, 128, 128, 255]);
                    let image_avg = texture_avg_rgba.unwrap_or(avg);
                    match image_bytes {
                        Some(bytes) => {
                            let size = applied_size_in.unwrap_or((10.0, 10.0));
                            // Before release 6 the image is held in the
                            // texture, without a quality (§14).
                            let (image, embedded) = if self.release.texture_image_object() {
                                (self.dib(bytes, *jpeg_quality), None)
                            } else {
                                let subtype = crate::xml26::image_type(bytes);
                                let dib = Dib {
                                    subtype,
                                    data: Blob(bytes.to_vec()),
                                    quality: 0,
                                };
                                (Ptr::Null, Some(dib))
                            };
                            m.texture = Some(Texture17 {
                                attrs: Ptr::Null,
                                b: 0,
                                image,
                                embedded,
                                size_in: [size.0, size.1],
                                file_name: S16::new(texture.as_deref().unwrap_or("")),
                                color: avg,
                                c: image_materials.contains(&index) as u8,
                            });
                            m.kind = if *colorized { 2 } else { 1 };
                        }
                        None => self.notes.push(format!(
                            "material {name:?} has no image and is written as its average colour"
                        )),
                    }
                    // A colourised texture holds its tint in both colours
                    // (SketchUp takes the second for the material's); any
                    // other holds the image's average there.
                    let second = if *colorized { avg } else { image_avg };
                    m.rgba = [second[0], second[1], second[2], 0xFF];
                    (name, *opacity)
                }
            };
            m.name = S16::new(name);
            m.pre.attrs = self.container(crate::Owner::Material(index), Vec::new());
            // The record stores the transparency; the flag says whether
            // it applies.
            m.use_opacity = (opacity < 1.0) as u8;
            m.opacity = 1.0 - opacity.clamp(0.0, 1.0);
            out.push(Ptr::new(id, Body::Material(Box::new(m))));
        }
        for (slot, index) in &model.material_links {
            if let Some(&id) = ids.get(*index) {
                self.material_of_slot.insert(*slot, id);
            }
        }
        out
    }

    fn layer(&mut self, l: &crate::Layer) -> Layer17 {
        Layer17 {
            pre: Pre::default(),
            name: S16::new(&l.name),
            hidden: (!l.visible) as u8,
            zero: 0,
            pid2: 0,
            internal: S16::new(&format!("Layer_{}", l.name)),
            two: [0, 1],
            rgba: l.rgba,
            empty: S16::default(),
            kind: 0,
            colorize: 0,
            // Unused while the flag is clear; SketchUp stores 0.5 on the
            // layers a user adds.
            opacity: if l.name == "Layer0" { 0.0 } else { 0.5 },
            use_opacity: 0,
            tail: 0,
            style: Ptr::Null,
        }
    }

    /// The twelve stock dash patterns, each a new object.
    fn stock_styles(&mut self) -> Vec<Ptr> {
        LINE_STYLES
            .iter()
            .enumerate()
            .map(|(k, (name, pattern))| {
                let pid = self.pids.fresh();
                let id = self.id();
                let attrs = self.container(crate::Owner::LineStyle(k), Vec::new());
                Ptr::new(
                    id,
                    Body::LineStyle(Box::new(crate::file17::LineStyle17 {
                        pre: Pre { attrs, pid },
                        zero: 0,
                        pid2: pid,
                        name: S16::new(name),
                        pattern: S16::new(pattern),
                        scales: [1.0; 3],
                    })),
                )
            })
            .collect()
    }

    /// A layer folder with the folders and layers in it (2020, §14);
    /// `next` is its place in pre-order, which names its dictionaries.
    fn layer_folder(&mut self, g: &crate::LayerFolder, next: &mut usize) -> Ptr {
        let pid = self.pids.fresh();
        let id = self.id();
        let attrs = self.container(crate::Owner::LayerFolder(*next), Vec::new());
        *next += 1;
        let children = g
            .folders
            .iter()
            .map(|c| self.layer_folder(c, next))
            .collect();
        let layers = g
            .layers
            .iter()
            .filter_map(|&i| self.layer_ids.get(i).map(|&id| Ptr::Ref(id)))
            .collect();
        Ptr::new(
            id,
            Body::LayerGroup(Box::new(crate::file17::LayerGroup17 {
                pre: Pre { attrs, pid },
                name: S16::new(&g.name),
                children,
                layers,
                hidden: g.hidden as u8,
            })),
        )
    }

    fn layers(&mut self) -> Vec<Ptr> {
        let model = self.model;
        let mut layers = model.layers.clone();
        if layers.is_empty() {
            layers.push(default_layer());
        }
        let mut out = Vec::with_capacity(layers.len());
        if self.release.has_line_styles() {
            self.stock_line_styles = self.stock_styles();
            // Only the twelve stock styles are written; the dictionaries
            // of any other line style are left out and counted.
            let extra: usize = self
                .dicts
                .iter()
                .filter(|(o, _)| matches!(o, crate::Owner::LineStyle(k) if *k >= LINE_STYLES.len()))
                .map(|(_, d)| d.len())
                .sum();
            self.lost_dictionaries += extra;
        }
        for (index, l) in layers.iter().enumerate() {
            let id = self.id();
            self.layer_ids.push(id);
            let mut body = self.layer(l);
            if self.release.layer_has_words() {
                if let Some(&(_, v)) = model
                    .features
                    .layer_page_behaviour
                    .iter()
                    .find(|(li, _)| *li == index)
                {
                    body.tail = v;
                }
            }
            if self.release.layer_has_style() {
                if let Some(&(_, s)) = model
                    .features
                    .layer_line_styles
                    .iter()
                    .find(|(li, _)| *li == index)
                {
                    if let Some(slot) = self.stock_line_styles.get_mut(s) {
                        // The first user holds the object, the rest and
                        // the style list refer back to it.
                        let sid = slot.id().unwrap_or(0);
                        body.style = std::mem::replace(slot, Ptr::Ref(sid));
                    }
                }
            }
            body.pre.attrs = self.container(crate::Owner::Layer(index), Vec::new());
            out.push(Ptr::new(id, Body::Layer(Box::new(body))));
        }
        for (slot, index) in &model.layer_links {
            if *index != 0 {
                if let Some(&id) = self.layer_ids.get(*index) {
                    self.layer_of_slot.insert(*slot, id);
                }
            }
        }
        out
    }

    fn drawbase(&self, material: u16, hidden: bool, layer: u16) -> Drawbase {
        Drawbase {
            material: self.material_ptr(material),
            hidden: hidden as u8,
            layer: self.layer_ptr(layer),
            ..Default::default()
        }
    }

    /// The face's texture placement, a child of its attribute container
    /// (§9).
    fn face_texture(&mut self, t: &crate::FaceTexture) -> Ptr {
        let mut k = [0.0f64; 24];
        k[..9].copy_from_slice(&t.front);
        k[9..12].copy_from_slice(&t.front_extra);
        k[12..21].copy_from_slice(&t.back);
        k[21..].copy_from_slice(&t.back_extra);
        let ftc = Ftc17 {
            pre: Pre::default(),
            zero: 0,
            k,
            front_pins: t.front_pins.iter().take(4).copied().collect(),
            back_pins: t.back_pins.iter().take(4).copied().collect(),
            flags: t.flags,
        };
        let child = self.id();
        Ptr::new(child, Body::Ftc(Box::new(ftc)))
    }

    /// An anchor (§10.1) of an annotation in run `owner`; `None` when it
    /// names an entity the file does not hold.
    fn anchor(&mut self, a: &crate::Anchor, owner: Option<usize>) -> Option<Anchor17> {
        let located = a
            .entity
            .and_then(|p| self.locate(owner, p, a.kind == 2, &a.path));
        // An anchor may name no entity and hold a point alone (a
        // dimension whose ends are free), and an anchor of kind 0 is a
        // point whatever it names. Any other needs the entity it names,
        // and a vertex anchor, which stores no point, its vertex.
        let needs_entity = a.kind == 2 || (a.kind != 0 && a.entity.is_some());
        if located.is_none() && needs_entity {
            return None;
        }
        let (resolved, path) = match located {
            Some((id, path)) => (Some(id), path),
            None => (None, Vec::new()),
        };
        // Where two edges meet (kind 9), the second edge too: SketchUp does
        // not open a file holding such an anchor without it.
        let second = a
            .second
            .and_then(|p| self.locate(owner, p, false, &a.second_path));
        if a.kind == 9 && second.is_none() {
            return None;
        }
        let (second, second_path) = match second {
            Some((id, path)) => (Ptr::Ref(id), path),
            None => (Ptr::Null, Vec::new()),
        };
        // A vertex anchor stores no point: the vertex gives it.
        let point = match a.kind {
            2 => [0.0; 3],
            5 => [a.point_m[0], inch(a.point_m[1]), inch(a.point_m[2])],
            _ => inches(a.point_m),
        };
        Some(Anchor17 {
            kind: a.kind,
            four: 4,
            point,
            entity: resolved.map(Ptr::Ref).unwrap_or(Ptr::Null),
            second,
            path: path.into_iter().map(Ptr::Ref).collect(),
            second_path: second_path.into_iter().map(Ptr::Ref).collect(),
        })
    }

    /// The entity list of one run, and its active section plane.
    /// A run's relationships (§7.1), naming entities the run's list
    /// already holds; one whose entity the run lacks is left out.
    fn relationships(&mut self, run: &GeometryRun) -> Vec<Ptr> {
        let ri = self
            .model
            .geometry
            .iter()
            .position(|r| std::ptr::eq(r, run));
        let mut out = Vec::new();
        for [a, b] in &run.relationships {
            let id = |p: &u32| {
                ri.and_then(|r| self.runs.get(r))
                    .and_then(|t| t.entities.get(p).copied())
            };
            match (id(a), id(b)) {
                (Some(a), Some(b)) => {
                    let body = Body::Relationship(Relationship17 {
                        pre: Pre::default(),
                        a: Ptr::Ref(a),
                        b: Ptr::Ref(b),
                    });
                    out.push(Ptr::new(self.id(), body));
                }
                _ => self.notes.push(format!(
                    "a relationship names entities {a} and {b}, which its list does not hold, and is left out"
                )),
            }
        }
        out
    }

    fn entities(&mut self, run: &GeometryRun, is_root: bool) -> (Vec<Ptr>, Ptr) {
        let model = self.model;
        let m = &run.mesh;
        let mut out: Vec<Ptr> = Vec::new();
        let ri = model.geometry.iter().position(|r| std::ptr::eq(r, run));

        // Curves, written inside their first member edge.
        let mut members = vec![0u32; run.curves.len()];
        for e in &m.edges {
            if let Some(n) = e.curve.and_then(|c| members.get_mut(c as usize)) {
                *n += 1;
            }
        }
        let mut curves: Vec<Once> = Vec::with_capacity(run.curves.len());
        for (ci, (c, n)) in run.curves.iter().zip(&members).enumerate() {
            let pre = Pre {
                attrs: match ri {
                    Some(r) => self.container(crate::Owner::Curve(r, ci), Vec::new()),
                    None => Ptr::Null,
                },
                pid: self.pids.keep(c.pid),
            };
            let count = if *n == 0 { c.members } else { *n };
            let body = match &c.arc {
                None => Body::Curve(Curve17 {
                    pre,
                    ends: Default::default(),
                    b: 0,
                    members: count,
                }),
                Some(a) => {
                    let p = arc_params(a);
                    let mut head = [0u8; 5];
                    put(&mut head, 1, &count.to_le_bytes());
                    Body::ArcCurve(Box::new(ArcCurve17 {
                        pre,
                        ends: Default::default(),
                        head,
                        params: p,
                    }))
                }
            };
            let id = self.id();
            // A dimension can anchor on a curve (an arc's center).
            self.note_pid(c.pid, id, is_root, ri);
            curves.push(Once {
                id,
                body: Some(body),
            });
        }

        // Edges, each holding its vertices where they first appear.
        let mut vertex_ids: Vec<Option<u32>> = vec![None; m.vertices.len()];
        let mut edge_ids = Vec::with_capacity(m.edges.len());
        let mut edge_of_ends: HashMap<(u32, u32), usize> = HashMap::new();
        for (i, e) in m.edges.iter().enumerate() {
            edge_of_ends
                .entry((e.v0.min(e.v1), e.v0.max(e.v1)))
                .or_insert(i);
            edge_ids.push(self.id());
        }
        for i in arc_order(run) {
            let e = &m.edges[i];
            let id = edge_ids[i];
            let pid = self.pids.keep(e.pid);
            self.note_pid(e.pid, id, is_root, ri);
            let mut vertex = |b: &mut Builder, v: u32| -> Ptr {
                if let Some(id) = vertex_ids[v as usize] {
                    return Ptr::Ref(id);
                }
                let id = b.id();
                vertex_ids[v as usize] = Some(id);
                let model_pid = m.vertex_pids.get(v as usize).copied().unwrap_or(0);
                if model_pid != 0 {
                    if let Some(t) = ri.and_then(|r| b.runs.get_mut(r)) {
                        t.vertices.entry(model_pid).or_insert(id);
                    }
                }
                Ptr::new(
                    id,
                    Body::Vertex(Vertex17 {
                        pre: Pre {
                            attrs: match ri {
                                Some(r) => {
                                    b.container(crate::Owner::Vertex(r, v as usize), Vec::new())
                                }
                                None => Ptr::Null,
                            },
                            pid: b.pids.keep(model_pid),
                        },
                        p: m.inches_of(v as usize, inch_v),
                    }),
                )
            };
            let v0 = vertex(self, e.v0);
            let v1 = vertex(self, e.v1);
            let curve = match e.curve.and_then(|c| curves.get_mut(c as usize)) {
                Some(c) => c.ptr(),
                None => Ptr::Null,
            };
            let mut drawbase = self.drawbase(e.material.unwrap_or(0), e.hidden, e.layer);
            drawbase.soft = e.soft as u8;
            drawbase.smooth = e.smooth as u8;
            drawbase.casts_shadows = e.casts_shadows as u8;
            drawbase.receives_shadows = e.receives_shadows as u8;
            out.push(Ptr::new(
                id,
                Body::Edge(Box::new(Edge17 {
                    pre: Pre {
                        attrs: match ri {
                            Some(r) => self.container(crate::Owner::Edge(r, i), Vec::new()),
                            None => Ptr::Null,
                        },
                        pid,
                    },
                    drawbase,
                    v0,
                    v1,
                    curve,
                })),
            ));
        }

        // Faces: loops of edge uses. A use runs along its edge when its
        // direction byte is 0 and against it when 1; the uses follow the
        // loop, counter-clockwise about the normal on the outer loop and
        // clockwise on a hole.
        for (fi, f) in m.faces.iter().enumerate() {
            let id = self.id();
            let pid = self.pids.keep(f.pid);
            self.note_pid(f.pid, id, is_root, ri);
            let ftc = match &f.texture {
                Some(t) => vec![self.face_texture(t)],
                None => Vec::new(),
            };
            let attrs = match ri {
                Some(r) => self.container(crate::Owner::Face(r, fi), ftc),
                None => {
                    let id = self.id();
                    match ftc.is_empty() {
                        true => Ptr::Null,
                        false => Ptr::new(
                            id,
                            Body::AttrContainer(AttrContainer {
                                pre: Pre::default(),
                                dictionaries: ftc,
                            }),
                        ),
                    }
                }
            };
            let n = f.normal;
            let d = crate::build26::plane_offset(f, m, inch_v);
            let mut loops = Vec::with_capacity(1 + f.holes.len());
            for (k, ring) in std::iter::once(&f.outer).chain(&f.holes).enumerate() {
                let mut ring = ring.clone();
                let w = newell(&ring, &m.vertices);
                let ccw = w[0] * n[0] + w[1] * n[1] + w[2] * n[2] > 0.0;
                if ccw != (k == 0) {
                    ring.reverse();
                }
                let loop_id = self.id();
                let mut uses = Vec::with_capacity(ring.len());
                for j in 0..ring.len() {
                    let (a, b) = (ring[j], ring[(j + 1) % ring.len()]);
                    // `validate` has checked that the edge exists.
                    let Some(&ei) = edge_of_ends.get(&(a.min(b), a.max(b))) else {
                        continue;
                    };
                    uses.push(Ptr::new(
                        self.id(),
                        Body::EdgeUse(EdgeUse17 {
                            pre: Pre::default(),
                            edge: Ptr::Ref(edge_ids[ei]),
                            dir: (m.edges[ei].v0 != a) as u8,
                            parent: Ptr::Ref(loop_id),
                        }),
                    ));
                }
                loops.push(Ptr::new(
                    loop_id,
                    Body::Loop(Loop17 {
                        pre: Pre::default(),
                        c: [(k == 0) as u8, convex(&ring, &m.vertices, n) as u8],
                        uses,
                    }),
                ));
            }
            out.push(Ptr::new(
                id,
                Body::Face(Box::new(Face17 {
                    pre: Pre { attrs, pid },
                    drawbase: Drawbase {
                        casts_shadows: f.casts_shadows as u8,
                        receives_shadows: f.receives_shadows as u8,
                        ..self.drawbase(f.front_material.unwrap_or(0), f.hidden, f.layer)
                    },
                    plane: [n[0], n[1], n[2], d],
                    loops,
                    back_material: self.material_ptr(f.back_material.unwrap_or(0)),
                })),
            ));
        }

        // Instances: components, groups, images.
        // (class, pid, defref, transform, material, hidden, layer, the
        // placement's own flags and GUID)
        let placements = run
            .placed
            .iter()
            .map(|p| {
                let class = if p.is_group {
                    InstanceClass::Group
                } else {
                    InstanceClass::Component
                };
                (
                    class,
                    p.pid,
                    p.defref,
                    p.transform,
                    p.material,
                    p.hidden,
                    p.layer,
                    Some((
                        p.casts_shadows,
                        p.receives_shadows,
                        p.locked,
                        p.guid.as_str(),
                    )),
                )
            })
            .chain(run.images.iter().map(|im| {
                (
                    InstanceClass::Image,
                    im.pid,
                    im.defref,
                    im.transform,
                    0,
                    im.hidden,
                    im.layer,
                    Some((im.casts_shadows, im.receives_shadows, false, "")),
                )
            }));
        for (k, (class, model_pid, defref, transform, material, hidden, layer, own)) in
            placements.enumerate()
        {
            let def = model
                .definition_links
                .binary_search_by_key(&defref, |l| l.0)
                .ok()
                .and_then(|i| {
                    self.def_ids
                        .get(model.definition_links[i].1)
                        .copied()
                        .flatten()
                });
            let Some(def) = def else {
                self.notes.push(format!(
                    "instance {model_pid} places a definition that is not written and is left out"
                ));
                continue;
            };
            let id = self.id();
            let pid = self.pids.keep(model_pid);
            self.note_pid(model_pid, id, is_root, ri);
            if model_pid != 0 {
                if let Some(t) = ri.and_then(|r| self.runs.get_mut(r)) {
                    t.instances.entry(model_pid).or_insert(id);
                }
            }
            let mut drawbase = self.drawbase(material, hidden, layer);
            if class == InstanceClass::Image {
                drawbase.casts_shadows = 0;
            }
            if let Some((casts, receives, locked, _)) = own {
                drawbase.casts_shadows = casts as u8;
                drawbase.receives_shadows = receives as u8;
                drawbase.locked = locked as u8;
            }
            let guid = own
                .and_then(|(_, _, _, g)| crate::build26::guid_bytes(g))
                .unwrap_or_else(|| crate::build26::random_guid(&mut self.seed));
            out.push(Ptr::new(
                id,
                Body::Instance(Box::new(Instance17 {
                    class,
                    pre: Pre {
                        attrs: match ri {
                            Some(r) if k < run.placed.len() => {
                                self.container(crate::Owner::Instance(r, k), Vec::new())
                            }
                            Some(r) => self.container(
                                crate::Owner::Image(r, k - run.placed.len()),
                                Vec::new(),
                            ),
                            None => Ptr::Null,
                        },
                        pid,
                    },
                    drawbase,
                    definition: Ptr::Ref(def),
                    transform,
                    // A release without instance names (§14, 4 → 5)
                    // holds none.
                    name: S16::new(
                        self.instance_names
                            .get(&model_pid)
                            .filter(|_| self.release.instance_has_name())
                            .map(String::as_str)
                            .unwrap_or(""),
                    ),
                    guid,
                })),
            ));
        }

        // Annotations belong to the run whose definition index they carry.
        let owns = |def_index: Option<usize>| {
            (is_root || run.def_index.is_some()) && def_index == run.def_index
        };
        for (gi, g) in model
            .guides
            .iter()
            .enumerate()
            .filter(|(_, g)| owns(g.def_index))
        {
            let id = self.id();
            let pid = self.pids.keep(g.pid);
            self.note_pid(g.pid, id, is_root, ri);
            let mut params = [0.0f64; 8];
            params[..3].copy_from_slice(&inches(g.point_m));
            params[3..6].copy_from_slice(&g.direction);
            params[6..].copy_from_slice(&g.bounds);
            out.push(Ptr::new(
                id,
                Body::GuideLine(Box::new(GuideLine17 {
                    pre: Pre {
                        attrs: self.container(crate::Owner::Guide(gi), Vec::new()),
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: g.casts_shadows as u8,
                        receives_shadows: g.receives_shadows as u8,
                        ..self.drawbase(g.material.unwrap_or(0), g.hidden, g.layer)
                    },
                    params,
                    tail: if self.release >= Release::R2018 {
                        [g.stipple.to_le_bytes()[0], g.stipple.to_le_bytes()[1], 0, 0]
                    } else {
                        [0; 4]
                    },
                })),
            ));
        }
        for (k, g) in run.guide_points.iter().enumerate() {
            let id = self.id();
            let pid = self.pids.keep(g.pid);
            self.note_pid(g.pid, id, is_root, ri);
            out.push(Ptr::new(
                id,
                Body::GuidePoint(Box::new(GuidePoint17 {
                    pre: Pre {
                        attrs: match ri {
                            Some(r) => self.container(crate::Owner::GuidePoint(r, k), Vec::new()),
                            None => Ptr::Null,
                        },
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: g.casts_shadows as u8,
                        receives_shadows: g.receives_shadows as u8,
                        ..self.drawbase(g.material.unwrap_or(0), g.hidden, g.layer)
                    },
                    point: inches(g.point_m),
                    anchor: inches(g.anchor_m),
                    b: g.flag,
                })),
            ));
        }
        for (k, p) in run.polylines.iter().enumerate() {
            let id = self.id();
            let pid = self.pids.keep(p.pid);
            self.note_pid(p.pid, id, is_root, ri);
            out.push(Ptr::new(
                id,
                Body::Polyline(Box::new(Polyline17 {
                    pre: Pre {
                        attrs: match ri {
                            Some(r) => self.container(crate::Owner::Polyline(r, k), Vec::new()),
                            None => Ptr::Null,
                        },
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: p.casts_shadows as u8,
                        receives_shadows: p.receives_shadows as u8,
                        ..self.drawbase(p.material.unwrap_or(0), p.hidden, p.layer)
                    },
                    points: p.points_m.iter().map(|q| inches(*q)).collect(),
                })),
            ));
        }
        let mut active = Ptr::Null;
        for (k, s) in run.sections.iter().enumerate() {
            let id = self.id();
            let pid = self.pids.keep(s.pid);
            self.note_pid(s.pid, id, is_root, ri);
            if s.pid != 0 && run.active_section == Some(s.pid) {
                active = Ptr::Ref(id);
            }
            out.push(Ptr::new(
                id,
                Body::SectionPlane(Box::new(SectionPlane17 {
                    pre: Pre {
                        attrs: match ri {
                            Some(r) => self.container(crate::Owner::SectionPlane(r, k), Vec::new()),
                            None => Ptr::Null,
                        },
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: s.casts_shadows as u8,
                        receives_shadows: s.receives_shadows as u8,
                        ..self.drawbase(s.material.unwrap_or(0), s.hidden, s.layer)
                    },
                    plane: s.plane,
                    names: if self.release.section_has_names() {
                        [S16::new(&s.name), S16::new(&s.symbol)]
                    } else {
                        Default::default()
                    },
                })),
            ));
        }
        for (xi, x) in model
            .texts
            .iter()
            .enumerate()
            .filter(|(_, x)| owns(x.def_index))
        {
            let Some(anchor) = self.anchor(&x.anchor, ri) else {
                self.notes.push(format!(
                    "text {:?} is anchored to an entity the file does not hold and is left out",
                    x.content
                ));
                continue;
            };
            let id = self.id();
            let pid = self.pids.keep(x.pid);
            self.note_pid(x.pid, id, is_root, ri);
            let leader = match x.leader {
                crate::Leader::None => 0,
                crate::Leader::ViewBased => 1,
                crate::Leader::Pushpin => 2,
            };
            let leader_offset = if x.leader == crate::Leader::Pushpin {
                inches(x.leader_offset)
            } else {
                x.leader_offset
            };
            out.push(Ptr::new(
                id,
                Body::Text(Box::new(Text17 {
                    pre: Pre {
                        attrs: self.container(crate::Owner::Text(xi), Vec::new()),
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: x.casts_shadows as u8,
                        receives_shadows: x.receives_shadows as u8,
                        ..self.drawbase(x.material.unwrap_or(0), x.hidden, x.layer)
                    },
                    font: self.font_ptr(x.font),
                    screen: x.screen_position,
                    anchor,
                    leader_offset,
                    view_direction: x.view_direction,
                    leader,
                    one: 1,
                    a: 1,
                    b: 0,
                    arrow: x.arrow,
                    leader_shown: !x.leader_hidden as u8,
                    content: S16::new(&x.content),
                    tail: [0, x.leader_hidden as u8, 0, 0, 0],
                })),
            ));
        }
        for (di, d) in model
            .dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| owns(d.def_index))
        {
            let (Some(start), Some(end)) = (self.anchor(&d.start, ri), self.anchor(&d.end, ri))
            else {
                self.notes.push(format!(
                    "dimension {} is anchored to an entity the file does not hold and is left out",
                    d.pid
                ));
                continue;
            };
            let id = self.id();
            let pid = self.pids.keep(d.pid);
            self.note_pid(d.pid, id, is_root, ri);
            out.push(Ptr::new(
                id,
                Body::Dimension(Box::new(Dimension17 {
                    pre: Pre {
                        attrs: self.container(crate::Owner::Dimension(di), Vec::new()),
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: d.casts_shadows as u8,
                        receives_shadows: d.receives_shadows as u8,
                        ..self.drawbase(d.material.unwrap_or(0), d.hidden, d.layer)
                    },
                    text: S16::new(&d.text_override),
                    font: self.dimension_font(d.font),
                    aligned: d.aligned as u8,
                    arrow: d.arrow,
                    start,
                    end,
                    normal: d.normal,
                    x_axis: d.x_axis,
                    u: d.variant,
                    offset: inch(d.offset_m),
                    zero: d.undecoded,
                    text_position: d.text_position,
                })),
            ));
        }
        for (di, d) in model
            .radial_dimensions
            .iter()
            .enumerate()
            .filter(|(_, d)| owns(d.def_index))
        {
            // The measured edge, in the same entity list; without one,
            // the measured arc itself (§10.12).
            let edge = d
                .edge
                .and_then(|p| self.locate(ri, p, false, &[]))
                .filter(|(_, path)| path.is_empty());
            let (edge, arc) = match (edge, &d.arc) {
                (Some((edge, _)), _) => (Ptr::Ref(edge), None),
                (None, Some(a)) if d.edge.is_none() => (Ptr::Null, Some(arc_params(a))),
                (None, None) if d.edge.is_none() => {
                    self.notes.push(format!(
                        "radial dimension {} names no edge and holds no arc and is left out",
                        d.pid
                    ));
                    continue;
                }
                _ => {
                    self.notes.push(format!(
                        "radial dimension {} measures an edge its entity list does not hold and is left out",
                        d.pid
                    ));
                    continue;
                }
            };
            let id = self.id();
            let pid = self.pids.keep(d.pid);
            self.note_pid(d.pid, id, is_root, ri);
            out.push(Ptr::new(
                id,
                Body::RadialDimension(Box::new(RadialDimension17 {
                    pre: Pre {
                        attrs: self.container(crate::Owner::RadialDimension(di), Vec::new()),
                        pid,
                    },
                    drawbase: Drawbase {
                        casts_shadows: d.casts_shadows as u8,
                        receives_shadows: d.receives_shadows as u8,
                        ..self.drawbase(d.material.unwrap_or(0), d.hidden, d.layer)
                    },
                    text: S16::new(&d.text_override),
                    font: self.dimension_font(d.font),
                    aligned: d.aligned as u8,
                    arrow: d.arrow,
                    edge,
                    leader: d.leader,
                    diameter: d.diameter as u8,
                    arc,
                })),
            ));
        }
        (out, active)
    }

    /// A definition (§7.1), with its own copy of the default layer.
    fn definition(&mut self, index: usize) -> Ptr {
        let model = self.model;
        let d = &model.definitions[index];
        let id = self.id();
        let layer_id = self.id();
        let layer = self.layer(model.layers.first().unwrap_or(&default_layer()));
        let empty = empty_run(d.map_index);
        let run = run_of(model, d.map_index).unwrap_or(&empty);
        let (entities, active_section) = self.entities(run, false);
        let relationships = self.relationships(run);
        let mut mid = [0u8; 43];
        mid[0] = d.flag as u8;
        for (k, v) in inches(d.point_m).iter().enumerate() {
            put(&mut mid, 1 + 8 * k, &v.to_le_bytes());
        }
        let b = &d.behaviour;
        mid[28] = b.glues_to_surface as u8;
        mid[29] = b.cuts_opening as u8;
        put(&mut mid, 30, &b.glue_plane.to_le_bytes());
        mid[34] = (b.always_faces_camera as u8) | ((b.shadows_face_sun as u8) << 1);
        put(&mut mid, 35, &b.extra.to_le_bytes());
        put(
            &mut mid,
            39,
            &match d.kind {
                DefinitionKind::Component => 0u32,
                DefinitionKind::Group => 1,
                DefinitionKind::Image => 2,
            }
            .to_le_bytes(),
        );
        // The preview the model carries; else no image, which every
        // group's definition and some components' store, and the camera
        // such definitions hold.
        let (thumb, cam) = (self.id(), self.id());
        let image = match &d.thumbnail {
            Some(bytes) => self.dib_alone(bytes),
            None => Ptr::Null,
        };
        let thumb_camera = d
            .thumbnail_camera
            .as_ref()
            .map(camera)
            .unwrap_or_else(empty_camera);
        let attrs = self.container(crate::Owner::Definition(index), Vec::new());
        let body = Def17 {
            space: Space {
                pre: Pre { attrs, pid: 0 },
                layers: vec![Ptr::new(layer_id, Body::Layer(Box::new(layer)))],
                active_layer: Ptr::Ref(layer_id),
                entities,
                relationships,
                active_section,
                ..Default::default()
            },
            guid: self.guid(&d.guid),
            name: S16::new(&d.name),
            description: S16::new(&d.description),
            source: S16::new(&d.source_path),
            timestamp: d.timestamp,
            mid,
            thumbnail: Ptr::new(
                thumb,
                Body::Thumbnail(Thumbnail {
                    pre: Pre::default(),
                    camera: Ptr::new(cam, Body::Camera(Box::new(thumb_camera))),
                    image,
                }),
            ),
        };
        self.def_ids[index] = Some(id);
        Ptr::new(id, Body::Def(Box::new(body)))
    }

    /// A style (§10.8): the settings a style holds over the default
    /// style's, and its watermarks.
    fn style(
        &mut self,
        s: &crate::Style,
        settings: &RenderingOptions,
        watermarks: &[crate::Watermark],
        written: &mut HashMap<WatermarkKey, u32>,
    ) -> (Style17, Vec<Ptr>) {
        let list = self.watermark_list(watermarks, written);
        let refs = list
            .iter()
            .map(|p| p.id().map(Ptr::Ref).unwrap_or(Ptr::Null))
            .collect();
        let mut items = Vec::with_capacity(STYLE_ITEMS.len());
        let values = settings.values();
        let mut list = Some(list);
        for (id, default) in STYLE_ITEMS {
            let set = RO_FIELDS
                .iter()
                .zip(&values)
                .find(|(f, _)| f.item == Some(*id) && f.name != "hidden_objects")
                .and_then(|(f, (_, v))| v.map(|v| stored(f.name, v)));
            let values = match (*default, set) {
                (StyleDefault::Watermarks, _) => list
                    .take()
                    .unwrap_or_default()
                    .into_iter()
                    .map(StyleVal::Ptr)
                    .collect(),
                (StyleDefault::U8(_), Some(v)) => vec![StyleVal::U8(ro_bytes(RoKind::Bool, v)[0])],
                (StyleDefault::U32(_), Some(v)) => {
                    let b = ro_bytes(RoKind::U32, v);
                    vec![StyleVal::U32(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))]
                }
                (StyleDefault::F64(_), Some(RoValue::F64(f))) => vec![StyleVal::F64(f)],
                (StyleDefault::U8(x), None) => vec![StyleVal::U8(x)],
                (StyleDefault::U32(x), None) => vec![StyleVal::U32(x)],
                (StyleDefault::F64(x), _) => vec![StyleVal::F64(x)],
            };
            // An item every style holds that no setting names (1001,
            // 7010) takes the style's own value when it keeps one.
            let values = match (set, s.items.iter().find(|x| x.id == *id)) {
                (None, Some(x)) => match &x.value {
                    crate::StyleValue::Bool(b) => vec![StyleVal::U8(*b as u8)],
                    crate::StyleValue::Int(i) => vec![StyleVal::U32(*i as u32)],
                    crate::StyleValue::Double(f) => vec![StyleVal::F64(*f)],
                    crate::StyleValue::Colour(c) => vec![StyleVal::Rgba(*c)],
                    crate::StyleValue::Other { .. } => values,
                },
                _ => values,
            };
            items.push(StyleItem {
                id: *id,
                values,
                strokes: None,
            });
        }
        // Items a later SketchUp adds: the rendering options a 2017 style
        // names only when such a SketchUp saved it (section fill, 7015
        // and 7016), then the items the model keeps as they are, each
        // among the items in the order of their ids.
        let mut more: Vec<StyleItem> = Vec::new();
        for (f, (_, v)) in RO_FIELDS.iter().zip(&values) {
            let (Some(id), Some(v)) = (f.item, *v) else {
                continue;
            };
            if STYLE_ITEMS.iter().any(|(i, _)| *i == id)
                || matches!(f.name, "hidden_geometry" | "hidden_objects")
            {
                continue;
            }
            let value = match (f.kind, stored(f.name, v)) {
                (RoKind::Bool, v) => StyleVal::U8(ro_bytes(RoKind::Bool, v)[0]),
                (RoKind::F64, RoValue::F64(x)) => StyleVal::F64(x),
                (RoKind::F32, v) => {
                    // A float item has no 2017 form; the value every
                    // 2026 file gives it loses nothing.
                    let text = match v {
                        RoValue::F32(x) => crate::xml26::g_digits(x as f64, 8),
                        _ => String::new(),
                    };
                    let constant = crate::xml26::STYLE_CONSTANTS
                        .iter()
                        .any(|(i, k, t)| *i == id && *k == 6 && *t == text);
                    if !constant {
                        self.notes.push(format!(
                            "style {:?} item {id} is left out: its value type has no 2017 form",
                            s.name
                        ));
                    }
                    continue;
                }
                (_, v) => {
                    let b = ro_bytes(RoKind::U32, v);
                    StyleVal::U32(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                }
            };
            more.push(StyleItem {
                id,
                values: vec![value],
                strokes: None,
            });
        }
        for x in &s.items {
            let value = match &x.value {
                crate::StyleValue::Bool(b) => StyleVal::U8(*b as u8),
                crate::StyleValue::Int(i) => StyleVal::U32(*i as u32),
                crate::StyleValue::Double(f) => StyleVal::F64(*f),
                crate::StyleValue::Colour(c) => StyleVal::Rgba(*c),
                crate::StyleValue::Other { kind, text } => {
                    // A value every 2026 file holds loses nothing.
                    let constant = crate::xml26::STYLE_CONSTANTS
                        .iter()
                        .any(|(id, k, t)| *id == x.id && k == kind && t == text);
                    if !constant {
                        self.notes.push(format!(
                            "style {:?} item {} is left out: its value type has no 2017 form",
                            s.name, x.id
                        ));
                    }
                    continue;
                }
            };
            more.push(StyleItem {
                id: x.id,
                values: vec![value],
                strokes: None,
            });
        }
        let mut have: HashSet<u32> = items.iter().map(|i| i.id).collect();
        let mut more: Vec<StyleItem> = more.into_iter().filter(|i| have.insert(i.id)).collect();
        if !more.is_empty() {
            items.append(&mut more);
            // Stable: items of one id keep their order.
            items.sort_by_key(|i| i.id);
        }
        // The strokes of a sketchy-edge style, item 1013, among the items
        // in the order of their ids.
        if let Some(st) = &s.strokes {
            let images = st.images.iter().map(|b| self.dib_alone(b)).collect();
            let item = StyleItem {
                id: crate::file17::ITEM_STROKES,
                values: Vec::new(),
                strokes: Some(Box::new(crate::file17::Strokes17 {
                    numbers: [st.dropout_length, st.fade_factor],
                    a: st.width,
                    b: st.halo as u8,
                    c: st.halo_length,
                    variants: st.variants,
                    lengths: st.lengths.clone(),
                    images,
                })),
            };
            let at = items.partition_point(|i| i.id < item.id);
            items.insert(at, item);
        }
        let body = Style17 {
            guid: self.guid(&s.guid),
            name: S16::new(&s.name),
            description: S16::new(&s.description),
            items,
            ..Default::default()
        };
        (body, refs)
    }

    /// A watermark list as item 5001 holds it: the background watermarks,
    /// a null for the model, the overlays; empty without watermarks.
    fn watermark_list(
        &mut self,
        watermarks: &[crate::Watermark],
        written: &mut HashMap<WatermarkKey, u32>,
    ) -> Vec<Ptr> {
        let mut one = |b: &mut Builder, w: &crate::Watermark| -> Option<Ptr> {
            let bytes = w.image_bytes.as_ref()?;
            let key: WatermarkKey = (
                w.name.clone(),
                w.source_file.clone(),
                crate::zip::crc32(bytes),
                [
                    w.position as u64,
                    w.tiled as u64,
                    w.stretched as u64,
                    w.keep_aspect_ratio as u64,
                    w.background as u64,
                    w.mask as u64,
                    w.blend.to_bits(),
                    w.scale.to_bits(),
                ],
            );
            if let Some(&id) = written.get(&key) {
                return Some(Ptr::Ref(id));
            }
            let id = b.id();
            written.insert(key, id);
            let image = b.dib(bytes, None);
            Some(Ptr::new(
                id,
                Body::Watermark(Box::new(Watermark17 {
                    pre: Pre::default(),
                    zero: 0,
                    path: S16::new(&w.source_file),
                    time: crate::xml26::watermark_time(w),
                    position: w.position,
                    tiled: w.tiled as u8,
                    stretched: w.stretched as u8,
                    keep_aspect: w.keep_aspect_ratio as u8,
                    background: w.background as u8,
                    mask: w.mask as u8,
                    blend: w.blend,
                    scale: w.scale,
                    name: S16::new(&w.name),
                    image,
                })),
            ))
        };
        let mut background = Vec::new();
        let mut overlay = Vec::new();
        for w in watermarks {
            match one(self, w) {
                Some(p) if w.background => background.push(p),
                Some(p) => overlay.push(p),
                None => self.notes.push(format!(
                    "watermark {:?} has no image and is left out",
                    w.name
                )),
            }
        }
        if background.is_empty() && overlay.is_empty() {
            return Vec::new();
        }
        background.push(Ptr::Null);
        background.extend(overlay);
        background
    }

    /// The saved-property flags a scene is written with.
    fn scene_flags(&self, s: &Scene) -> u32 {
        if self.model.container == crate::Container::Stream {
            s.saved.0
        } else {
            s.saved.0 & 0x7F
        }
    }

    fn scene(&mut self, s: &Scene, style: Ptr, scene_id: u32) -> Ptr {
        let model = self.model;
        // A 2017 scene's flags are its own; a 2026 scene's bits past 64
        // mean other things there (§16.10), so only the seven properties
        // of bits 1 to 64 carry over from one.
        let flags = self.scene_flags(s);
        let has = |bit: u32| flags & bit != 0;
        let mut body = Scene17 {
            name: S16::new(&s.name),
            description: S16::new(&s.description),
            flags,
            in_animation: s.in_animation as u8,
            f: {
                // The page's own transition and delay times, -1 for the
                // model's.
                let k = model.scenes.iter().position(|x| std::ptr::eq(x, s));
                let extra = k.and_then(|k| {
                    model
                        .features
                        .scene_pages
                        .iter()
                        .find(|(i, _)| *i == k)
                        .map(|(_, x)| x)
                });
                let time =
                    |t: Option<f64>| t.filter(|t| t.is_finite() && *t >= 0.0).unwrap_or(-1.0);
                [
                    time(extra.and_then(|x| x.transition_s)),
                    time(extra.and_then(|x| x.delay_s)),
                ]
            },
            thumb_flags: if s.thumbnail.is_some() {
                [1, 1]
            } else {
                [1, 0]
            },
            thumb_kind: s.thumbnail.is_some() as u8,
            thumb: Blob(s.thumbnail.clone().unwrap_or_default()),
            ..Default::default()
        };
        // A release whose scenes store no thumbnail (§14) holds none of
        // these fields.
        if !self.release.scene_has_thumbnail() {
            (body.thumb_flags, body.thumb_kind, body.thumb) = ([0; 2], 0, Blob::default());
        }
        if let Some(p) = s
            .match_photo
            .as_ref()
            .filter(|_| self.release.scene_has_photo())
        {
            let image = if p.image.is_empty() {
                Ptr::Null
            } else {
                self.dib_alone(&p.image)
            };
            let id = self.id();
            body.photo = Ptr::new(
                id,
                Body::BackgroundImage(Box::new(crate::file17::BackgroundImage17 {
                    path: S16::new(&p.path),
                    image,
                    width: p.width,
                    height: p.height,
                    time: p.time,
                    scale: p.scale,
                    points: p.points.clone(),
                    ..Default::default()
                })),
            );
        }
        if has(SceneProperties::CAMERA) {
            let c = s
                .camera
                .clone()
                .or_else(|| model.camera.clone())
                .unwrap_or_else(default_camera);
            body.camera = Ptr::new(self.id(), Body::Camera(Box::new(camera(&c))));
        }
        let shadows = s.shadows.as_ref().or(model.shadows.as_ref());
        if has(SceneProperties::STYLE_AND_FOG) {
            body.rendering = rendering(
                s.rendering.as_ref().or(model.rendering.as_ref()),
                shadows.map(|x| x.displayed).unwrap_or(false),
                self.release,
            );
            body.style = style;
        }
        if has(SceneProperties::SHADOWS) {
            body.shadow = shadow(shadows.unwrap_or(&default_shadows()));
        }
        if has(SceneProperties::AXES) {
            // SketchUp turns a scene's axes on when this byte is set
            // whatever the rendering options say (§10.10): the byte is
            // the axes' visibility as the scene shows them.
            let shown = has(SceneProperties::STYLE_AND_FOG)
                .then_some(s.rendering.as_ref())
                .flatten()
                .or(model.rendering.as_ref())
                .is_none_or(|r| r.model_axes);
            body.axes = axes(s.axes.as_ref().or(model.axes.as_ref()), shown as u8);
        }
        let refs = |b: &mut Builder, pids: &[u32], what: &str| -> Vec<Ptr> {
            pids.iter()
                .filter_map(|p| match b.obj_of_pid.get(p) {
                    Some(&id) => Some(Ptr::Ref(id)),
                    None => {
                        b.notes.push(format!(
                            "scene {:?} names {what} {p}, which the file does not hold",
                            s.name
                        ));
                        None
                    }
                })
                .collect()
        };
        if has(SceneProperties::HIDDEN_GEOMETRY) {
            body.hidden_entities = refs(self, &s.hidden_entities, "hidden entity");
        }
        if has(SceneProperties::VISIBLE_LAYERS) {
            body.hidden_layers = s
                .hidden_layers
                .iter()
                .filter_map(|&i| self.layer_ids.get(i).map(|&id| Ptr::Ref(id)))
                .collect();
        }
        if has(SceneProperties::ACTIVE_SECTION_PLANES) {
            body.section_planes = refs(self, &s.active_section_planes, "section plane");
        }
        Ptr::new(scene_id, Body::Scene(Box::new(body)))
    }

    fn build(mut self) -> Result<Built, Error> {
        let model = self.model;
        let release = self.release;
        if release.space_has_groups()
            && crate::features::folders_too_deep(&model.features.layer_folders, 0)
        {
            return Err(Error(format!(
                "layer folders are nested more than {} deep",
                crate::features::FOLDER_DEPTH
            )));
        }
        let mut f = File17 {
            release,
            version: S16::new(release.version_string()),
            seed: u32::from_le_bytes(crate::env::random_bytes()),
            ..Default::default()
        };
        f.guid = self.guid(model.model_guid.as_deref().unwrap_or(""));
        // A 2017 model written as 2017 keeps its own version string.
        if release.keeps_own_version()
            && model.container == crate::Container::Stream
            && model.version.starts_with("{17.")
        {
            f.version = S16::new(&model.version);
        }
        // What the release cannot hold is reported, never refused (§14).
        let guids = model
            .geometry
            .iter()
            .flat_map(|r| &r.placed)
            .filter(|p| !p.guid.is_empty())
            .count();
        if guids > 0 && !release.instance_has_guid() {
            self.notes.push(format!(
                "instance identifiers are not stored by release {}; {guids} instances are written without theirs",
                release.number()
            ));
        }
        let stippled = model.guides.iter().filter(|g| g.stipple != 0xFFFF).count();
        if stippled > 0 && release < Release::R2018 {
            self.notes.push(format!(
                "guide dash patterns are not stored by release {}; {stippled} guides are written solid",
                release.number()
            ));
        }
        let thumbnails = model
            .scenes
            .iter()
            .filter(|s| s.thumbnail.is_some())
            .count();
        if thumbnails > 0 && !release.scene_has_thumbnail() {
            self.notes.push(format!(
                "scene thumbnails are not stored by release {}; {thumbnails} scenes are written without theirs",
                release.number()
            ));
        }
        let ft = &model.features;
        let hatched = model
            .geometry
            .iter()
            .flat_map(|r| &r.sections)
            .filter(|s| s.hatch.is_some())
            .count();
        for (what, n) in [
            ("hatch patterns", ft.hatch_patterns.len()),
            ("material hatch patterns", ft.material_hatch.len()),
            ("instance hatch patterns", ft.instance_hatch.len()),
            ("section plane hatch patterns", hatched),
            (
                "the section hatch of the display settings",
                ft.render_hatch.is_some() as usize,
            ),
            (
                "layer folders",
                if release.space_has_groups() {
                    0
                } else {
                    ft.layer_folders.len()
                },
            ),
            (
                "layer line styles",
                if release.layer_has_style() {
                    0
                } else {
                    ft.layer_line_styles.len()
                },
            ),
            (
                "layer page behaviours",
                if release.layer_has_words() {
                    0
                } else {
                    ft.layer_page_behaviour.len()
                },
            ),
            ("the coordinate reference system", ft.crs.is_some() as usize),
            ("physically based materials", ft.material_pbr.len()),
            ("environments", ft.environments.len()),
            ("page environments", ft.scene_environment.len()),
            (
                "page extras",
                ft.scene_pages
                    .iter()
                    .filter(|(_, x)| {
                        x.display_name.is_some()
                            || !x.hidden_layer_folders.is_empty()
                            || (!release.scene_has_pair()
                                && (x.transition_s.is_some() || x.delay_s.is_some()))
                    })
                    .count(),
            ),
            ("current style records", ft.current_style.is_some() as usize),
            ("snaps", ft.snaps.len()),
            ("procedures", ft.procedures.len()),
            ("procedure kinds", ft.procedure_kinds.len()),
        ] {
            if n > 0 {
                self.notes.push(format!(
                    "{what} are not stored by release {}; {n} written without them",
                    release.number()
                ));
            }
        }
        f.tail_count = 1 + model
            .geometry
            .iter()
            .map(|r| r.sections.len() as u32)
            .sum::<u32>();
        let mut lost: Vec<&str> = model
            .rendering
            .iter()
            .chain(model.scenes.iter().filter_map(|s| s.rendering.as_ref()))
            .flat_map(|ro| rendering_lost(ro, release))
            .collect();
        lost.sort_unstable();
        lost.dedup();
        if !lost.is_empty() {
            self.notes.push(format!(
                "display settings are not stored by release {}: {} are written as the default document's",
                release.number(),
                lost.join(", ")
            ));
        }
        // The settings and lists a stream file has no record for (§16.8):
        // the definition, hatch-pattern and environment lists and the
        // hatch patterns always, the line styles before 2019, the layer
        // folders before 2020, the styles before release 6. A release
        // without attributes is counted with the other objects.
        let unheld = |o: crate::Owner| {
            use crate::Owner;
            match o {
                Owner::Definitions
                | Owner::HatchPatterns
                | Owner::Environments
                | Owner::HatchPattern(_) => true,
                Owner::LineStyles | Owner::LineStyle(_) => !release.has_line_styles(),
                Owner::LayerFolder(_) => !release.space_has_groups(),
                Owner::Styles | Owner::Style(_) | Owner::CurrentStyle => !release.has_styles(),
                _ => false,
            }
        };
        // Owners, not entries: one that holds a dictionary, counted once.
        // None of them is visited by `container`, so a release without
        // attributes counts them here too.
        let settings_owners = model
            .dictionaries
            .iter()
            .filter(|a| a.owner.is_settings() && unheld(a.owner) && !a.dictionaries.is_empty())
            .map(|a| a.owner)
            .collect::<std::collections::HashSet<_>>()
            .len();
        if settings_owners > 0 {
            self.notes.push(format!(
                "{settings_owners} dictionary sets of lists or settings (the definition, hatch pattern or environment list, hatch patterns, or what the release lacks: line styles, layer folders, styles) are not stored by release {}",
                release.number()
            ));
        }
        if !release.has_option_sets() {
            if model.preview.is_some() {
                self.notes.push(format!(
                    "the document preview image is not stored by release {}",
                    release.number()
                ));
            }
            let props = model
                .dictionaries
                .iter()
                .filter(|a| a.owner == crate::Owner::Model)
                .flat_map(|a| &a.dictionaries)
                .any(|d| d.name == "ModelProperties");
            if props {
                self.notes.push(format!(
                    "the ModelProperties dictionary is not stored by release {}",
                    release.number()
                ));
            }
        }
        if !release.has_view_watermarks() && model.anti_aliased_textures == Some(false) {
            self.notes.push(format!(
                "the anti-aliased textures setting is not stored by release {}; it reads as on",
                release.number()
            ));
        }
        if release.has_attributes()
            && !release.attribute_has_tail()
            && model
                .dictionaries
                .iter()
                .flat_map(|a| &a.dictionaries)
                .any(|d| d.flag != 0)
        {
            self.notes.push(format!(
                "dictionary flags are not stored by release {}",
                release.number()
            ));
        }
        if !release.definition_has_extra()
            && model.definitions.iter().any(|d| d.behaviour.extra != 0)
        {
            self.notes.push(format!(
                "the behaviour word after a definition's bits is not stored by release {}",
                release.number()
            ));
        }
        if !release.definition_has_bits()
            && model
                .definitions
                .iter()
                .any(|d| d.behaviour.always_faces_camera || d.behaviour.shadows_face_sun)
        {
            self.notes.push(format!(
                "the behaviour bits of definitions (always faces the camera, shadows face the sun) are not stored by release {}",
                release.number()
            ));
        }
        let bounded = model
            .guides
            .iter()
            .filter(|g| g.bounds != [-1e30, 1e30])
            .count();
        if bounded > 0 && !release.guide_has_tail() {
            self.notes.push(format!(
                "guide-line bounds are not stored by release {}; {bounded} guides are written unbounded",
                release.number()
            ));
        }
        // A release before 4 holds the direction to the target, which
        // reads back as the point one inch along it (§14).
        let distant = model
            .camera
            .iter()
            .chain(model.scenes.iter().filter_map(|s| s.camera.as_ref()))
            .filter(|c| {
                let d: f64 = (0..3)
                    .map(|k| (c.target_m[k] - c.eye_m[k]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                d > 0.0 && (d - 0.0254).abs() > 1e-7
            })
            .count();
        if distant > 0 && !release.camera_is_object() {
            self.notes.push(format!(
                "the distance from a camera to its target is not stored by release {}; {distant} cameras are written with the target one inch from the eye",
                release.number()
            ));
        }
        if model.selected_scene.is_some() && !release.has_selected_scene() {
            self.notes.push(format!(
                "the selected scene is not stored by release {}",
                release.number()
            ));
        }
        let named = model
            .instances
            .iter()
            .filter(|i| i.pid != 0 && i.name.as_ref().is_some_and(|n| !n.is_empty()))
            .count();
        if named > 0 && !release.instance_has_name() {
            self.notes.push(format!(
                "instance names are not stored by release {}; {named} are left out",
                release.number()
            ));
        }
        let locked = model
            .geometry
            .iter()
            .flat_map(|g| &g.placed)
            .filter(|p| p.locked)
            .count();
        if locked > 0 && !release.drawbase_has_locked() {
            self.notes.push(format!(
                "locks are not stored by release {}; {locked} instances are written unlocked",
                release.number()
            ));
        }
        let n = model
            .scenes
            .iter()
            .filter(|s| s.match_photo.is_some())
            .count();
        if n > 0 && !release.scene_has_photo() {
            self.notes.push(format!(
                "matched photos are not stored by release {}; {n} are left out",
                release.number()
            ));
        }
        if !model.styles.is_empty() && !release.has_styles() {
            self.notes.push(format!(
                "styles are not stored by release {}; {} are left out",
                release.number(),
                model.styles.len()
            ));
        }
        let dimension_style = model
            .dimensions
            .iter()
            .any(|d| d.font.is_some() || d.aligned || d.arrow != 0 || d.text_position != 0)
            || model
                .radial_dimensions
                .iter()
                .any(|d| d.font.is_some() || d.aligned || d.arrow != 0);
        if dimension_style && !release.dimension_has_font() {
            self.notes.push(format!(
                "dimension fonts, alignment, arrows and text positions are not stored by release {}",
                release.number()
            ));
        }
        let screen_text = model.text_defaults.as_ref().is_some_and(|t| {
            t.screen_font.is_some() || ![[0; 4], [63, 63, 63, 255]].contains(&t.screen_text_color)
        });
        if screen_text && !release.has_screen_text() {
            self.notes.push(format!(
                "the screen-text font and colour are not stored by release {}",
                release.number()
            ));
        }
        if release.font_tail_len() < 15
            && model.fonts.iter().any(|f| {
                f.model_height || ![0.0, crate::Font::DEFAULT_HEIGHT_M].contains(&f.height_m)
            })
        {
            self.notes.push(format!(
                "font heights are not stored by release {}",
                release.number()
            ));
        }
        let two_point = model
            .camera
            .iter()
            .chain(model.scenes.iter().filter_map(|s| s.camera.as_ref()))
            .any(|c| c.two_point_perspective || c.scale_2d != 1.0 || c.offset_2d != [0.0; 2]);
        if two_point && release.camera_tail_len() < 33 {
            self.notes.push(format!(
                "two-point perspective and the 2D zoom and pan are not stored by release {}",
                release.number()
            ));
        }
        let qualities = model.materials.iter().any(|m| {
            matches!(
                m,
                Material::Textured {
                    jpeg_quality: Some(_),
                    ..
                }
            )
        });
        if qualities && !release.dib_has_quality() {
            self.notes.push(format!(
                "JPEG qualities are not stored by release {}",
                release.number()
            ));
        }
        let photos = model
            .scenes
            .iter()
            .filter_map(|s| s.match_photo.as_ref())
            .filter(|p| p.width != 0 || p.height != 0 || p.time != 0)
            .count();
        if photos > 0 && !release.photo_has_size() && release.scene_has_photo() {
            self.notes.push(format!(
                "matched-photo sizes and times are not stored by release {}; {photos} are left out",
                release.number()
            ));
        }
        if !model.classifications.is_empty() && !release.has_schema_list() {
            self.notes.push(format!(
                "classification schemas are not stored by release {}; {} are left out",
                release.number(),
                model.classifications.len()
            ));
        }

        // ---- before the model
        f.version_map = Ptr::new(
            self.id(),
            Body::VersionMap(
                release
                    .version_map()
                    .into_iter()
                    .chain([(END_OF_VERSION_MAP, 0)])
                    .map(|(n, s)| (S16::new(n), s))
                    .collect(),
            ),
        );
        f.thumbnail = match &model.thumbnail {
            Some(bytes) => self.dib_alone(bytes),
            None => self.png(128, 128),
        };
        let (u, a) = (&model.units, &model.animation);
        f.option_sets = vec![
            OptionSet {
                name: "NamedOptions".into(),
                entries: vec![],
            },
            OptionSet {
                name: "PageOptions".into(),
                entries: vec![
                    entry("ShowTransition", AttrVal::Bool(a.transitions as u8)),
                    entry("TransitionTime", AttrVal::F64(a.transition_s)),
                ],
            },
            OptionSet {
                name: "SlideshowOptions".into(),
                entries: vec![
                    entry("LoopSlideshow", AttrVal::Bool(a.loop_slideshow as u8)),
                    entry("SlideTime", AttrVal::F64(a.delay_s)),
                ],
            },
            OptionSet {
                name: "UnitsOptions".into(),
                entries: vec![
                    entry("LengthPrecision", AttrVal::I32(u.length_precision as i32)),
                    entry("LengthFormat", AttrVal::I32(u.length_format as i32)),
                    entry("LengthUnit", AttrVal::I32(u.length_unit as i32)),
                    entry("LengthSnapEnabled", AttrVal::Bool(u.length_snap as u8)),
                    entry("LengthSnapLength", AttrVal::F64(u.length_snap_length)),
                    entry("AnglePrecision", AttrVal::I32(u.angle_precision as i32)),
                    entry("AngleSnapEnabled", AttrVal::Bool(u.angle_snap as u8)),
                    entry("SnapAngle", AttrVal::F64(u.snap_angle)),
                    entry(
                        "SuppressUnitsDisplay",
                        AttrVal::Bool(u.suppress_units_display as u8),
                    ),
                    entry(
                        "ForceInchDisplay",
                        AttrVal::Bool(u.force_inch_display as u8),
                    ),
                ],
            },
        ];
        // Area and volume units, which a 2017 file holds when a later
        // SketchUp saved it, after the entries every file has; SketchUp
        // 2017 ignores them (§16.12).
        let units = &mut f.option_sets.last_mut().expect("units options").entries;
        for (key, v) in [
            ("AreaUnit", u.area_unit),
            ("VolumeUnit", u.volume_unit),
            ("AreaPrecision", u.area_precision),
            ("VolumePrecision", u.volume_precision),
        ] {
            if let Some(v) = v {
                units.push(entry(key, AttrVal::I32(v as i32)));
            }
        }
        // The model's own dictionaries: `ModelProperties` in the
        // properties container, the rest with the model's space. A
        // model without them takes the properties of a new document and
        // a `GeoReference` built from its settings.
        let own = self.dicts.remove(&crate::Owner::Model).unwrap_or_default();
        let (properties, rest): (Vec<_>, Vec<_>) =
            own.into_iter().partition(|d| d.name == "ModelProperties");
        f.properties = if !release.has_option_sets() {
            // A release without the field holds no properties (§14, 6 →
            // 7); the note above names a dictionary left out.
            Ptr::Null
        } else if properties.is_empty() {
            let (container, dictionary) = (self.id(), self.id());
            Ptr::new(
                container,
                Body::AttrContainer(AttrContainer {
                    pre: Pre::default(),
                    dictionaries: vec![Ptr::new(
                        dictionary,
                        Body::AttrNamed(AttrNamed {
                            name: "ModelProperties".into(),
                            entries: vec![
                                entry("Description", AttrVal::Str(S16::default())),
                                entry("IsClassified", AttrVal::Bool(0)),
                                entry("IsDynamic", AttrVal::Bool(0)),
                                entry("Name", AttrVal::Str(S16::default())),
                            ],
                            ..Default::default()
                        }),
                    )],
                }),
            )
        } else {
            self.dicts.insert(crate::Owner::Model, properties);
            self.container(crate::Owner::Model, Vec::new())
        };
        // A model built without dictionaries takes a `GeoReference` from
        // its geo-location settings; one read from a file holds its own,
        // and its flat attributes may name another object's location.
        let mut rest = rest;
        if model.dictionaries.is_empty() {
            rest.extend(self.geo_reference_dictionary());
        }
        let space_attrs = if rest.is_empty() {
            Ptr::Null
        } else {
            self.dicts.insert(crate::Owner::Model, rest);
            self.container(crate::Owner::Model, Vec::new())
        };
        f.preview = match &model.preview {
            _ if !release.has_option_sets() => Ptr::Null,
            Some(bytes) => self.dib_alone(bytes),
            None => self.png(128, 128),
        };
        let shadows = model.shadows.clone().unwrap_or_else(default_shadows);
        f.camera = Ptr::new(
            self.id(),
            Body::Camera(Box::new(camera(
                model.camera.as_ref().unwrap_or(&default_camera()),
            ))),
        );
        f.rendering = rendering(model.rendering.as_ref(), shadows.displayed, release);
        f.rendering.pre.attrs = self.container(crate::Owner::RenderingOptions, Vec::new());

        // ---- the model
        f.space.pre.attrs = space_attrs;
        f.space.materials_pre.attrs = self.container(crate::Owner::Materials, Vec::new());
        f.space.materials = self.materials();
        f.space.layers_pre.attrs = self.container(crate::Owner::Layers, Vec::new());
        f.space.layers = self.layers();
        if release.space_has_groups() {
            let mut next = 0;
            f.space.groups.2 = model
                .features
                .layer_folders
                .iter()
                .map(|g| self.layer_folder(g, &mut next))
                .collect();
        }
        let active = model
            .active_layer
            .and_then(|i| self.layer_ids.get(i).copied())
            .unwrap_or(self.layer_ids[0]);
        f.space.active_layer = Ptr::Ref(active);
        f.space.current_material = model
            .current_material
            .and_then(|i| model.material_links.iter().find(|(_, x)| *x == i))
            .map(|(slot, _)| self.material_ptr(*slot))
            .unwrap_or(Ptr::Null);
        for index in definition_order(model)? {
            let d = self.definition(index);
            f.space.definitions.push(d);
        }
        for r in &model.geometry {
            let owned = match r.def_index {
                None => true,
                some => model.definitions.iter().any(|d| d.map_index == some),
            };
            if !owned {
                self.notes.push(format!(
                    "a run of {} edges and {} faces belongs to definition map index {:?}, which no definition has, and is left out",
                    r.mesh.edges.len(),
                    r.mesh.faces.len(),
                    r.def_index
                ));
            }
        }
        let empty = empty_run(None);
        let root = model
            .geometry
            .iter()
            .find(|r| r.def_index.is_none())
            .unwrap_or(&empty);
        let (entities, active) = self.entities(root, true);
        f.space.entities = entities;
        f.space.relationships = self.relationships(root);
        f.space.active_section = active;

        // ---- after the model
        f.shadow = shadow(&shadows);
        f.shadow.pre.attrs = self.container(crate::Owner::ShadowInfo, Vec::new());
        let mut written = HashMap::new();
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
        // A style keeps one identifier: the current settings name the
        // active style by it.
        // A stream file lists a style once per identifier, so a second
        // style with the same identifier gets a new one.
        let mut guids: std::collections::HashSet<[u8; 16]> = std::collections::HashSet::new();
        let mut renewed = 0;
        for s in styles.iter_mut() {
            match crate::build26::guid_bytes(&s.guid) {
                Some(g) if guids.insert(g) => {}
                kept => {
                    renewed += kept.is_some() as usize;
                    let g = crate::build26::random_guid(&mut self.seed);
                    guids.insert(g);
                    s.guid = g.iter().map(|b| format!("{b:02x}")).collect();
                }
            }
        }
        if renewed > 0 {
            self.notes.push(format!(
                "{renewed} styles shared an identifier with an earlier style and are written with new ones"
            ));
        }
        let active_style = model
            .active_style
            .filter(|&i| i < styles.len())
            .unwrap_or(0);
        // Styles are written where a scene first names them, so their
        // watermarks are made in that order too: scenes first.
        let scene_style: Vec<Option<usize>> = model
            .scenes
            .iter()
            .map(|s| {
                (s.saved.0 & SceneProperties::STYLE_AND_FOG != 0).then(|| {
                    s.style
                        .as_deref()
                        .and_then(|n| styles.iter().position(|x| x.name == n))
                        .unwrap_or(active_style)
                })
            })
            .collect();
        let mut order: Vec<usize> = Vec::new();
        for i in scene_style.iter().flatten().copied().chain(0..styles.len()) {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        // A release without styles (§14, 5 → 6) holds none, and its
        // scenes name none.
        if !release.has_styles() {
            order.clear();
        }
        let mut bodies: Vec<Option<Once>> = (0..styles.len()).map(|_| None).collect();
        for &i in &order {
            let s = styles[i].clone();
            let (mut body, _) = self.style(&s, &s.settings, &s.watermarks, &mut written);
            body.pre.attrs = self.container(crate::Owner::Style(i), Vec::new());
            bodies[i] = Some(Once {
                id: self.id(),
                body: Some(Body::Style(Box::new(body))),
            });
        }
        self.styles = bodies.into_iter().flatten().collect();
        let scene_ids: Vec<u32> = model.scenes.iter().map(|_| self.id()).collect();
        for (k, s) in model.scenes.iter().enumerate() {
            let style = match scene_style[k] {
                Some(i) if release.has_styles() => self.styles[i].ptr(),
                _ => Ptr::Null,
            };
            let mut scene = self.scene(s, style, scene_ids[k]);
            if let Ptr::New(o) = &mut scene {
                if let Body::Scene(b) = &mut o.body {
                    b.pre.attrs = self.container(crate::Owner::Scene(k), Vec::new());
                    // A scene writes a sub-record only for a saved property
                    // (style and fog 2, shadows 4, axes 8); the dictionaries
                    // of one it does not write are left out and counted.
                    let flags = self.scene_flags(s);
                    for (bit, owner) in [
                        (2, crate::Owner::SceneRendering(k)),
                        (4, crate::Owner::SceneShadowInfo(k)),
                        (8, crate::Owner::SceneAxes(k)),
                    ] {
                        if flags & bit == 0 {
                            self.lost_unsaved += self.dicts.get(&owner).map_or(0, |d| d.len());
                            continue;
                        }
                        let attrs = self.container(owner, Vec::new());
                        match owner {
                            crate::Owner::SceneRendering(_) => b.rendering.pre.attrs = attrs,
                            crate::Owner::SceneShadowInfo(_) => b.shadow.pre.attrs = attrs,
                            _ => b.axes.pre.attrs = attrs,
                        }
                    }
                }
            }
            f.scenes.push(scene);
        }
        f.scenes_pre.attrs = self.container(crate::Owner::Scenes, Vec::new());
        f.selected_scene = model
            .selected_scene
            .and_then(|i| scene_ids.get(i))
            .or(scene_ids.last())
            .map(|&id| Ptr::Ref(id))
            .unwrap_or(Ptr::Null);
        f.axes = axes(model.axes.as_ref(), 0);
        f.axes.pre.attrs = self.container(crate::Owner::Axes, Vec::new());

        let dd = model.dimension_defaults.clone();
        let td = model.text_defaults.clone();
        f.dimension_font = self.font_ptr(dd.as_ref().and_then(|d| d.font));
        f.dimension_defaults = DIMENSION_DEFAULTS;
        if let Some(d) = &dd {
            f.dimension_defaults[0] = d.aligned as u8;
            f.dimension_defaults[22] = d.highlight_non_associated as u8;
            f.dimension_defaults[27] = d.radius_prefix as u8;
            f.dimension_defaults[28] = d.hide_foreshortened as u8;
            f.dimension_defaults[37] = d.hide_small as u8;
            put(&mut f.dimension_defaults, 14, &d.arrow.to_le_bytes());
            put(&mut f.dimension_defaults, 46, &d.color);
            put(
                &mut f.dimension_defaults,
                54,
                &d.text_position.to_le_bytes(),
            );
        }
        f.text_font = self.font_ptr(td.as_ref().and_then(|t| t.font));
        f.text_defaults = TEXT_DEFAULTS;
        if let Some(t) = &td {
            put(&mut f.text_defaults, 0, &t.leader.to_le_bytes());
            put(&mut f.text_defaults, 9, &t.arrow.to_le_bytes());
            put(&mut f.text_defaults, 14, &t.leader_text_color);
            put(&mut f.text_defaults, 18, &t.screen_text_color);
        }
        if release.has_screen_text() {
            f.screen_font = self.font_ptr(td.as_ref().and_then(|t| t.screen_font));
        }
        f.fonts = (0..self.fonts.len()).map(|i| self.fonts[i].ptr()).collect();
        if !release.has_styles() {
            return self.finish(f);
        }

        f.styles_pre.attrs = self.container(crate::Owner::Styles, Vec::new());
        f.styles = (0..self.styles.len())
            .map(|i| self.styles[i].ptr())
            .collect();
        if release.has_line_styles() {
            f.line_styles_pre.attrs = self.container(crate::Owner::LineStyles, Vec::new());
            f.line_styles = std::mem::take(&mut self.stock_line_styles);
        }
        f.active_style = Ptr::Ref(self.styles[active_style].id);
        let current = styles[active_style].clone();
        let (body, current_watermarks) = self.style(
            &current,
            model.rendering.as_ref().unwrap_or(&current.settings),
            &model.watermarks,
            &mut written,
        );
        let mut body = body;
        body.pre.attrs = self.container(crate::Owner::CurrentStyle, Vec::new());
        f.current_style = Ptr::new(self.id(), Body::Style(Box::new(body)));
        f.style_changed = (model.watermarks != current.watermarks
            || model
                .rendering
                .as_ref()
                .map(|r| r.style_view() != current.settings.style_view())
                .unwrap_or(false)) as u32;
        // A release without the view's list keeps the watermarks in the
        // current style alone (§14, 6 → 7).
        if release.has_view_watermarks() {
            f.watermarks = current_watermarks;
        }
        f.anti_aliased_textures = model.anti_aliased_textures.unwrap_or(true) as u8;
        // A release without the schema list holds no classification
        // (§14, 2013 and 2014).
        f.schemas = model
            .classifications
            .iter()
            .filter(|_| release.has_schema_list())
            .map(|c| {
                let id = self.id();
                Ptr::new(
                    id,
                    Body::SchemaZip(SchemaZip {
                        path: S16::new(&c.source_path),
                        name: S16::new(&c.name),
                        data: Blob(c.data.clone()),
                    }),
                )
            })
            .collect();
        self.finish(f)
    }

    /// The file and its notes, with the next persistent id to give out.
    fn finish(mut self, mut f: File17) -> Result<Built, Error> {
        f.root[3] = self.pids.max.saturating_add(1);
        let release = self.release;
        if self.lost_dictionaries > 0 {
            self.notes.push(format!(
                "attribute dictionaries are not stored by release {}; {} are left out",
                release.number(),
                self.lost_dictionaries
            ));
        }
        if self.lost_unsaved > 0 {
            self.notes.push(format!(
                "{} dictionaries of scene style, shadow or axes settings are left out: the scene does not save that property",
                self.lost_unsaved
            ));
        }
        if self.lost_placements > 0 {
            self.notes.push(format!(
                "texture placements are not stored by release {}; {} faces are written without theirs",
                release.number(),
                self.lost_placements
            ));
        }
        // Before release 4 the cameras hold a direction, and each curve
        // names its first and last edge, the last written in the curve
        // where it is new (§14): the file is written with every object
        // where its first reference falls, and read back as built.
        if !release.camera_is_object() || release.curve_has_ends() {
            // Read back first, so that the ids follow the stream.
            f = File17::read(&f.write()?)?;
            let mut table = crate::file17::flatten(&mut f);
            for b in table.values_mut() {
                if let Body::Camera(c) = b {
                    if !release.camera_is_object() {
                        crate::file17::camera_direction(c);
                    }
                }
            }
            if release.curve_has_ends() {
                crate::file17::link_curve_ends(&mut table);
            }
            let bytes = crate::file17::write_flat(&f, table)?;
            f = File17::read(&bytes)?;
        }
        Ok(Built {
            file: f,
            notes: self.notes,
        })
    }

    /// The model's attribute container: the `GeoReference` dictionary
    /// when the model carries a geographic location.
    /// The `GeoReference` dictionary the model's settings give: the
    /// geo-location keys among its attributes, with the flag; none for
    /// a model without any.
    fn geo_reference_dictionary(&self) -> Option<crate::Dictionary> {
        let model = self.model;
        const KEYS: [&str; 6] = [
            "GeoReferenceNorthAngle",
            "Latitude",
            "Longitude",
            "ModelTranslationX",
            "ModelTranslationY",
            "UsesGeoReferencing",
        ];
        let mut entries: Vec<(String, crate::Value)> = KEYS
            .iter()
            .filter_map(|k| {
                model
                    .attributes
                    .iter()
                    .find(|a| a.key == *k)
                    .map(|a| (k.to_string(), value_of(&a.value)))
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
}

fn default_layer() -> crate::Layer {
    crate::Layer {
        name: "Layer0".into(),
        visible: true,
        rgba: [255, 84, 84, 255],
    }
}

fn empty_run(def_index: Option<usize>) -> GeometryRun {
    GeometryRun {
        start: 0,
        end: 0,
        top_level: 0,
        frame: None,
        def_index,
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
    }
}

/// Build the document of `model` as a file of `release`, one whose
/// every layout the grammar holds (§14).
pub(crate) fn from_model_for(model: &Model, release: Release) -> Result<Built, Error> {
    if !release.grammar() {
        return Err(Error(format!(
            "{UNSUPPORTED}: a file of release {} is not written: the grammar is that of {}",
            release.number(),
            Release::grammar_releases()
        )));
    }
    crate::build26::validate(model)?;
    let max_pid = model.max_pid();
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
    let mut b = Builder {
        model,
        release,
        next_id: 0,
        pids: Pids {
            next: max_pid.saturating_add(1).max(1),
            used: HashSet::new(),
            max: 0,
        },
        seed: u32::from_le_bytes(crate::env::random_bytes()) as u64 | 1,
        notes: Vec::new(),
        material_of_slot: HashMap::new(),
        layer_of_slot: HashMap::new(),
        layer_ids: Vec::new(),
        stock_line_styles: Vec::new(),
        def_ids: vec![None; model.definitions.len()],
        fonts: Vec::new(),
        styles: Vec::new(),
        lost_dictionaries: 0,
        lost_unsaved: 0,
        lost_placements: 0,
        instance_names: model
            .instances
            .iter()
            // An instance without an id cannot be told from another.
            .filter(|i| i.pid != 0)
            .filter_map(|i| i.name.clone().map(|n| (i.pid, n)))
            .collect(),
        obj_of_pid: HashMap::new(),
        runs: model.geometry.iter().map(|_| RunTable::default()).collect(),
        dib_of_image: HashMap::new(),
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
    };
    for f in &fonts {
        let id = b.id();
        b.fonts.push(Once {
            id,
            body: Some(Body::Font(font(f))),
        });
    }
    b.build()
}

/// A solid colour's alpha byte for an opacity (§8.1): the opacity times
/// 255, truncated.
pub(crate) fn alpha_byte(opacity: f64) -> u8 {
    (opacity.clamp(0.0, 1.0) * 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Arc, Curve, Mesh, MeshEdge, MeshFace};

    fn edge(v0: u32, v1: u32, curve: Option<u32>) -> MeshEdge {
        MeshEdge {
            pid: 0,
            v0,
            v1,
            soft: false,
            smooth: false,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
            layer: 0,
            material: None,
            curve,
        }
    }

    /// A circle of `n` vertices of radius 1 m about the origin, flat on
    /// the ground, vertex `k` at the angle `k` n-ths of a turn.
    fn circle(n: u32) -> (Vec<[f64; 3]>, Curve) {
        let vertices = (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64;
                [a.cos(), a.sin(), 0.0]
            })
            .collect();
        let arc = Arc {
            center_m: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            x_axis_m: [1.0, 0.0, 0.0],
            y_axis_m: [0.0, 1.0, 0.0],
            radius_m: 1.0,
            start_rad: 0.0,
            end_rad: std::f64::consts::TAU,
        };
        (
            vertices,
            Curve {
                pid: 0,
                members: n,
                arc: Some(arc),
            },
        )
    }

    /// The members of an arc are written from the arc's start, whatever
    /// order the model lists them in and whichever way each is stored;
    /// edges that are no member keep their places.
    #[test]
    fn arc_members_are_written_from_the_arcs_start() {
        let (mut vertices, curve) = circle(8);
        vertices.push([5.0, 5.0, 0.0]);
        vertices.push([6.0, 5.0, 0.0]);
        // Members from vertex k to k + 1, listed from the fourth on, the
        // odd ones stored against the arc, with a plain edge among them.
        let mut edges = Vec::new();
        for j in 0..8u32 {
            let k = (j + 3) % 8;
            let (a, b) = (k, (k + 1) % 8);
            edges.push(if k % 2 == 1 {
                edge(b, a, Some(0))
            } else {
                edge(a, b, Some(0))
            });
            if j == 2 {
                edges.push(edge(8, 9, None));
            }
        }
        let run = GeometryRun {
            mesh: Mesh {
                vertices,
                vertex_pids: vec![],
                faces: vec![],
                edges,
                vertices_in: vec![],
            },
            curves: vec![curve],
            ..Default::default()
        };
        let order = arc_order(&run);
        assert_eq!(order[3], 3, "the plain edge keeps its place");
        let starts: Vec<u32> = order
            .iter()
            .filter(|&&i| run.mesh.edges[i].curve.is_some())
            .map(|&i| run.mesh.edges[i].v0.min(run.mesh.edges[i].v1))
            .collect();
        // The last member joins vertex 7 to vertex 0.
        assert_eq!(starts, [0, 1, 2, 3, 4, 5, 6, 0]);
        let last = &run.mesh.edges[*order.last().unwrap()];
        assert_eq!((last.v0.max(last.v1), last.v0.min(last.v1)), (7, 0));
    }

    /// Three quarters of a circle in two members, both stored against
    /// the arc and listed last first: the member of half a turn, whose
    /// ends say the same either way round, comes first.
    #[test]
    fn a_member_of_half_a_turn_sorts_by_its_ends() {
        let (_, mut curve) = circle(2);
        curve.members = 2;
        if let Some(arc) = curve.arc.as_mut() {
            arc.end_rad = 1.5 * std::f64::consts::PI;
        }
        let run = GeometryRun {
            mesh: Mesh {
                vertices: vec![[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]],
                vertex_pids: vec![],
                faces: vec![],
                edges: vec![edge(2, 1, Some(0)), edge(1, 0, Some(0))],
                vertices_in: vec![],
            },
            curves: vec![curve],
            ..Default::default()
        };
        assert_eq!(arc_order(&run), [1, 0]);
    }

    /// A face that is flat only nearly: the plane lies midway between its
    /// vertices, not through the first.
    #[test]
    fn a_plane_lies_midway_between_the_vertices() {
        let m = Mesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0254e-3],
                [0.0, 1.0, 0.0],
            ],
            vertex_pids: vec![],
            faces: vec![],
            edges: vec![],
            vertices_in: vec![],
        };
        let f = MeshFace {
            pid: 0,
            outer: vec![0, 1, 2, 3],
            holes: vec![],
            normal: [0.0, 0.0, 1.0],
            front_material: None,
            back_material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
            layer: 0,
            texture: None,
        };
        // The vertices stand 0 and 0.001 inches up the normal.
        let d = crate::build26::plane_offset(&f, &m, |x| x / M_PER_INCH);
        assert!((d + 0.0005).abs() < 1e-12, "{d}");
    }

    #[test]
    fn a_camera_is_stored_upright() {
        let cam = |eye: [f64; 3], target: [f64; 3], up: [f64; 3]| crate::Camera {
            eye_m: eye,
            target_m: target,
            up,
            perspective: true,
            fov_deg: 35.0,
            parallel_height_m: 1.0,
            two_point_perspective: false,
            description: String::new(),
            undecoded: [1.0, 1000.0],
            aspect_ratio: 0.0,
            scale_2d: 1.0,
            offset_2d: [0.0; 2],
            image_width: 0.0,
        };
        let check = |c: &crate::Camera| {
            let u = crate::build26::upright(c);
            let d = [
                c.target_m[0] - c.eye_m[0],
                c.target_m[1] - c.eye_m[1],
                c.target_m[2] - c.eye_m[2],
            ];
            let dot = u[0] * d[0] + u[1] * d[1] + u[2] * d[2];
            let len = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
            assert!(dot.abs() < 1e-9, "{u:?} is not across {d:?}");
            assert!((len - 1.0).abs() < 1e-12, "{u:?} is not a unit vector");
            u
        };
        // Looking down at an angle with the world's up.
        let u = check(&cam([-4.0, -6.0, 3.5], [0.0, 0.0, 0.5], [0.0, 0.0, 1.0]));
        assert!(u[2] > 0.5);
        // Across already, but too long.
        assert_eq!(
            check(&cam([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 2.0])),
            [0.0, 0.0, 1.0]
        );
        // Up along the line of sight, and straight down.
        check(&cam([0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]));
        check(&cam([0.0, 0.0, 5.0], [0.0; 3], [0.0, 0.0, 1.0]));
        // Upright as it stands: unchanged to the bit.
        let kept = [0.6, 0.0, 0.8];
        assert_eq!(
            crate::build26::upright(&cam([0.0; 3], [0.8, 0.0, -0.6], kept)),
            kept
        );
        // No line of sight at all.
        let u = crate::build26::upright(&cam([1.0; 3], [1.0; 3], [0.0, 0.0, 3.0]));
        assert_eq!(u, [0.0, 0.0, 1.0]);
    }

    /// A scene's axes byte follows its rendering options (§10.10): the
    /// file's model axes stay 0, a scene showing the axes writes 1, a
    /// scene hiding them 0, and a scene without options of its own
    /// follows the model's.
    #[test]
    fn a_scenes_axes_byte_follows_its_rendering_options() {
        use crate::{RenderingOptions, Scene, SceneProperties};
        let mut m = Model {
            rendering: Some(RenderingOptions {
                model_axes: false,
                ..Default::default()
            }),
            ..Default::default()
        };
        let scene = |name: &str, saved: u32, axes: Option<bool>| Scene {
            name: name.into(),
            saved: SceneProperties(saved),
            rendering: axes.map(|model_axes| RenderingOptions {
                model_axes,
                ..Default::default()
            }),
            ..Default::default()
        };
        m.scenes = vec![
            scene(
                "shown",
                SceneProperties::STYLE_AND_FOG | SceneProperties::AXES,
                Some(true),
            ),
            scene(
                "hidden",
                SceneProperties::STYLE_AND_FOG | SceneProperties::AXES,
                Some(false),
            ),
            scene(
                "inherits",
                SceneProperties::STYLE_AND_FOG | SceneProperties::AXES,
                None,
            ),
            scene("no rendering", SceneProperties::AXES, None),
        ];
        let f = from_model_for(&m, Release::R2017).unwrap().file;
        assert_eq!(f.axes.shown, 0, "the model's axes");
        let bytes: Vec<u8> = f
            .scenes
            .iter()
            .map(|p| match &p.obj().unwrap().body {
                Body::Scene(s) => s.axes.shown,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(bytes, [1, 0, 0, 0]);
        m.rendering.as_mut().unwrap().model_axes = true;
        let f = from_model_for(&m, Release::R2017).unwrap().file;
        let bytes: Vec<u8> = f
            .scenes
            .iter()
            .map(|p| match &p.obj().unwrap().body {
                Body::Scene(s) => s.axes.shown,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(bytes, [1, 0, 1, 1]);
    }
}
