//! The whole stream-container file as typed records (`docs/WRITING.md`, SKP_FORMAT
//! §2–§10): every object of the archive and every field between them,
//! from the header to the last byte. Each layout is one function over the
//! symmetric codec ([`crate::ar17`]), so [`File17::read`] followed by
//! [`File17::write`] reproduces a file byte for byte, and a `File17`
//! built from a `Model` ([`crate::build17`]) is written by the same
//! layouts SketchUp's own files are read with.
//!
//! Fields whose meaning is not decoded are kept as the bytes they hold
//! (`SKP_FORMAT.md` §15 lists them); nothing in a file is skipped.

use crate::ar17::{Ar, R, S16};
use crate::model::Error;
use crate::Release;

/// A payload: an image, a schema archive, a block of fields.
#[derive(Clone, PartialEq, Default)]
pub(crate) struct Blob(pub Vec<u8>);

impl std::fmt::Debug for Blob {
    /// Short payloads in hexadecimal, long ones by length.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.len() > 256 {
            return write!(f, "[{} bytes]", self.0.len());
        }
        for (i, b) in self.0.iter().enumerate() {
            if i > 0 && i % 4 == 0 {
                f.write_str(" ")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// An object pointer (§3.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) enum Ptr {
    #[default]
    Null,
    /// A back-reference to the object with this id.
    Ref(u32),
    /// A new object, written here.
    New(Box<Obj>),
}

impl Ptr {
    pub fn new(id: u32, body: Body) -> Ptr {
        Ptr::New(Box::new(Obj { id, body }))
    }
    /// The object written at this pointer.
    pub fn obj(&self) -> Option<&Obj> {
        match self {
            Ptr::New(o) => Some(o),
            _ => None,
        }
    }
    /// The id of the object this pointer names, new or referenced.
    pub fn id(&self) -> Option<u32> {
        match self {
            Ptr::Null => None,
            Ptr::Ref(id) => Some(*id),
            Ptr::New(o) => Some(o.id),
        }
    }
}

/// One archive object. `id` is its identity for back-references: the
/// store-map slot in a file that was read, any unique number in a file
/// that is being built (the writer assigns the slots).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Obj {
    pub id: u32,
    pub body: Body,
}

/// The entity preamble (§5.1): the attribute container and the
/// persistent id.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Pre {
    pub attrs: Ptr,
    pub pid: u32,
}

/// The drawing-element base (§5.2). The material and the layer are object
/// pointers (null for none and for the default layer).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Drawbase {
    pub material: Ptr,
    pub hidden: u8,
    /// 1 on every element but a placed image.
    pub casts_shadows: u8,
    /// 1 on every observed element.
    pub receives_shadows: u8,
    pub soft: u8,
    pub smooth: u8,
    /// Instances and groups; 0 elsewhere.
    pub locked: u8,
    pub layer: Ptr,
}

impl Default for Drawbase {
    fn default() -> Self {
        Drawbase {
            material: Ptr::Null,
            hidden: 0,
            casts_shadows: 1,
            receives_shadows: 1,
            soft: 0,
            smooth: 0,
            locked: 0,
            layer: Ptr::Null,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Dib {
    /// 1 JPEG, 4 PNG.
    pub subtype: u32,
    pub data: Blob,
    /// JPEG quality; stored after a JPEG payload only.
    pub quality: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Camera17 {
    pub body: [u8; 137],
    pub one: u16,
    pub description: S16,
    pub tail: [u8; 33],
}

impl Default for Camera17 {
    /// An unzoomed view: the 2D zoom at +9 of the tail is 1, which a
    /// release whose tail ends before it (§14,
    /// [`Release::camera_tail_len`]) reads as.
    fn default() -> Self {
        let mut tail = [0; 33];
        tail[9..17].copy_from_slice(&1f64.to_le_bytes());
        Camera17 {
            body: [0; 137],
            one: 1,
            description: S16::default(),
            tail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Thumbnail {
    pub pre: Pre,
    pub camera: Ptr,
    pub image: Ptr,
}

/// A typed attribute value (§10.5).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AttrVal {
    Nil,
    I32(i32),
    F32(f32),
    F64(f64),
    Bool(u8),
    /// Seconds since 1970 (the shadow study's `ShadowTime`, `SunRise`
    /// and `SunSet`).
    Time(u32),
    Str(S16),
    Array(Vec<AttrVal>),
    Raw8([u8; 8]),
    Vec3([f64; 3]),
    /// A direction (the shadow study's `SunDirection`), 3 × f64 like
    /// [`AttrVal::Vec3`] under its own type byte.
    Dir3([f64; 3]),
}

impl AttrVal {
    fn type_byte(&self) -> u8 {
        match self {
            AttrVal::Nil => 0x00,
            AttrVal::I32(_) => 0x04,
            AttrVal::F32(_) => 0x05,
            AttrVal::F64(_) => 0x06,
            AttrVal::Bool(_) => 0x07,
            AttrVal::Time(_) => 0x09,
            AttrVal::Str(_) => 0x0A,
            AttrVal::Array(_) => 0x0B,
            AttrVal::Raw8(_) => 0x0C,
            AttrVal::Vec3(_) => 0x11,
            AttrVal::Dir3(_) => 0x12,
        }
    }
    fn of_type(t: u8, at: usize) -> R<AttrVal> {
        Ok(match t {
            0x00 => AttrVal::Nil,
            0x04 => AttrVal::I32(0),
            0x05 => AttrVal::F32(0.0),
            0x06 => AttrVal::F64(0.0),
            0x07 => AttrVal::Bool(0),
            0x09 => AttrVal::Time(0),
            0x0A => AttrVal::Str(S16::default()),
            0x0B => AttrVal::Array(Vec::new()),
            0x0C => AttrVal::Raw8([0; 8]),
            0x11 => AttrVal::Vec3([0.0; 3]),
            0x12 => AttrVal::Dir3([0.0; 3]),
            other => return Err(Error(format!("attribute value type {other:#x} at {at:#x}"))),
        })
    }
}

/// A key and its value, as option sets and attribute dictionaries hold
/// them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    pub key: S16,
    pub value: AttrVal,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct AttrContainer {
    pub pre: Pre,
    pub dictionaries: Vec<Ptr>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct AttrNamed {
    pub pre: Pre,
    pub four: [u8; 4],
    pub name: S16,
    pub entries: Vec<Entry>,
    pub tail: u32,
}

/// The texture half of a material (§8.1).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Texture17 {
    pub attrs: Ptr,
    pub b: u8,
    pub image: Ptr,
    /// The image held in place of `image` before release 6 (§14,
    /// [`Release::texture_image_object`]), after its byte `01`.
    pub embedded: Option<Dib>,
    pub size_in: [f64; 2],
    pub file_name: S16,
    pub color: [u8; 4],
    pub c: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Material17 {
    pub pre: Pre,
    pub name: S16,
    /// Present on a textured material.
    pub texture: Option<Texture17>,
    /// The byte a solid material holds in place of the texture.
    pub solid_b: u8,
    /// A solid material's colour; a textured material's average colour.
    pub rgba: [u8; 4],
    pub empty: S16,
    pub kind: u32,
    pub colorize: u32,
    pub opacity: f64,
    pub use_opacity: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Layer17 {
    pub pre: Pre,
    pub name: S16,
    pub hidden: u8,
    pub zero: u16,
    pub pid2: u32,
    pub internal: S16,
    pub two: [u8; 2],
    pub rgba: [u8; 4],
    pub empty: S16,
    pub kind: u32,
    pub colorize: u32,
    pub opacity: f64,
    pub use_opacity: u8,
    pub tail: u32,
    /// From 2019 (§14): the layer's dash pattern, a `CCustomLineStyle`
    /// pointer (null for none), new at its first user and a
    /// back-reference after.
    pub style: Ptr,
}

/// What the model and every definition share (§4, §7.1): the drawing
/// base, a material list, a layer list, a definition list and an entity
/// list, then the relationships and the active section plane.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Space {
    pub pre: Pre,
    pub drawbase: Drawbase,
    /// The material list's entity preamble (§5.1): where the list's
    /// dictionaries sit.
    pub materials_pre: Pre,
    pub materials: Vec<Ptr>,
    pub current_material: Ptr,
    /// The layer list's entity preamble.
    pub layers_pre: Pre,
    pub layers: Vec<Ptr>,
    pub active_layer: Ptr,
    /// Three bytes, a string and nine bytes after the active layer, from
    /// 2020 (§14): all empty or zero in every file seen.
    pub groups: ([u8; 3], S16, Vec<Ptr>, [u8; 5]),
    pub definitions: Vec<Ptr>,
    pub entities: Vec<Ptr>,
    /// `CRelationship` objects.
    pub relationships: Vec<Ptr>,
    /// The entity list's active section plane.
    pub active_section: Ptr,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Def17 {
    pub space: Space,
    pub guid: [u8; 16],
    pub name: S16,
    pub description: S16,
    pub source: S16,
    pub timestamp: u32,
    /// §7.1: a flag byte and 27 undecoded bytes, the behaviour (glue u8,
    /// cut u8, plane u32, bits u8), a u32, and the definition kind u32.
    pub mid: [u8; 43],
    pub thumbnail: Ptr,
}

impl Default for Def17 {
    fn default() -> Self {
        Def17 {
            space: Space::default(),
            guid: [0; 16],
            name: S16::default(),
            description: S16::default(),
            source: S16::default(),
            timestamp: 0,
            mid: [0; 43],
            thumbnail: Ptr::Null,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Vertex17 {
    pub pre: Pre,
    pub p: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Edge17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub v0: Ptr,
    pub v1: Ptr,
    pub curve: Ptr,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct EdgeUse17 {
    pub pre: Pre,
    pub edge: Ptr,
    pub dir: u8,
    pub parent: Ptr,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Loop17 {
    pub pre: Pre,
    /// 1 on a face's outer loop, 0 on a hole; then 1 on a convex loop.
    pub c: [u8; 2],
    pub uses: Vec<Ptr>,
}

impl Default for Loop17 {
    fn default() -> Self {
        Loop17 {
            pre: Pre::default(),
            c: [1, 1],
            uses: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Face17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub plane: [f64; 4],
    pub loops: Vec<Ptr>,
    pub back_material: Ptr,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ArcCurve17 {
    pub pre: Pre,
    /// Two edges of the curve before release 4 (§14,
    /// [`Release::curve_has_ends`]).
    pub ends: [Ptr; 2],
    pub head: [u8; 5],
    pub params: [f64; 14],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Curve17 {
    pub pre: Pre,
    /// Two edges of the curve before release 4 (§14,
    /// [`Release::curve_has_ends`]).
    pub ends: [Ptr; 2],
    pub b: u8,
    pub members: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InstanceClass {
    #[default]
    Component,
    Group,
    Image,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Instance17 {
    pub class: InstanceClass,
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub definition: Ptr,
    pub transform: [f64; 13],
    pub name: S16,
    pub guid: [u8; 16],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Relationship17 {
    pub pre: Pre,
    pub a: Ptr,
    pub b: Ptr,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Ftc17 {
    pub pre: Pre,
    pub zero: u32,
    pub k: [f64; 24],
    pub front_pins: Vec<[f64; 4]>,
    pub back_pins: Vec<[f64; 4]>,
    pub flags: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Font17 {
    pub pre: Pre,
    pub name: S16,
    /// bold u8, italic u8, size u32, u8, f64 (§10.1).
    pub tail: [u8; 15],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Anchor17 {
    pub kind: u32,
    pub four: u32,
    pub point: [f64; 3],
    pub entity: Ptr,
    /// A second entity; null on every vertex anchor.
    pub second: Ptr,
    /// The instances leading to `entity`.
    pub path: Vec<Ptr>,
    /// The instances leading to `second`.
    pub second_path: Vec<Ptr>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Dimension17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub text: S16,
    pub font: Ptr,
    pub aligned: u8,
    pub arrow: u32,
    pub start: Anchor17,
    pub end: Anchor17,
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
    pub u: u32,
    pub offset: f64,
    pub zero: f64,
    pub text_position: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Text17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub font: Ptr,
    pub screen: [f64; 2],
    pub anchor: Anchor17,
    pub leader_offset: [f64; 3],
    pub view_direction: [f64; 3],
    pub leader: u32,
    pub one: u32,
    pub a: u8,
    pub b: u8,
    pub arrow: u32,
    pub leader_shown: u8,
    pub content: S16,
    pub tail: [u8; 5],
}

/// A radial dimension (§10.12): a circle's or an arc's radius or
/// diameter.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RadialDimension17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub text: S16,
    pub font: Ptr,
    pub aligned: u8,
    pub arrow: u32,
    /// An edge of the measured arc or circle.
    pub edge: Ptr,
    /// The leader's angle on the curve and its end's offset (§10.12).
    pub leader: [f64; 2],
    /// 1 for a diameter, 0 for a radius.
    pub diameter: u8,
    /// The measured arc when no edge is named (§10.12): the 14 f64 of a
    /// `CArcCurve` (centre, normal, x axis, start and end angle, y axis).
    pub arc: Option<[f64; 14]>,
}

/// A 3D polyline (§10.13): a chain of points.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Polyline17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    /// Inches.
    pub points: Vec<[f64; 3]>,
}

/// A dash pattern of the line-style list (2019 and later, §14): the
/// name, the pattern in model units as text, and three numbers.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LineStyle17 {
    pub pre: Pre,
    pub zero: u16,
    pub pid2: u32,
    pub name: S16,
    pub pattern: S16,
    pub scales: [f64; 3],
}

/// A layer folder (2020, §14): its name, the folders directly in it, the
/// layers directly in it and whether it is hidden.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct LayerGroup17 {
    pub pre: Pre,
    pub name: S16,
    pub children: Vec<Ptr>,
    pub layers: Vec<Ptr>,
    pub hidden: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SectionPlane17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub plane: [f64; 4],
    /// The name and the symbol, from 2018 (§14).
    pub names: [S16; 2],
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct GuidePoint17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub point: [f64; 3],
    pub anchor: [f64; 3],
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuideLine17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    /// point, direction, the two line-parameter bounds.
    pub params: [f64; 8],
    pub tail: [u8; 4],
}

impl Default for GuideLine17 {
    /// An unbounded line, which a release without bounds (§14,
    /// [`Release::guide_has_tail`]) reads as.
    fn default() -> Self {
        GuideLine17 {
            pre: Pre::default(),
            drawbase: Drawbase::default(),
            params: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1e30, 1e30],
            tail: [0; 4],
        }
    }
}

/// The shadow settings record (§10.9).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Shadow17 {
    pub pre: Pre,
    pub time: u32,
    pub a: u8,
    pub city: S16,
    pub country: S16,
    pub longitude: f64,
    pub latitude: f64,
    pub tz_offset: f64,
    pub north: [f64; 3],
    /// u8, on faces, on ground, from edges, u32 light, u32 dark, use sun,
    /// u8.
    pub tail: [u8; 14],
}

/// A drawing element holding an origin and three axes (§10.10): the
/// model axes and a scene's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Axes17 {
    pub pre: Pre,
    pub drawbase: Drawbase,
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
    /// Whether the axes are displayed: a scene's rendering `model_axes`,
    /// which SketchUp resets from this byte; 0 on the model's axes.
    pub shown: u8,
}

impl Default for Axes17 {
    fn default() -> Self {
        Axes17 {
            pre: Pre::default(),
            drawbase: Drawbase::default(),
            origin: [0.0; 3],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
            z: [0.0, 0.0, 1.0],
            shown: 1,
        }
    }
}

/// The display settings block (§10.7): an entity preamble, then the 206
/// bytes of fields.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rendering17 {
    pub pre: Pre,
    pub fields: Blob,
}

pub(crate) const RENDERING_LEN: usize = 206;

impl Default for Rendering17 {
    fn default() -> Self {
        Rendering17 {
            pre: Pre::default(),
            fields: Blob(vec![0; RENDERING_LEN]),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scene17 {
    pub pre: Pre,
    pub name: S16,
    pub description: S16,
    pub flags: u32,
    pub camera: Ptr,
    pub rendering: Rendering17,
    pub style: Ptr,
    pub shadow: Shadow17,
    pub axes: Axes17,
    pub hidden_entities: Vec<Ptr>,
    pub hidden_layers: Vec<Ptr>,
    pub section_planes: Vec<Ptr>,
    /// The byte before `in_animation` before release 6 (§14,
    /// [`Release::scene_has_photo`]); 1 in every scene observed.
    pub one: u8,
    pub in_animation: u8,
    pub f: [f64; 2],
    /// The scene's matched photo (null on every scene without one).
    pub photo: Ptr,
    /// `01 01` when the thumbnail follows; `01 00` and `00 00` store
    /// none.
    pub thumb_flags: [u8; 2],
    pub thumb_kind: u8,
    pub thumb: Blob,
}

impl Default for Scene17 {
    fn default() -> Self {
        Scene17 {
            pre: Pre::default(),
            name: S16::default(),
            description: S16::default(),
            flags: 0,
            camera: Ptr::Null,
            rendering: Rendering17::default(),
            style: Ptr::Null,
            shadow: Shadow17::default(),
            axes: Axes17::default(),
            hidden_entities: Vec::new(),
            hidden_layers: Vec::new(),
            section_planes: Vec::new(),
            one: 1,
            in_animation: 0,
            f: [0.0; 2],
            photo: Ptr::Null,
            thumb_flags: [0; 2],
            thumb_kind: 0,
            thumb: Blob::default(),
        }
    }
}

/// One value of a style item (§10.8).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StyleVal {
    U8(u8),
    U32(u32),
    Rgba([u8; 4]),
    F64(f64),
    /// A watermark of item 5001 (null marks the model).
    Ptr(Ptr),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct StyleItem {
    pub id: u32,
    pub values: Vec<StyleVal>,
    /// Item 1013 holds this in place of values.
    pub strokes: Option<Box<Strokes17>>,
}

/// The strokes of a sketchy-edge style (§10.8): the images edges are
/// drawn with, one for each stroke length and variant.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Strokes17 {
    /// The dropout length and the fade factor (§10.8).
    pub numbers: [f64; 2],
    /// The width: 8, 12, 16 and 24 observed; the height of the images
    /// in most styles.
    pub a: u32,
    /// Halo on.
    pub b: u8,
    /// The halo length.
    pub c: u32,
    /// The images stored for each length.
    pub variants: u32,
    /// The stroke lengths, in pixels: the widths of the images.
    pub lengths: Vec<u32>,
    /// `CDib` objects, the variants of the first length first.
    pub images: Vec<Ptr>,
}

/// The style item of a sketchy-edge style's strokes.
pub(crate) const ITEM_STROKES: u32 = 1013;

/// How the message of an error starts when a file holds a structure that
/// is known and not decoded, as opposed to a file that is damaged.
pub(crate) const UNSUPPORTED: &str = "unsupported";

/// The style item that lists watermarks.
pub(crate) const ITEM_WATERMARKS: u32 = 5001;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Style17 {
    pub pre: Pre,
    pub guid: [u8; 16],
    pub empty: S16,
    pub three: u32,
    pub name: S16,
    pub description: S16,
    pub items: Vec<StyleItem>,
}

impl Default for Style17 {
    fn default() -> Self {
        Style17 {
            pre: Pre::default(),
            guid: [0; 16],
            empty: S16::default(),
            three: 3,
            name: S16::default(),
            description: S16::default(),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Watermark17 {
    pub pre: Pre,
    pub zero: u8,
    pub path: S16,
    pub time: u32,
    pub position: u32,
    pub tiled: u8,
    pub stretched: u8,
    pub keep_aspect: u8,
    pub background: u8,
    pub mask: u8,
    pub blend: f64,
    pub scale: f64,
    pub name: S16,
    pub image: Ptr,
}

/// A scene's matched photo (§10.10): the image Match Photo was set on,
/// and the points of its perspective bars.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BackgroundImage17 {
    pub pre: Pre,
    /// Where the image was loaded from.
    pub path: S16,
    pub zero: u32,
    pub image: Ptr,
    pub width: u32,
    pub height: u32,
    pub zero2: u32,
    /// UNIX seconds.
    pub time: u32,
    pub one: u8,
    pub scale: f64,
    /// Points in the image, each x and y in 0..1 from its corner and a
    /// zero: twelve in the one file observed.
    pub points: Vec<[f64; 3]>,
    pub tail: [u8; 36],
}

impl Default for BackgroundImage17 {
    fn default() -> Self {
        BackgroundImage17 {
            pre: Pre::default(),
            path: S16::default(),
            zero: 0,
            image: Ptr::Null,
            width: 0,
            height: 0,
            zero2: 0,
            time: 0,
            one: 1,
            scale: 1.0,
            points: Vec::new(),
            tail: [0; 36],
        }
    }
}

/// A classification schema stored with the model.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SchemaZip {
    pub path: S16,
    pub name: S16,
    pub data: Blob,
}

/// An archive object's body, by class.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Body {
    VersionMap(Vec<(S16, u32)>),
    Dib(Dib),
    Camera(Box<Camera17>),
    Thumbnail(Thumbnail),
    AttrContainer(AttrContainer),
    AttrNamed(AttrNamed),
    Material(Box<Material17>),
    Layer(Box<Layer17>),
    Def(Box<Def17>),
    Vertex(Vertex17),
    Edge(Box<Edge17>),
    EdgeUse(EdgeUse17),
    Loop(Loop17),
    Face(Box<Face17>),
    ArcCurve(Box<ArcCurve17>),
    Curve(Curve17),
    Instance(Box<Instance17>),
    Relationship(Relationship17),
    Ftc(Box<Ftc17>),
    Font(Font17),
    Dimension(Box<Dimension17>),
    RadialDimension(Box<RadialDimension17>),
    Polyline(Box<Polyline17>),
    Text(Box<Text17>),
    SectionPlane(Box<SectionPlane17>),
    LineStyle(Box<LineStyle17>),
    LayerGroup(Box<LayerGroup17>),
    GuidePoint(Box<GuidePoint17>),
    GuideLine(Box<GuideLine17>),
    Scene(Box<Scene17>),
    Style(Box<Style17>),
    Watermark(Box<Watermark17>),
    BackgroundImage(Box<BackgroundImage17>),
    SchemaZip(SchemaZip),
}

/// Every class a layout exists for.
const CLASSES: &[&str] = &[
    "CVersionMap",
    "CDib",
    "CCamera",
    "CThumbnail",
    "CAttributeContainer",
    "CAttributeNamed",
    "CMaterial",
    "CLayer",
    "CComponentDefinition",
    "CVertex",
    "CEdge",
    "CEdgeUse",
    "CLoop",
    "CFace",
    "CArcCurve",
    "CCurve",
    "CComponentInstance",
    "CGroup",
    "CImage",
    "CRelationship",
    "CFaceTextureCoords",
    "CSkFont",
    "CDimensionLinear",
    "CDimensionRadial",
    "CPolyline3d",
    "CText",
    "CSectionPlane",
    "CCustomLineStyle",
    "CLayerGroup",
    "CConstructionPoint",
    "CConstructionLine",
    "CViewPage",
    "CSkpStyle",
    "CWatermark",
    "CBackgroundImage",
    "CSchemaZipFile",
];

impl Body {
    /// The entity preamble of an object that has one.
    pub fn pre(&self) -> Option<&Pre> {
        Some(match self {
            Body::Thumbnail(v) => &v.pre,
            Body::AttrContainer(v) => &v.pre,
            Body::AttrNamed(v) => &v.pre,
            Body::Material(v) => &v.pre,
            Body::Layer(v) => &v.pre,
            Body::Def(v) => &v.space.pre,
            Body::Vertex(v) => &v.pre,
            Body::Edge(v) => &v.pre,
            Body::EdgeUse(v) => &v.pre,
            Body::Loop(v) => &v.pre,
            Body::Face(v) => &v.pre,
            Body::ArcCurve(v) => &v.pre,
            Body::Curve(v) => &v.pre,
            Body::Instance(v) => &v.pre,
            Body::Relationship(v) => &v.pre,
            Body::Ftc(v) => &v.pre,
            Body::Font(v) => &v.pre,
            Body::Dimension(v) => &v.pre,
            Body::RadialDimension(v) => &v.pre,
            Body::Polyline(v) => &v.pre,
            Body::Text(v) => &v.pre,
            Body::SectionPlane(v) => &v.pre,
            Body::LineStyle(v) => &v.pre,
            Body::LayerGroup(v) => &v.pre,
            Body::GuidePoint(v) => &v.pre,
            Body::GuideLine(v) => &v.pre,
            Body::Scene(v) => &v.pre,
            Body::Style(v) => &v.pre,
            Body::Watermark(v) => &v.pre,
            Body::BackgroundImage(v) => &v.pre,
            Body::VersionMap(_) | Body::Dib(_) | Body::Camera(_) | Body::SchemaZip(_) => {
                return None
            }
        })
    }

    /// The entity preamble of an object that has one, to change.
    #[cfg(test)]
    pub fn pre_mut(&mut self) -> Option<&mut Pre> {
        Some(match self {
            Body::Thumbnail(v) => &mut v.pre,
            Body::AttrContainer(v) => &mut v.pre,
            Body::AttrNamed(v) => &mut v.pre,
            Body::Material(v) => &mut v.pre,
            Body::Layer(v) => &mut v.pre,
            Body::Def(v) => &mut v.space.pre,
            Body::Vertex(v) => &mut v.pre,
            Body::Edge(v) => &mut v.pre,
            Body::EdgeUse(v) => &mut v.pre,
            Body::Loop(v) => &mut v.pre,
            Body::Face(v) => &mut v.pre,
            Body::ArcCurve(v) => &mut v.pre,
            Body::Curve(v) => &mut v.pre,
            Body::Instance(v) => &mut v.pre,
            Body::Relationship(v) => &mut v.pre,
            Body::Ftc(v) => &mut v.pre,
            Body::Font(v) => &mut v.pre,
            Body::Dimension(v) => &mut v.pre,
            Body::RadialDimension(v) => &mut v.pre,
            Body::Polyline(v) => &mut v.pre,
            Body::Text(v) => &mut v.pre,
            Body::SectionPlane(v) => &mut v.pre,
            Body::LineStyle(v) => &mut v.pre,
            Body::LayerGroup(v) => &mut v.pre,
            Body::GuidePoint(v) => &mut v.pre,
            Body::GuideLine(v) => &mut v.pre,
            Body::Scene(v) => &mut v.pre,
            Body::Style(v) => &mut v.pre,
            Body::Watermark(v) => &mut v.pre,
            Body::BackgroundImage(v) => &mut v.pre,
            Body::VersionMap(_) | Body::Dib(_) | Body::Camera(_) | Body::SchemaZip(_) => {
                return None
            }
        })
    }

    /// The interned class name, when a layout exists for it.
    pub fn static_name(name: &str) -> Option<&'static str> {
        CLASSES.iter().copied().find(|c| *c == name)
    }

    /// An empty body of a class, for the reader to fill.
    pub fn new(class: &str) -> Option<Body> {
        let inst = |class| {
            Body::Instance(Box::new(Instance17 {
                class,
                ..Default::default()
            }))
        };
        Some(match class {
            "CVersionMap" => Body::VersionMap(Vec::new()),
            "CDib" => Body::Dib(Dib::default()),
            "CCamera" => Body::Camera(Default::default()),
            "CThumbnail" => Body::Thumbnail(Default::default()),
            "CAttributeContainer" => Body::AttrContainer(Default::default()),
            "CAttributeNamed" => Body::AttrNamed(Default::default()),
            "CMaterial" => Body::Material(Default::default()),
            "CLayer" => Body::Layer(Default::default()),
            "CComponentDefinition" => Body::Def(Default::default()),
            "CVertex" => Body::Vertex(Default::default()),
            "CEdge" => Body::Edge(Default::default()),
            "CEdgeUse" => Body::EdgeUse(Default::default()),
            "CLoop" => Body::Loop(Default::default()),
            "CFace" => Body::Face(Default::default()),
            "CArcCurve" => Body::ArcCurve(Default::default()),
            "CCurve" => Body::Curve(Default::default()),
            "CComponentInstance" => inst(InstanceClass::Component),
            "CGroup" => inst(InstanceClass::Group),
            "CImage" => inst(InstanceClass::Image),
            "CRelationship" => Body::Relationship(Default::default()),
            "CFaceTextureCoords" => Body::Ftc(Default::default()),
            "CSkFont" => Body::Font(Default::default()),
            "CDimensionLinear" => Body::Dimension(Default::default()),
            "CDimensionRadial" => Body::RadialDimension(Default::default()),
            "CPolyline3d" => Body::Polyline(Default::default()),
            "CText" => Body::Text(Default::default()),
            "CSectionPlane" => Body::SectionPlane(Default::default()),
            "CCustomLineStyle" => Body::LineStyle(Default::default()),
            "CLayerGroup" => Body::LayerGroup(Default::default()),
            "CConstructionPoint" => Body::GuidePoint(Default::default()),
            "CConstructionLine" => Body::GuideLine(Default::default()),
            "CViewPage" => Body::Scene(Default::default()),
            "CSkpStyle" => Body::Style(Default::default()),
            "CWatermark" => Body::Watermark(Default::default()),
            "CBackgroundImage" => Body::BackgroundImage(Default::default()),
            "CSchemaZipFile" => Body::SchemaZip(Default::default()),
            _ => return None,
        })
    }

    pub fn class_name(&self) -> &'static str {
        match self {
            Body::VersionMap(_) => "CVersionMap",
            Body::Dib(_) => "CDib",
            Body::Camera(_) => "CCamera",
            Body::Thumbnail(_) => "CThumbnail",
            Body::AttrContainer(_) => "CAttributeContainer",
            Body::AttrNamed(_) => "CAttributeNamed",
            Body::Material(_) => "CMaterial",
            Body::Layer(_) => "CLayer",
            Body::Def(_) => "CComponentDefinition",
            Body::Vertex(_) => "CVertex",
            Body::Edge(_) => "CEdge",
            Body::EdgeUse(_) => "CEdgeUse",
            Body::Loop(_) => "CLoop",
            Body::Face(_) => "CFace",
            Body::ArcCurve(_) => "CArcCurve",
            Body::Curve(_) => "CCurve",
            Body::Instance(i) => match i.class {
                InstanceClass::Component => "CComponentInstance",
                InstanceClass::Group => "CGroup",
                InstanceClass::Image => "CImage",
            },
            Body::Relationship(_) => "CRelationship",
            Body::Ftc(_) => "CFaceTextureCoords",
            Body::Font(_) => "CSkFont",
            Body::Dimension(_) => "CDimensionLinear",
            Body::RadialDimension(_) => "CDimensionRadial",
            Body::Polyline(_) => "CPolyline3d",
            Body::Text(_) => "CText",
            Body::SectionPlane(_) => "CSectionPlane",
            Body::LineStyle(_) => "CCustomLineStyle",
            Body::LayerGroup(_) => "CLayerGroup",
            Body::GuidePoint(_) => "CConstructionPoint",
            Body::GuideLine(_) => "CConstructionLine",
            Body::Scene(_) => "CViewPage",
            Body::Style(_) => "CSkpStyle",
            Body::Watermark(_) => "CWatermark",
            Body::BackgroundImage(_) => "CBackgroundImage",
            Body::SchemaZip(_) => "CSchemaZipFile",
        }
    }
}

// ---- layouts

const MATERIAL: &[&str] = &["CMaterial"];
const LAYER: &[&str] = &["CLayer"];
const ATTRS: &[&str] = &["CAttributeContainer"];
const DIB: &[&str] = &["CDib"];
const FONT: &[&str] = &["CSkFont"];
const VERTEX: &[&str] = &["CVertex"];
const CURVES: &[&str] = &["CCurve", "CArcCurve"];
const DEFINITION: &[&str] = &["CComponentDefinition"];

fn pre(ar: &mut Ar, v: &mut Pre) -> R {
    if ar.release().has_attributes() {
        ar.ptr_to(&mut v.attrs, ATTRS)?;
    } else if !ar.reading() && v.attrs != Ptr::Null {
        return Err(Error(format!(
            "release {} stores no attributes",
            ar.release().number()
        )));
    }
    ar.pid(&mut v.pid)
}

/// A camera: an object, or its fields in place before release 4 (§14,
/// [`Release::camera_is_object`]).
fn camera_ptr(ar: &mut Ar, p: &mut Ptr) -> R {
    if ar.release().camera_is_object() {
        ar.ptr_to(p, &["CCamera"])
    } else {
        ar.inline(p, "CCamera")
    }
}

/// A constant run of bytes: whole in 2017, without its last (zero) byte
/// in a release before it (§14, [`Release::run_len`]). The bytes a
/// release does not hold stay as they are.
fn run(ar: &mut Ar, v: &mut [u8]) -> R {
    let n = ar.release().run_len(v.len());
    ar.bytes(&mut v[..n])
}

fn drawbase(ar: &mut Ar, v: &mut Drawbase) -> R {
    ar.mark(0);
    ar.ptr_to(&mut v.material, MATERIAL)?;
    ar.u8(&mut v.hidden)?;
    ar.u8(&mut v.casts_shadows)?;
    ar.u8(&mut v.receives_shadows)?;
    ar.u8(&mut v.soft)?;
    ar.u8(&mut v.smooth)?;
    if ar.release().drawbase_has_locked() {
        ar.u8(&mut v.locked)?;
    }
    ar.ptr_to(&mut v.layer, LAYER)
}

fn attr_value(ar: &mut Ar, v: &mut AttrVal, depth: u32) -> R {
    match v {
        AttrVal::Nil => Ok(()),
        AttrVal::I32(x) => {
            let mut u = *x as u32;
            ar.u32(&mut u)?;
            *x = u as i32;
            Ok(())
        }
        AttrVal::F32(x) => {
            let mut u = x.to_bits();
            ar.u32(&mut u)?;
            *x = f32::from_bits(u);
            Ok(())
        }
        AttrVal::F64(x) => ar.f64(x),
        AttrVal::Bool(x) => ar.u8(x),
        AttrVal::Time(t) => ar.u32(t),
        AttrVal::Str(s) => ar.str(s),
        AttrVal::Raw8(b) => ar.bytes(b),
        AttrVal::Vec3(p) | AttrVal::Dir3(p) => ar.f64s(p),
        AttrVal::Array(items) => {
            if depth > 8 {
                return Err(Error("attribute arrays nest deeper than 8".into()));
            }
            let n = ar.count(items.len())?;
            // Elements are added as they are read: a count is only known
            // to be honest once its elements are there.
            if ar.reading() {
                items.clear();
            }
            for i in 0..n {
                if ar.reading() {
                    items.push(AttrVal::Nil);
                }
                let e = &mut items[i];
                value_type(ar, e)?;
                attr_value(ar, e, depth + 1)?;
            }
            Ok(())
        }
    }
}

/// The type byte of a value: written from the value, or read, leaving
/// an empty value of that type to fill.
fn value_type(ar: &mut Ar, v: &mut AttrVal) -> R {
    let at = ar.pos();
    let mut t = v.type_byte();
    ar.u8(&mut t)?;
    if ar.reading() {
        *v = AttrVal::of_type(t, at)?;
    }
    Ok(())
}

/// Keys and values ended by an empty key.
fn entries(ar: &mut Ar, v: &mut Vec<Entry>) -> R {
    if ar.reading() {
        v.clear();
        loop {
            let mut key = S16::default();
            ar.str(&mut key)?;
            if key.is_empty() {
                return Ok(());
            }
            let mut value = AttrVal::Nil;
            value_type(ar, &mut value)?;
            attr_value(ar, &mut value, 0)?;
            v.push(Entry { key, value });
        }
    }
    for e in v.iter_mut() {
        if e.key.is_empty() {
            return Err(Error("an attribute key is empty".into()));
        }
        ar.str(&mut e.key)?;
        value_type(ar, &mut e.value)?;
        attr_value(ar, &mut e.value, 0)?;
    }
    ar.str(&mut S16::default())
}

fn dib(ar: &mut Ar, v: &mut Dib) -> R {
    ar.u32(&mut v.subtype)?;
    ar.mark(0);
    ar.blob(&mut v.data.0)?;
    if v.subtype == 1 && ar.release().dib_has_quality() {
        ar.u32(&mut v.quality)?;
    }
    Ok(())
}

fn camera(ar: &mut Ar, v: &mut Camera17) -> R {
    if !ar.release().camera_is_object() {
        return ar.bytes(&mut v.body[..129]);
    }
    ar.bytes(&mut v.body)?;
    ar.u16(&mut v.one)?;
    ar.str(&mut v.description)?;
    let n = ar.release().camera_tail_len();
    ar.bytes(&mut v.tail[..n])
}

fn material(ar: &mut Ar, v: &mut Material17) -> R {
    pre(ar, &mut v.pre)?;
    ar.str(&mut v.name)?;
    let at = ar.pos();
    let mut textured = v.texture.is_some() as u8;
    ar.u8(&mut textured)?;
    if ar.reading() {
        v.texture = match textured {
            0 => None,
            1 => Some(Texture17::default()),
            other => return Err(Error(format!("texture flag {other} at {at:#x}"))),
        };
    }
    match &mut v.texture {
        None => ar.u8(&mut v.solid_b)?,
        Some(t) if !ar.release().texture_image_object() => {
            let at = ar.pos();
            let mut one = t.embedded.is_some() as u8;
            ar.u8(&mut one)?;
            if one != 1 {
                return Err(Error(format!(
                    "{UNSUPPORTED}: a texture without its image, at {at:#x}"
                )));
            }
            if t.attrs != Ptr::Null || t.image != Ptr::Null {
                return Err(Error(format!(
                    "release {} holds a texture's image in place",
                    ar.release().number()
                )));
            }
            dib(ar, t.embedded.get_or_insert_with(Dib::default))?;
            ar.f64s(&mut t.size_in)?;
            ar.str(&mut t.file_name)?;
            ar.bytes(&mut t.color)?;
            ar.u8(&mut t.c)?;
        }
        Some(t) => {
            ar.ptr_to(&mut t.attrs, ATTRS)?;
            if ar.release().texture_has_byte() {
                ar.u8(&mut t.b)?;
            }
            ar.ptr_to(&mut t.image, DIB)?;
            ar.f64s(&mut t.size_in)?;
            ar.str(&mut t.file_name)?;
            ar.bytes(&mut t.color)?;
            ar.u8(&mut t.c)?;
        }
    }
    ar.bytes(&mut v.rgba)?;
    ar.str(&mut v.empty)?;
    ar.u32(&mut v.kind)?;
    ar.u32(&mut v.colorize)?;
    ar.f64(&mut v.opacity)?;
    ar.u8(&mut v.use_opacity)
}

fn layer(ar: &mut Ar, v: &mut Layer17) -> R {
    pre(ar, &mut v.pre)?;
    ar.str(&mut v.name)?;
    ar.u8(&mut v.hidden)?;
    let words = ar.release().layer_has_words();
    if words {
        ar.u16(&mut v.zero)?;
    }
    ar.pid(&mut v.pid2)?;
    ar.str(&mut v.internal)?;
    ar.bytes(&mut v.two)?;
    ar.bytes(&mut v.rgba)?;
    ar.str(&mut v.empty)?;
    ar.u32(&mut v.kind)?;
    ar.u32(&mut v.colorize)?;
    ar.f64(&mut v.opacity)?;
    ar.u8(&mut v.use_opacity)?;
    if words {
        ar.u32(&mut v.tail)?;
    }
    if ar.release().layer_has_style() {
        ar.ptr_to(&mut v.style, &["CCustomLineStyle"])?;
    } else if !ar.reading() && v.style != Ptr::Null {
        return Err(Error(format!(
            "release {} stores no layer line style",
            ar.release().number()
        )));
    }
    Ok(())
}

fn space(ar: &mut Ar, v: &mut Space) -> R {
    pre(ar, &mut v.pre)?;
    drawbase(ar, &mut v.drawbase)?;
    // The material and the layer list each open with an entity preamble
    // (§4, §5.1): absent before release 4, the attribute-container
    // pointer up to 2016.
    pre(ar, &mut v.materials_pre)?;
    ar.ptrs(&mut v.materials)?;
    ar.ptr_to(&mut v.current_material, MATERIAL)?;
    pre(ar, &mut v.layers_pre)?;
    ar.ptrs(&mut v.layers)?;
    ar.ptr_to(&mut v.active_layer, LAYER)?;
    if ar.release().space_has_groups() {
        ar.bytes(&mut v.groups.0)?;
        ar.str(&mut v.groups.1)?;
        ar.ptrs(&mut v.groups.2)?;
        ar.bytes(&mut v.groups.3)?;
    }
    ar.ptrs(&mut v.definitions)?;
    ar.mark(1);
    ar.mark_entities(true);
    ar.ptrs(&mut v.entities)?;
    ar.mark_entities(false);
    ar.ptrs(&mut v.relationships)?;
    ar.ptr_to(&mut v.active_section, &["CSectionPlane"])
}

fn definition(ar: &mut Ar, v: &mut Def17) -> R {
    space(ar, &mut v.space)?;
    ar.bytes(&mut v.guid)?;
    ar.str(&mut v.name)?;
    ar.str(&mut v.description)?;
    ar.str(&mut v.source)?;
    ar.u32(&mut v.timestamp)?;
    // The flag and the point, a zero run, then the behaviour, a u32 and
    // the kind (§7.1).
    ar.bytes(&mut v.mid[..25])?;
    let n = ar.release().space_run_len();
    ar.bytes(&mut v.mid[25..25 + n])?;
    if ar.release().definition_has_bits() {
        ar.bytes(&mut v.mid[28..35])?;
    } else {
        ar.bytes(&mut v.mid[28..34])?;
    }
    if ar.release().definition_has_extra() {
        ar.bytes(&mut v.mid[35..39])?;
    }
    ar.bytes(&mut v.mid[39..])?;
    ar.ptr_to(&mut v.thumbnail, &["CThumbnail"])
}

fn pins(ar: &mut Ar, v: &mut Vec<[f64; 4]>) -> R {
    let n = ar.count(v.len())?;
    if ar.reading() {
        if n > 4 {
            return Err(Error(format!("{n} texture pins on one side")));
        }
        v.clear();
        v.resize(n, [0.0; 4]);
    }
    for p in v.iter_mut() {
        ar.f64s(p)?;
    }
    Ok(())
}

fn anchor(ar: &mut Ar, v: &mut Anchor17) -> R {
    ar.u32(&mut v.kind)?;
    ar.u32(&mut v.four)?;
    ar.f64s(&mut v.point)?;
    ar.ptr(&mut v.entity)?;
    ar.ptr(&mut v.second)?;
    ar.ptrs(&mut v.path)?;
    ar.ptrs(&mut v.second_path)
}

/// A shadow record; the document's (`model`) holds 13 bytes after the
/// north vector, a scene's 14 (§10.9).
fn shadow(ar: &mut Ar, v: &mut Shadow17, model: bool) -> R {
    pre(ar, &mut v.pre)?;
    ar.u32(&mut v.time)?;
    ar.u8(&mut v.a)?;
    ar.str(&mut v.city)?;
    ar.str(&mut v.country)?;
    ar.f64(&mut v.longitude)?;
    ar.f64(&mut v.latitude)?;
    ar.f64(&mut v.tz_offset)?;
    ar.f64s(&mut v.north)?;
    let n = if model { 13 } else { v.tail.len() };
    ar.bytes(&mut v.tail[..n])
}

fn axes(ar: &mut Ar, v: &mut Axes17) -> R {
    pre(ar, &mut v.pre)?;
    drawbase(ar, &mut v.drawbase)?;
    ar.f64s(&mut v.origin)?;
    ar.f64s(&mut v.x)?;
    ar.f64s(&mut v.y)?;
    ar.f64s(&mut v.z)?;
    ar.u8(&mut v.shown)
}

/// The display settings: the preamble, then the release's block, the
/// parts of the 2017 one it holds ([`Release::rendering_parts`]).
fn rendering(ar: &mut Ar, v: &mut Rendering17) -> R {
    pre(ar, &mut v.pre)?;
    let n = ar.release().rendering_len();
    ar.vec(&mut v.fields.0, n)
}

fn scene(ar: &mut Ar, v: &mut Scene17) -> R {
    pre(ar, &mut v.pre)?;
    ar.str(&mut v.name)?;
    ar.str(&mut v.description)?;
    ar.u32(&mut v.flags)?;
    let f = v.flags;
    if f & 1 != 0 {
        camera_ptr(ar, &mut v.camera)?;
    }
    if f & 2 != 0 {
        rendering(ar, &mut v.rendering)?;
        // A release without styles holds no scene style (§14, 5 → 6).
        if ar.release().has_styles() {
            ar.ptr_to(&mut v.style, &["CSkpStyle"])?;
        } else if !ar.reading() && v.style != Ptr::Null {
            return Err(Error(format!(
                "release {} stores no scene style",
                ar.release().number()
            )));
        }
    }
    if f & 4 != 0 {
        shadow(ar, &mut v.shadow, false)?;
    }
    if f & 8 != 0 {
        axes(ar, &mut v.axes)?;
    }
    if f & 16 != 0 {
        ar.ptrs(&mut v.hidden_entities)?;
    }
    if f & 32 != 0 {
        ar.ptrs(&mut v.hidden_layers)?;
    }
    if f & 64 != 0 {
        ar.ptrs(&mut v.section_planes)?;
    }
    if !ar.release().scene_has_photo() {
        ar.u8(&mut v.one)?;
    }
    ar.u8(&mut v.in_animation)?;
    if ar.release().scene_has_pair() {
        ar.f64s(&mut v.f)?;
    }
    if !ar.release().scene_has_photo() {
        if !ar.reading() && v.photo != Ptr::Null {
            return Err(Error(format!(
                "release {} stores no matched photo",
                ar.release().number()
            )));
        }
        return Ok(());
    }
    ar.ptr_to(&mut v.photo, &["CBackgroundImage"])?;
    if !ar.release().scene_has_thumbnail() {
        if !ar.reading() && !v.thumb.0.is_empty() {
            return Err(Error(format!(
                "release {} stores no scene thumbnail",
                ar.release().number()
            )));
        }
        return Ok(());
    }
    ar.bytes(&mut v.thumb_flags)?;
    if v.thumb_flags == [1, 1] {
        ar.u8(&mut v.thumb_kind)?;
        ar.blob(&mut v.thumb.0)?;
    } else if !ar.reading() && !v.thumb.0.is_empty() {
        return Err(Error("a scene thumbnail needs the flags 01 01".into()));
    }
    Ok(())
}

fn style(ar: &mut Ar, v: &mut Style17) -> R {
    pre(ar, &mut v.pre)?;
    ar.bytes(&mut v.guid)?;
    ar.str(&mut v.empty)?;
    ar.u32(&mut v.three)?;
    ar.str(&mut v.name)?;
    ar.str(&mut v.description)?;
    let n = ar.count(v.items.len())?;
    if ar.reading() {
        v.items.clear();
    }
    for i in 0..n {
        if ar.reading() {
            v.items.push(StyleItem::default());
        }
        let item = &mut v.items[i];
        ar.u32(&mut item.id)?;
        if item.id == ITEM_STROKES {
            strokes(ar, item)?;
            continue;
        }
        let n = ar.count(item.values.len())?;
        if ar.reading() {
            item.values.clear();
            for _ in 0..n {
                let mut value = if item.id == ITEM_WATERMARKS {
                    StyleVal::Ptr(Ptr::Null)
                } else {
                    let at = ar.pos();
                    let mut t = 0u32;
                    ar.u32(&mut t)?;
                    match t {
                        1 => StyleVal::U8(0),
                        4 => StyleVal::U32(0),
                        5 => StyleVal::Rgba([0; 4]),
                        7 => StyleVal::F64(0.0),
                        other => {
                            return Err(Error(format!(
                                "style value type {other} at {at:#x} (item {})",
                                item.id
                            )))
                        }
                    }
                };
                style_value(ar, &mut value)?;
                item.values.push(value);
            }
        } else {
            for value in item.values.iter_mut() {
                let mut t = match value {
                    StyleVal::U8(_) => 1u32,
                    StyleVal::U32(_) => 4,
                    StyleVal::Rgba(_) => 5,
                    StyleVal::F64(_) => 7,
                    StyleVal::Ptr(_) => 0,
                };
                match (item.id == ITEM_WATERMARKS, &value) {
                    (true, StyleVal::Ptr(_)) => {}
                    (false, StyleVal::Ptr(_)) | (true, _) => {
                        return Err(Error(format!(
                            "style item {} holds a value of the wrong kind",
                            item.id
                        )))
                    }
                    (false, _) => ar.u32(&mut t)?,
                }
                style_value(ar, value)?;
            }
        }
    }
    Ok(())
}

/// Item 1013: the count of its two numbers, the numbers without a type,
/// then the stroke settings, the lengths and the images.
fn strokes(ar: &mut Ar, item: &mut StyleItem) -> R {
    let at = ar.pos();
    if ar.reading() {
        item.strokes = Some(Default::default());
    }
    let Some(v) = item.strokes.as_deref_mut() else {
        return Err(Error(format!("style item {ITEM_STROKES} holds no strokes")));
    };
    let mut two = 2u32;
    ar.u32(&mut two)?;
    if two != 2 {
        return Err(Error(format!("{two} numbers in the strokes at {at:#x}")));
    }
    ar.f64s(&mut v.numbers)?;
    ar.u32(&mut v.a)?;
    ar.u8(&mut v.b)?;
    ar.u32(&mut v.c)?;
    ar.u32(&mut v.variants)?;
    let n = ar.count(v.lengths.len())?;
    if ar.reading() {
        if n > 64 {
            return Err(Error(format!("{n} stroke lengths at {at:#x}")));
        }
        v.lengths.clear();
        v.lengths.resize(n, 0);
    }
    for l in v.lengths.iter_mut() {
        ar.u32(l)?;
    }
    let n = ar.count(v.images.len())?;
    if ar.reading() {
        if n > 4096 {
            return Err(Error(format!("{n} stroke images at {at:#x}")));
        }
        v.images.clear();
        v.images.resize(n, Ptr::Null);
    }
    for p in v.images.iter_mut() {
        ar.ptr_to(p, DIB)?;
    }
    Ok(())
}

fn style_value(ar: &mut Ar, v: &mut StyleVal) -> R {
    match v {
        StyleVal::U8(x) => ar.u8(x),
        StyleVal::U32(x) => ar.u32(x),
        StyleVal::Rgba(x) => ar.bytes(x),
        StyleVal::F64(x) => ar.f64(x),
        StyleVal::Ptr(p) => ar.ptr_to(p, &["CWatermark"]),
    }
}

fn watermark(ar: &mut Ar, v: &mut Watermark17) -> R {
    pre(ar, &mut v.pre)?;
    ar.u8(&mut v.zero)?;
    ar.str(&mut v.path)?;
    ar.u32(&mut v.time)?;
    ar.u32(&mut v.position)?;
    ar.u8(&mut v.tiled)?;
    ar.u8(&mut v.stretched)?;
    ar.u8(&mut v.keep_aspect)?;
    ar.u8(&mut v.background)?;
    ar.u8(&mut v.mask)?;
    ar.f64(&mut v.blend)?;
    ar.f64(&mut v.scale)?;
    ar.str(&mut v.name)?;
    ar.ptr_to(&mut v.image, DIB)
}

/// A scene's matched photo (§10.10). Before release 7 it holds no zero
/// words, size or time, and its image follows the points (§14,
/// [`Release::photo_has_size`]).
fn photo(ar: &mut Ar, v: &mut BackgroundImage17) -> R {
    let rel = ar.release();
    pre(ar, &mut v.pre)?;
    ar.str(&mut v.path)?;
    if rel.photo_has_size() {
        ar.u32(&mut v.zero)?;
        ar.ptr_to(&mut v.image, DIB)?;
        ar.u32(&mut v.width)?;
        ar.u32(&mut v.height)?;
        ar.u32(&mut v.zero2)?;
        ar.u32(&mut v.time)?;
    }
    ar.u8(&mut v.one)?;
    ar.f64(&mut v.scale)?;
    let n = ar.count(v.points.len())?;
    if ar.reading() {
        if n > 64 {
            return Err(Error(format!("{n} points on a matched photo")));
        }
        v.points.clear();
        v.points.resize(n, [0.0; 3]);
    }
    for p in v.points.iter_mut() {
        ar.f64s(p)?;
    }
    if !rel.photo_has_size() {
        return ar.ptr_to(&mut v.image, DIB);
    }
    let n = rel.photo_tail_len();
    ar.bytes(&mut v.tail[..n])
}

/// A dimension's font, alignment and arrow (§10.11), held from release
/// 6 ([`Release::dimension_has_font`]).
fn dimension_style(ar: &mut Ar, font: &mut Ptr, aligned: &mut u8, arrow: &mut u32) -> R {
    if !ar.release().dimension_has_font() {
        if !ar.reading() && *font != Ptr::Null {
            return Err(Error(format!(
                "release {} stores no dimension font",
                ar.release().number()
            )));
        }
        return Ok(());
    }
    ar.ptr_to(font, FONT)?;
    ar.u8(aligned)?;
    ar.u32(arrow)
}

/// The body of one object, by class.
pub(crate) fn layout(ar: &mut Ar, b: &mut Body) -> R {
    match b {
        Body::VersionMap(v) => {
            if ar.reading() {
                v.clear();
                loop {
                    let (mut name, mut schema) = (S16::default(), 0u32);
                    ar.str(&mut name)?;
                    ar.u32(&mut schema)?;
                    let end = name.text() == END_OF_VERSION_MAP;
                    v.push((name, schema));
                    if end {
                        return Ok(());
                    }
                }
            }
            if v.last().map(|e| e.0.text()) != Some(END_OF_VERSION_MAP.to_string()) {
                return Err(Error("the version map has no end marker".into()));
            }
            for (name, schema) in v.iter_mut() {
                ar.str(name)?;
                ar.u32(schema)?;
            }
            Ok(())
        }
        Body::Dib(v) => dib(ar, v),
        Body::Camera(v) => camera(ar, v),
        Body::Thumbnail(v) => {
            pre(ar, &mut v.pre)?;
            camera_ptr(ar, &mut v.camera)?;
            ar.ptr_to(&mut v.image, DIB)
        }
        Body::AttrContainer(v) => {
            pre(ar, &mut v.pre)?;
            ar.ptrs_until_null(&mut v.dictionaries)
        }
        Body::AttrNamed(v) => {
            pre(ar, &mut v.pre)?;
            ar.bytes(&mut v.four)?;
            ar.str(&mut v.name)?;
            entries(ar, &mut v.entries)?;
            if ar.release().attribute_has_tail() {
                ar.u32(&mut v.tail)?;
            }
            Ok(())
        }
        Body::Material(v) => material(ar, v),
        Body::Layer(v) => layer(ar, v),
        Body::Def(v) => definition(ar, v),
        Body::Vertex(v) => {
            pre(ar, &mut v.pre)?;
            ar.mark(0);
            ar.f64s(&mut v.p)
        }
        Body::Edge(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.ptr_to(&mut v.v0, VERTEX)?;
            ar.ptr_to(&mut v.v1, VERTEX)?;
            ar.ptr_to(&mut v.curve, CURVES)
        }
        Body::EdgeUse(v) => {
            pre(ar, &mut v.pre)?;
            ar.ptr_to(&mut v.edge, &["CEdge"])?;
            ar.u8(&mut v.dir)?;
            ar.ptr_to(&mut v.parent, &["CLoop"])
        }
        Body::Loop(v) => {
            pre(ar, &mut v.pre)?;
            ar.bytes(&mut v.c)?;
            ar.ptrs_until_null(&mut v.uses)
        }
        Body::Face(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.f64s(&mut v.plane)?;
            ar.ptrs(&mut v.loops)?;
            ar.ptr_to(&mut v.back_material, MATERIAL)
        }
        Body::ArcCurve(v) => {
            pre(ar, &mut v.pre)?;
            if ar.release().curve_has_ends() {
                ar.ptr_to(&mut v.ends[0], &["CEdge"])?;
                ar.ptr_to(&mut v.ends[1], &["CEdge"])?;
            }
            ar.bytes(&mut v.head)?;
            ar.f64s(&mut v.params)
        }
        Body::Curve(v) => {
            pre(ar, &mut v.pre)?;
            if ar.release().curve_has_ends() {
                ar.ptr_to(&mut v.ends[0], &["CEdge"])?;
                ar.ptr_to(&mut v.ends[1], &["CEdge"])?;
            }
            ar.u8(&mut v.b)?;
            ar.u32(&mut v.members)
        }
        Body::Instance(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.ptr_to(&mut v.definition, DEFINITION)?;
            ar.mark(1);
            ar.f64s(&mut v.transform)?;
            if ar.release().instance_has_name() {
                ar.str(&mut v.name)?;
            } else if !ar.reading() && !v.name.is_empty() {
                return Err(Error(format!(
                    "release {} stores no instance name",
                    ar.release().number()
                )));
            }
            if ar.release().instance_has_guid() {
                ar.bytes(&mut v.guid)?;
            }
            Ok(())
        }
        Body::Relationship(v) => {
            pre(ar, &mut v.pre)?;
            ar.ptr(&mut v.a)?;
            ar.ptr(&mut v.b)
        }
        Body::Ftc(v) => {
            pre(ar, &mut v.pre)?;
            ar.u32(&mut v.zero)?;
            ar.f64s(&mut v.k)?;
            pins(ar, &mut v.front_pins)?;
            pins(ar, &mut v.back_pins)?;
            ar.u32(&mut v.flags[0])?;
            ar.u32(&mut v.flags[1])
        }
        Body::Font(v) => {
            pre(ar, &mut v.pre)?;
            ar.str(&mut v.name)?;
            let n = ar.release().font_tail_len();
            ar.bytes(&mut v.tail[..n])
        }
        Body::Dimension(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.str(&mut v.text)?;
            dimension_style(ar, &mut v.font, &mut v.aligned, &mut v.arrow)?;
            anchor(ar, &mut v.start)?;
            anchor(ar, &mut v.end)?;
            ar.f64s(&mut v.normal)?;
            ar.f64s(&mut v.x_axis)?;
            ar.u32(&mut v.u)?;
            ar.f64(&mut v.offset)?;
            ar.f64(&mut v.zero)?;
            if ar.release().dimension_has_text_position() {
                ar.u32(&mut v.text_position)?;
            }
            Ok(())
        }
        Body::RadialDimension(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.str(&mut v.text)?;
            dimension_style(ar, &mut v.font, &mut v.aligned, &mut v.arrow)?;
            ar.ptr_to(&mut v.edge, &["CEdge"])?;
            ar.f64s(&mut v.leader)?;
            ar.u8(&mut v.diameter)?;
            if matches!(v.edge, Ptr::Null) {
                ar.f64s(v.arc.get_or_insert([0.0; 14]))?;
            } else {
                v.arc = None;
            }
            Ok(())
        }
        Body::Polyline(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            let n = ar.count(v.points.len())?;
            if ar.reading() {
                v.points.clear();
                v.points.resize(n, [0.0; 3]);
            }
            for p in v.points.iter_mut() {
                ar.f64s(p)?;
            }
            Ok(())
        }
        Body::Text(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.ptr_to(&mut v.font, FONT)?;
            ar.f64s(&mut v.screen)?;
            anchor(ar, &mut v.anchor)?;
            ar.f64s(&mut v.leader_offset)?;
            ar.f64s(&mut v.view_direction)?;
            ar.u32(&mut v.leader)?;
            ar.u32(&mut v.one)?;
            ar.u8(&mut v.a)?;
            ar.u8(&mut v.b)?;
            ar.u32(&mut v.arrow)?;
            ar.u8(&mut v.leader_shown)?;
            ar.str(&mut v.content)?;
            ar.bytes(&mut v.tail)
        }
        Body::LineStyle(v) => {
            pre(ar, &mut v.pre)?;
            if ar.release().line_style_has_second_id() {
                ar.u16(&mut v.zero)?;
                ar.pid(&mut v.pid2)?;
            }
            ar.str(&mut v.name)?;
            ar.str(&mut v.pattern)?;
            ar.f64s(&mut v.scales)
        }
        Body::LayerGroup(v) => {
            pre(ar, &mut v.pre)?;
            ar.str(&mut v.name)?;
            ar.ptrs(&mut v.children)?;
            ar.ptrs(&mut v.layers)?;
            ar.u8(&mut v.hidden)
        }
        Body::SectionPlane(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.f64s(&mut v.plane)?;
            if ar.release().section_has_names() {
                ar.str(&mut v.names[0])?;
                ar.str(&mut v.names[1])?;
            }
            Ok(())
        }
        Body::GuidePoint(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            ar.f64s(&mut v.point)?;
            ar.f64s(&mut v.anchor)?;
            ar.u8(&mut v.b)
        }
        Body::GuideLine(v) => {
            pre(ar, &mut v.pre)?;
            drawbase(ar, &mut v.drawbase)?;
            if !ar.release().guide_has_tail() {
                // Before release 4: no bounds or tail (§14).
                return ar.f64s(&mut v.params[..6]);
            }
            ar.f64s(&mut v.params)?;
            ar.bytes(&mut v.tail)
        }
        Body::Scene(v) => scene(ar, v),
        Body::Style(v) => style(ar, v),
        Body::Watermark(v) => watermark(ar, v),
        Body::BackgroundImage(v) => photo(ar, v),
        Body::SchemaZip(v) => {
            ar.str(&mut v.path)?;
            ar.str(&mut v.name)?;
            ar.blob(&mut v.data.0)
        }
    }
}

pub(crate) const END_OF_VERSION_MAP: &str = "End-Of-Version-Map";

/// One option set (§10.5, §16.12): a name and its keys.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct OptionSet {
    pub name: S16,
    pub entries: Vec<Entry>,
}

/// A stream-container file, whole.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct File17 {
    /// The release whose layouts the file holds: that of its version
    /// string when read.
    pub release: Release,
    // ---- the header (§2)
    pub doc_type: S16,
    pub version: S16,
    pub guid: [u8; 16],
    pub doc_name: S16,
    pub seed: u32,
    /// Class schemas the stream declares differently from the version
    /// map (files of release 17.0 declare `CComponentDefinition` 11).
    pub declared: Vec<(String, u16)>,
    // ---- the archive
    /// The class inventory, a `CVersionMap` object.
    pub version_map: Ptr,
    /// 2, 1200, 6, the highest persistent id, 0; a release holds the
    /// first [`Release::root_words`] of them.
    pub root: [u32; 5],
    pub thumbnail: Ptr,
    pub b15: [u8; 15],
    pub options_name: S16,
    pub options_version: u32,
    pub option_sets: Vec<OptionSet>,
    /// The `ModelProperties` dictionary's container.
    pub properties: Ptr,
    pub preview: Ptr,
    pub camera: Ptr,
    pub rendering: Rendering17,
    pub rendering_tail: [u8; 8],
    /// The model: its materials, layers, definitions and entities.
    pub space: Space,
    pub shadow: Shadow17,
    /// The scene list's entity preamble.
    pub scenes_pre: Pre,
    pub scenes: Vec<Ptr>,
    pub selected_scene: Ptr,
    pub axes: Axes17,
    pub z70: [u8; 70],
    pub dimension_font: Ptr,
    pub dimension_defaults: [u8; 61],
    pub text_font: Ptr,
    pub text_defaults: [u8; 22],
    pub screen_font: Ptr,
    pub fonts_pre: [u8; 3],
    pub fonts: Vec<Ptr>,
    /// The matched photo of the current view: a back-reference to a
    /// scene's `CBackgroundImage` (null in every file without one).
    pub view_photo: Ptr,
    /// The style list's entity preamble.
    pub styles_pre: Pre,
    pub styles: Vec<Ptr>,
    /// The dash patterns' list preamble and the patterns, from 2019 (§14).
    pub line_styles_pre: Pre,
    pub line_styles: Vec<Ptr>,
    pub active_style: Ptr,
    pub current_style: Ptr,
    pub style_changed: u32,
    /// The current view's watermark list, laid out as a style's item
    /// 5001 (null marks the model).
    pub watermarks: Vec<Ptr>,
    pub zero: u32,
    pub anti_aliased_textures: u8,
    pub schemas: Vec<Ptr>,
    /// The count that ends a file of 2018 or later (§14).
    pub tail_count: u32,
}

impl Default for File17 {
    fn default() -> Self {
        File17 {
            release: Release::R2017,
            doc_type: S16::new("SketchUp Model"),
            version: S16::new(Release::R2017.version_string()),
            guid: [0; 16],
            doc_name: S16::default(),
            seed: 0,
            declared: Vec::new(),
            version_map: Ptr::Null,
            root: [2, 1200, 6, 0, 0],
            thumbnail: Ptr::Null,
            b15: [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            options_name: S16::default(),
            options_version: 2,
            option_sets: Vec::new(),
            properties: Ptr::Null,
            preview: Ptr::Null,
            camera: Ptr::Null,
            rendering: Rendering17::default(),
            rendering_tail: [0, 0, 0, 0, 1, 0, 0, 0],
            space: Space::default(),
            shadow: Shadow17::default(),
            scenes_pre: Pre::default(),
            scenes: Vec::new(),
            selected_scene: Ptr::Null,
            axes: Axes17::default(),
            z70: [0; 70],
            dimension_font: Ptr::Null,
            dimension_defaults: [0; 61],
            text_font: Ptr::Null,
            text_defaults: [0; 22],
            screen_font: Ptr::Null,
            fonts_pre: [0; 3],
            fonts: Vec::new(),
            view_photo: Ptr::Null,
            styles_pre: Pre::default(),
            styles: Vec::new(),
            line_styles: Vec::new(),
            line_styles_pre: Pre::default(),
            active_style: Ptr::Null,
            current_style: Ptr::Null,
            style_changed: 0,
            watermarks: Vec::new(),
            zero: 0,
            anti_aliased_textures: 1,
            tail_count: 1,
            schemas: Vec::new(),
        }
    }
}

/// The option sets' version and the sets (§10.5).
fn option_sets(ar: &mut Ar, f: &mut File17) -> R {
    ar.u32(&mut f.options_version)?;
    let n = ar.count(f.option_sets.len())?;
    if ar.reading() {
        f.option_sets.clear();
    }
    for i in 0..n {
        if ar.reading() {
            f.option_sets.push(OptionSet::default());
        }
        let s = &mut f.option_sets[i];
        ar.str(&mut s.name)?;
        entries(ar, &mut s.entries)?;
    }
    Ok(())
}

/// The archive from the class inventory to the end of the file.
fn archive(ar: &mut Ar, f: &mut File17) -> R {
    let rel = ar.release();
    ar.ptr_to(&mut f.version_map, &["CVersionMap"])?;
    for v in f.root[..rel.root_words()].iter_mut() {
        ar.u32(v)?;
    }
    ar.ptr_to(&mut f.thumbnail, DIB)?;
    let n = rel.thumbnail_run_len();
    ar.bytes(&mut f.b15[..n])?;
    ar.str(&mut f.options_name)?;
    if rel.has_option_sets() {
        option_sets(ar, f)?;
        ar.ptr_to(&mut f.properties, ATTRS)?;
        ar.ptr_to(&mut f.preview, DIB)?;
    } else if !ar.reading() && (f.properties != Ptr::Null || f.preview != Ptr::Null) {
        return Err(Error(format!(
            "release {} stores no model attributes or preview",
            rel.number()
        )));
    }
    camera_ptr(ar, &mut f.camera)?;
    rendering(ar, &mut f.rendering)?;
    let n = rel.rendering_tail_len();
    ar.bytes(&mut f.rendering_tail[..n])?;
    space(ar, &mut f.space)?;
    shadow(ar, &mut f.shadow, true)?;
    // The scene list's entity preamble (§4, §5.1).
    pre(ar, &mut f.scenes_pre)?;
    ar.ptrs(&mut f.scenes)?;
    if rel.has_selected_scene() {
        ar.ptr_to(&mut f.selected_scene, &["CViewPage"])?;
    }
    axes(ar, &mut f.axes)?;
    if rel.option_sets_after_axes() {
        let n = 63 + rel.options_tail_len();
        ar.bytes(&mut f.z70[..63])?;
        option_sets(ar, f)?;
        ar.bytes(&mut f.z70[63..n])?;
    } else {
        run(ar, &mut f.z70)?;
    }
    ar.ptr_to(&mut f.dimension_font, FONT)?;
    let (a, b) = rel.dimension_defaults_range();
    ar.bytes(&mut f.dimension_defaults[a..b])?;
    ar.ptr_to(&mut f.text_font, FONT)?;
    if rel.has_screen_text() {
        ar.bytes(&mut f.text_defaults)?;
        ar.ptr_to(&mut f.screen_font, FONT)?;
    } else {
        ar.bytes(&mut f.text_defaults[..18])?;
        if !ar.reading() && f.screen_font != Ptr::Null {
            return Err(Error(format!(
                "release {} stores no screen font",
                rel.number()
            )));
        }
    }
    if rel.space_run_len() > 0 {
        run(ar, &mut f.fonts_pre)?;
    }
    ar.ptrs(&mut f.fonts)?;
    if !rel.has_styles() {
        if !ar.reading() && (!f.styles.is_empty() || f.current_style != Ptr::Null) {
            return Err(Error(format!("release {} stores no style", rel.number())));
        }
        return Ok(());
    }
    if rel.has_line_styles() {
        // The dash patterns, a list with its own preamble, come before
        // the matched photo (§14, 2018 → 2019).
        pre(ar, &mut f.line_styles_pre)?;
        ar.ptrs(&mut f.line_styles)?;
    } else if !ar.reading() && (!f.line_styles.is_empty() || f.line_styles_pre != Pre::default()) {
        return Err(Error(format!(
            "release {} stores no line styles",
            rel.number()
        )));
    }
    ar.ptr_to(&mut f.view_photo, &["CBackgroundImage"])?;
    // The style list's entity preamble (§4, §5.1).
    pre(ar, &mut f.styles_pre)?;
    ar.ptrs(&mut f.styles)?;
    ar.ptr_to(&mut f.active_style, &["CSkpStyle"])?;
    ar.ptr_to(&mut f.current_style, &["CSkpStyle"])?;
    ar.u32(&mut f.style_changed)?;
    if rel.has_view_watermarks() {
        ar.ptrs(&mut f.watermarks)?;
    } else if !ar.reading() && !f.watermarks.is_empty() {
        return Err(Error(format!(
            "release {} stores no watermark list of the view",
            rel.number()
        )));
    }
    let mut zero = f.zero.to_le_bytes();
    run(ar, &mut zero)?;
    f.zero = u32::from_le_bytes(zero);
    if !rel.has_view_watermarks() {
        return Ok(());
    }
    ar.u8(&mut f.anti_aliased_textures)?;
    if rel.has_schema_list() {
        ar.ptrs(&mut f.schemas)?;
        if rel.has_tail_count() {
            ar.u32(&mut f.tail_count)?;
        }
        Ok(())
    } else if !f.schemas.is_empty() {
        Err(Error(format!(
            "release {} holds no classification schema",
            rel.number()
        )))
    } else {
        Ok(())
    }
}

impl File17 {
    /// Read a stream-container file. Every byte is framed: a class without a layout,
    /// a reference that names no object, or bytes left after the last
    /// record are errors.
    pub fn read(d: &[u8]) -> R<File17> {
        let release = Self::release_of(d)?;
        Self::read_with(Ar::reader(d, 0, release)).map(|(f, _)| f)
    }

    /// Read a stream-container file for the model it holds: the fields between the
    /// objects as a `File17` whose every pointer is a reference, and the
    /// objects as they were collected.
    pub fn collect(d: &[u8]) -> R<(File17, crate::ar17::Collected)> {
        let release = Self::release_of(d)?;
        let (f, ar) = Self::read_with(Ar::collector(d, 0, release))?;
        let collected = ar
            .into_collected()
            .ok_or_else(|| Error("the reader collected nothing".into()))?;
        Ok((f, collected))
    }

    /// The release of a file by its header, when the grammar holds its
    /// layouts. A file of another release in the same container (2018 and
    /// later) is named as such rather than reported as damaged.
    fn release_of(d: &[u8]) -> R<Release> {
        if crate::detect_container(d) != crate::Container::Stream {
            return Err(Error("not a .skp file of the stream container".into()));
        }
        let version = crate::header::parse_header(d)
            .ok_or_else(|| Error("malformed .skp header".into()))?
            .version;
        match Release::of_version(&version) {
            Some(r) if r.grammar() => Ok(r),
            found => {
                let named = match found {
                    Some(r) => format!("release {} ({version})", r.number()),
                    None => format!("version {version}"),
                };
                Err(Error(format!(
                    "{UNSUPPORTED}: a file of {named} is read as a model, but its records \
                     are not: the grammar is that of {}",
                    Release::grammar_releases()
                )))
            }
        }
    }

    fn read_with(mut ar: Ar<'_>) -> R<(File17, Ar<'_>)> {
        let mut f = File17 {
            release: ar.release(),
            ..File17::default()
        };
        ar.str(&mut f.doc_type)?;
        ar.str(&mut f.version)?;
        ar.bytes(&mut f.guid)?;
        ar.str(&mut f.doc_name)?;
        ar.u32(&mut f.seed)?;
        archive(&mut ar, &mut f)?;
        // A collector holds the version map apart, and writes nothing;
        // any other reader has it here, or the file has none.
        if !ar.collecting() {
            let map = f.schemas()?;
            f.declared = ar
                .declared()
                .into_iter()
                .filter(|(class, schema)| {
                    class != "CVersionMap" && !map.iter().any(|(c, s)| c == class && s == schema)
                })
                .collect();
        }
        if ar.remaining() != 0 {
            return Err(Error(format!(
                "{} bytes follow the last record, at {:#x}",
                ar.remaining(),
                ar.pos()
            )));
        }
        Ok((f, ar))
    }

    /// The class schemas of the version map.
    fn schemas(&self) -> R<Vec<(String, u16)>> {
        let Some(Obj {
            body: Body::VersionMap(entries),
            ..
        }) = self.version_map.obj()
        else {
            return Err(Error("the file has no version map".into()));
        };
        entries
            .iter()
            .map(|(name, schema)| {
                u16::try_from(*schema)
                    .map(|s| (name.text(), s))
                    .map_err(|_| Error(format!("schema {schema} of {} exceeds u16", name.text())))
            })
            .collect()
    }

    /// The file's bytes. The store map is assigned here: classes are
    /// declared where they are first used and every reference is written
    /// as the slot its object received.
    pub fn write(&self) -> R<Vec<u8>> {
        let mut f = self.clone();
        let mut ar = Ar::writer(self.release);
        // The inventory itself is declared with schema 0.
        ar.set_schema("CVersionMap", 0);
        for (class, schema) in self.schemas()?.iter().chain(&self.declared) {
            ar.set_schema(class, *schema);
        }
        ar.str(&mut f.doc_type)?;
        ar.str(&mut f.version)?;
        ar.bytes(&mut f.guid)?;
        ar.str(&mut f.doc_name)?;
        ar.u32(&mut f.seed)?;
        archive(&mut ar, &mut f)?;
        Ok(ar.into_bytes())
    }
}

/// Every pointer field of a body.
pub(crate) fn body_ptrs(b: &mut Body) -> Vec<&mut Ptr> {
    fn space(s: &mut Space) -> Vec<&mut Ptr> {
        let mut v = vec![
            &mut s.pre.attrs,
            &mut s.drawbase.material,
            &mut s.drawbase.layer,
            &mut s.materials_pre.attrs,
        ];
        v.extend(s.materials.iter_mut());
        v.push(&mut s.current_material);
        v.push(&mut s.layers_pre.attrs);
        v.extend(s.layers.iter_mut());
        v.push(&mut s.active_layer);
        v.extend(s.groups.2.iter_mut());
        v.extend(s.definitions.iter_mut());
        v.extend(s.entities.iter_mut());
        v.extend(s.relationships.iter_mut());
        v.push(&mut s.active_section);
        v
    }
    fn anchor(a: &mut Anchor17) -> Vec<&mut Ptr> {
        let mut v = vec![&mut a.entity, &mut a.second];
        v.extend(a.path.iter_mut());
        v.extend(a.second_path.iter_mut());
        v
    }
    match b {
        Body::VersionMap(_) | Body::Dib(_) | Body::Camera(_) | Body::SchemaZip(_) => vec![],
        Body::Thumbnail(t) => vec![&mut t.pre.attrs, &mut t.camera, &mut t.image],
        Body::AttrContainer(c) => {
            let mut v = vec![&mut c.pre.attrs];
            v.extend(c.dictionaries.iter_mut());
            v
        }
        Body::AttrNamed(x) => vec![&mut x.pre.attrs],
        Body::Material(m) => {
            let mut v = vec![&mut m.pre.attrs];
            if let Some(t) = m.texture.as_mut() {
                v.push(&mut t.attrs);
                v.push(&mut t.image);
            }
            v
        }
        Body::Layer(x) => vec![&mut x.pre.attrs, &mut x.style],
        Body::LayerGroup(x) => {
            let mut v = vec![&mut x.pre.attrs];
            v.extend(x.children.iter_mut());
            v.extend(x.layers.iter_mut());
            v
        }
        Body::Def(d) => {
            let mut v = space(&mut d.space);
            v.push(&mut d.thumbnail);
            v
        }
        Body::Vertex(x) => vec![&mut x.pre.attrs],
        Body::Edge(e) => vec![
            &mut e.pre.attrs,
            &mut e.drawbase.material,
            &mut e.drawbase.layer,
            &mut e.v0,
            &mut e.v1,
            &mut e.curve,
        ],
        Body::EdgeUse(u) => vec![&mut u.pre.attrs, &mut u.edge, &mut u.parent],
        Body::Loop(l) => {
            let mut v = vec![&mut l.pre.attrs];
            v.extend(l.uses.iter_mut());
            v
        }
        Body::Face(x) => {
            let mut v = vec![
                &mut x.pre.attrs,
                &mut x.drawbase.material,
                &mut x.drawbase.layer,
            ];
            v.extend(x.loops.iter_mut());
            v.push(&mut x.back_material);
            v
        }
        Body::ArcCurve(x) => {
            let [a, b] = &mut x.ends;
            vec![&mut x.pre.attrs, a, b]
        }
        Body::Curve(x) => {
            let [a, b] = &mut x.ends;
            vec![&mut x.pre.attrs, a, b]
        }
        Body::Instance(x) => vec![
            &mut x.pre.attrs,
            &mut x.drawbase.material,
            &mut x.drawbase.layer,
            &mut x.definition,
        ],
        Body::Relationship(x) => vec![&mut x.pre.attrs, &mut x.a, &mut x.b],
        Body::Ftc(x) => vec![&mut x.pre.attrs],
        Body::Font(x) => vec![&mut x.pre.attrs],
        Body::Dimension(x) => {
            let mut v = vec![
                &mut x.pre.attrs,
                &mut x.drawbase.material,
                &mut x.drawbase.layer,
                &mut x.font,
            ];
            v.extend(anchor(&mut x.start));
            v.extend(anchor(&mut x.end));
            v
        }
        Body::RadialDimension(x) => vec![
            &mut x.pre.attrs,
            &mut x.drawbase.material,
            &mut x.drawbase.layer,
            &mut x.font,
            &mut x.edge,
        ],
        Body::Polyline(x) => vec![
            &mut x.pre.attrs,
            &mut x.drawbase.material,
            &mut x.drawbase.layer,
        ],
        Body::Text(x) => {
            let mut v = vec![
                &mut x.pre.attrs,
                &mut x.drawbase.material,
                &mut x.drawbase.layer,
                &mut x.font,
            ];
            v.extend(anchor(&mut x.anchor));
            v
        }
        Body::LineStyle(x) => vec![&mut x.pre.attrs],
        Body::SectionPlane(x) => vec![
            &mut x.pre.attrs,
            &mut x.drawbase.material,
            &mut x.drawbase.layer,
        ],
        Body::GuidePoint(x) => vec![
            &mut x.pre.attrs,
            &mut x.drawbase.material,
            &mut x.drawbase.layer,
        ],
        Body::GuideLine(x) => vec![
            &mut x.pre.attrs,
            &mut x.drawbase.material,
            &mut x.drawbase.layer,
        ],
        Body::Scene(x) => {
            let mut v = vec![
                &mut x.pre.attrs,
                &mut x.camera,
                &mut x.rendering.pre.attrs,
                &mut x.style,
                &mut x.shadow.pre.attrs,
                &mut x.axes.pre.attrs,
                &mut x.axes.drawbase.material,
                &mut x.axes.drawbase.layer,
            ];
            v.extend(x.hidden_entities.iter_mut());
            v.extend(x.hidden_layers.iter_mut());
            v.extend(x.section_planes.iter_mut());
            v.push(&mut x.photo);
            v
        }
        Body::Style(x) => {
            let mut v = vec![&mut x.pre.attrs];
            for item in x.items.iter_mut() {
                for val in item.values.iter_mut() {
                    if let StyleVal::Ptr(p) = val {
                        v.push(p);
                    }
                }
                if let Some(st) = item.strokes.as_deref_mut() {
                    v.extend(st.images.iter_mut());
                }
            }
            v
        }
        Body::Watermark(x) => vec![&mut x.pre.attrs, &mut x.image],
        Body::BackgroundImage(x) => vec![&mut x.pre.attrs, &mut x.image],
    }
}

/// Every pointer field of the file outside its objects.
pub(crate) fn file_ptrs(f: &mut File17) -> Vec<&mut Ptr> {
    let s = &mut f.space;
    let mut v = vec![
        &mut f.version_map,
        &mut f.thumbnail,
        &mut f.properties,
        &mut f.preview,
        &mut f.camera,
        &mut f.rendering.pre.attrs,
        &mut s.pre.attrs,
        &mut s.drawbase.material,
        &mut s.drawbase.layer,
        &mut s.materials_pre.attrs,
    ];
    v.extend(s.materials.iter_mut());
    v.push(&mut s.current_material);
    v.push(&mut s.layers_pre.attrs);
    v.extend(s.layers.iter_mut());
    v.push(&mut s.active_layer);
    v.extend(s.groups.2.iter_mut());
    v.extend(s.definitions.iter_mut());
    v.extend(s.entities.iter_mut());
    v.extend(s.relationships.iter_mut());
    v.push(&mut s.active_section);
    v.push(&mut f.shadow.pre.attrs);
    v.push(&mut f.scenes_pre.attrs);
    v.extend(f.scenes.iter_mut());
    v.push(&mut f.selected_scene);
    v.push(&mut f.axes.pre.attrs);
    v.push(&mut f.axes.drawbase.material);
    v.push(&mut f.axes.drawbase.layer);
    v.push(&mut f.dimension_font);
    v.push(&mut f.text_font);
    v.push(&mut f.screen_font);
    v.extend(f.fonts.iter_mut());
    v.push(&mut f.line_styles_pre.attrs);
    v.push(&mut f.view_photo);
    v.push(&mut f.styles_pre.attrs);
    v.extend(f.styles.iter_mut());
    v.push(&mut f.active_style);
    v.push(&mut f.current_style);
    v.extend(f.watermarks.iter_mut());
    v.extend(f.schemas.iter_mut());
    v
}

/// Every object of the file taken out into a table by id, each
/// pointer to one left as a reference.
pub(crate) fn flatten(f: &mut File17) -> std::collections::HashMap<u32, Body> {
    fn take(p: &mut Ptr, table: &mut std::collections::HashMap<u32, Body>) {
        match std::mem::take(p) {
            Ptr::New(o) => {
                let Obj { id, mut body } = *o;
                for q in body_ptrs(&mut body) {
                    take(q, table);
                }
                table.insert(id, body);
                *p = Ptr::Ref(id);
            }
            other => *p = other,
        }
    }
    let mut table = std::collections::HashMap::new();
    for p in file_ptrs(f) {
        take(p, &mut table);
    }
    table
}

/// The bytes of a flattened file ([`flatten`]): each object written
/// where the first reference to it falls, as the file's release lays
/// the file out.
pub(crate) fn write_flat(f: &File17, table: std::collections::HashMap<u32, Body>) -> R<Vec<u8>> {
    write_flat_keeping(f, table, true)
}

/// [`write_flat`] where an object no pointer reaches is an error only if
/// `all` is set; otherwise it is dropped (a file with pointers cut, to
/// stand for a release that lacks what they named).
pub(crate) fn write_flat_keeping(
    f: &File17,
    table: std::collections::HashMap<u32, Body>,
    all: bool,
) -> R<Vec<u8>> {
    let mut f = f.clone();
    let Some(Body::VersionMap(map)) = f.version_map.id().and_then(|id| table.get(&id)) else {
        return Err(Error("the file has no version map".into()));
    };
    let mut ar = Ar::writer(f.release);
    ar.set_schema("CVersionMap", 0);
    for (name, schema) in map {
        ar.set_schema(&name.text(), *schema as u16);
    }
    for (class, schema) in &f.declared {
        ar.set_schema(class, *schema);
    }
    ar.pending = table;
    ar.str(&mut f.doc_type)?;
    ar.str(&mut f.version)?;
    ar.bytes(&mut f.guid)?;
    ar.str(&mut f.doc_name)?;
    ar.u32(&mut f.seed)?;
    archive(&mut ar, &mut f)?;
    if all && !ar.pending.is_empty() {
        return Err(Error(format!(
            "{} objects of the flattened file are reached by no pointer",
            ar.pending.len()
        )));
    }
    Ok(ar.into_bytes())
}

/// The curves of a flattened file given the two edges a release before
/// 4 names in each (§14, [`Release::curve_has_ends`]): the first and the
/// last of the edges on the curve, in the order of their ids, which is
/// the stream's in a file that was read.
pub(crate) fn link_curve_ends(table: &mut std::collections::HashMap<u32, Body>) {
    let mut edges: std::collections::HashMap<u32, Vec<u32>> = std::collections::HashMap::new();
    for (id, b) in table.iter() {
        if let Body::Edge(e) = b {
            if let Some(c) = e.curve.id() {
                edges.entry(c).or_default().push(*id);
            }
        }
    }
    for (id, b) in table.iter_mut() {
        let ends = match b {
            Body::Curve(c) => &mut c.ends,
            Body::ArcCurve(c) => &mut c.ends,
            _ => continue,
        };
        if let Some(on) = edges.get_mut(id) {
            on.sort_unstable();
            *ends = [Ptr::Ref(on[0]), Ptr::Ref(on[on.len() - 1])];
        }
    }
}

/// A camera as a release before 4 holds it (§14,
/// [`Release::camera_is_object`]): the unit direction from the eye to
/// the target in place of the target.
pub(crate) fn camera_direction(c: &mut Camera17) {
    let f = |b: &[u8; 137], o: usize| f64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let eye = [f(&c.body, 0), f(&c.body, 8), f(&c.body, 16)];
    let target = [f(&c.body, 24), f(&c.body, 32), f(&c.body, 40)];
    let d = [target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    // A target at the eye has no direction: zero reads back as the eye.
    for (k, x) in d.iter().enumerate() {
        let unit = if n > 0.0 { x / n } else { 0.0 };
        c.body[24 + 8 * k..32 + 8 * k].copy_from_slice(&unit.to_le_bytes());
    }
}

/// Every attribute-container pointer of a flattened file set to null,
/// for a release without attributes (§14, [`Release::has_attributes`]).
#[cfg(test)]
pub(crate) fn drop_attributes(f: &mut File17, table: &mut std::collections::HashMap<u32, Body>) {
    for b in table.values_mut() {
        if let Some(pre) = b.pre_mut() {
            pre.attrs = Ptr::Null;
        }
        match b {
            Body::Material(m) => {
                if let Some(t) = m.texture.as_mut() {
                    t.attrs = Ptr::Null;
                }
            }
            Body::Scene(s) => {
                s.rendering.pre.attrs = Ptr::Null;
                s.shadow.pre.attrs = Ptr::Null;
                s.axes.pre.attrs = Ptr::Null;
            }
            _ => {}
        }
    }
    f.properties = Ptr::Null;
    f.rendering.pre.attrs = Ptr::Null;
    f.space.pre.attrs = Ptr::Null;
    f.shadow.pre.attrs = Ptr::Null;
    f.axes.pre.attrs = Ptr::Null;
}

/// The file as text for analysis: every record in stream order, strings
/// decoded, payloads shown by length, lists of numbers on one line.
pub(crate) fn describe(f: &File17) -> String {
    let text = format!("{f:#?}");
    let lines: Vec<&str> = text.lines().collect();
    let number = |l: &str| {
        let t = l.trim().trim_end_matches(',');
        !t.is_empty() && t.parse::<f64>().is_ok()
    };
    let mut out = String::with_capacity(text.len() / 3);
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.ends_with('[') {
            let mut j = i + 1;
            while j < lines.len() && number(lines[j]) {
                j += 1;
            }
            let closes = lines
                .get(j)
                .map(|l| l.trim().starts_with(']'))
                .unwrap_or(false);
            if closes && j > i + 1 {
                let values: Vec<&str> = lines[i + 1..j]
                    .iter()
                    .map(|l| l.trim().trim_end_matches(','))
                    .collect();
                out.push_str(line);
                out.push_str(&values.join(", "));
                out.push_str(lines[j].trim());
                out.push('\n');
                i = j + 1;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// The files to hold the layouts to: the corpus of every release the
    /// grammar holds, any local directories named by
    /// `SKPWRIGHT_WRITE_MODELS`, and the same-session saves of every
    /// release under `SKPWRIGHT_YEARS_DIR` ([`session_dirs`]; each searched
    /// two levels deep; files of releases the grammar does not hold are
    /// skipped).
    fn files() -> Vec<PathBuf> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let mut dirs = vec![root.join("third-party")];
        dirs.extend(
            Release::ALL
                .into_iter()
                .filter(|r| r.grammar())
                .map(|r| root.join(release_dir(r))),
        );
        let mut extra: Vec<PathBuf> = std::env::var_os("SKPWRIGHT_WRITE_MODELS")
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        extra.extend(session_dirs());
        for d in extra {
            for sub in subdirs(&d) {
                dirs.extend(subdirs(&sub));
                dirs.push(sub);
            }
            dirs.push(d);
        }
        let mut out = Vec::new();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for p in rd.flatten().map(|e| e.path()) {
                if p.extension().and_then(|e| e.to_str()) == Some("skp") {
                    out.push(p);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// The directory a release's files are kept in: `2016` for 2016, `8`
    /// for 8.
    fn release_dir(r: Release) -> String {
        if r.number() >= 13 {
            format!("20{}", r.number())
        } else {
            r.number().to_string()
        }
    }

    /// The directories of same-session saves `SKPWRIGHT_YEARS_DIR` names:
    /// each entry, or the `saves` and `v8run` directories inside it, or
    /// beside it when it is one of them.
    fn session_dirs() -> Vec<PathBuf> {
        let Some(var) = std::env::var_os("SKPWRIGHT_YEARS_DIR") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for d in std::env::split_paths(&var) {
            let inside: Vec<PathBuf> = ["saves", "v8run"]
                .iter()
                .map(|s| d.join(s))
                .filter(|s| s.is_dir())
                .collect();
            if !inside.is_empty() {
                out.extend(inside);
                continue;
            }
            let beside = d.parent().map(|p| p.join("v8run")).filter(|p| p.is_dir());
            out.push(d.clone());
            if d.file_name().is_some_and(|n| n == "saves") {
                out.extend(beside);
            }
        }
        out
    }

    /// The baselines of a directory of saves, with their release: its
    /// 2017 saves (`2017`, `2017b`), or, where it holds none, the saves
    /// of its newest release (`2013` in a set of releases 3 to 8).
    fn baselines(dir: &std::path::Path) -> Option<(Release, Vec<PathBuf>)> {
        let b17: Vec<PathBuf> = ["2017", "2017b"]
            .iter()
            .map(|b| dir.join(b))
            .filter(|b| b.is_dir())
            .collect();
        if !b17.is_empty() {
            return Some((Release::R2017, b17));
        }
        let r = Release::ALL
            .into_iter()
            .rev()
            .find(|r| dir.join(release_dir(*r)).is_dir())?;
        Some((r, vec![dir.join(release_dir(r))]))
    }

    fn subdirs(d: &std::path::Path) -> Vec<PathBuf> {
        std::fs::read_dir(d)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The release of a file in the container, when the grammar holds it.
    fn grammar_release(p: &std::path::Path) -> Option<Release> {
        use std::io::Read;
        // The header holds the path the model was saved from.
        let mut head = Vec::new();
        std::fs::File::open(p)
            .ok()?
            .take(0x400)
            .read_to_end(&mut head)
            .ok()?;
        if crate::detect_container(&head) != crate::Container::Stream {
            return None;
        }
        let (version, _) = crate::header_info(&head)?;
        Release::of_version(&version).filter(|r| r.grammar())
    }

    /// Every file of a release the grammar holds reads to its last byte
    /// and writes back identical, through a store map the writer assigns
    /// itself.
    #[test]
    fn every_grammar_release_file_round_trips_byte_for_byte() {
        let max = std::env::var("SKPWRIGHT_WRITE_MAX_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(u64::MAX);
        let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let (mut n, mut unsupported, mut failed) = (0, 0, Vec::new());
        let mut by_release = std::collections::BTreeMap::<u8, usize>::new();
        for p in files() {
            let Some(release) = grammar_release(&p) else {
                continue;
            };
            if std::fs::metadata(&p).map(|m| m.len() > max).unwrap_or(true) {
                continue;
            }
            let d = std::fs::read(&p).unwrap();
            let outcome = File17::read(&d).and_then(|f| f.write()).and_then(|w| {
                if w == d {
                    Ok(())
                } else {
                    let at = w
                        .iter()
                        .zip(&d)
                        .position(|(a, b)| a != b)
                        .unwrap_or(w.len().min(d.len()));
                    Err(Error(format!(
                        "written file differs at {at:#x} ({} bytes written, {} read)",
                        w.len(),
                        d.len()
                    )))
                }
            });
            n += 1;
            *by_release.entry(release.number()).or_default() += 1;
            match outcome {
                Ok(()) => {}
                // Only a file outside the corpus may hold an undecoded
                // structure.
                Err(e) if e.0.contains(UNSUPPORTED) && !p.starts_with(&corpus) => {
                    eprintln!("{}: {e}", p.display());
                    unsupported += 1;
                }
                Err(e) => failed.push(format!("{}: {e}", p.display())),
            }
        }
        assert!(n >= 90, "only {n} files found");
        eprintln!(
            "{n} files ({by_release:?} by release), {unsupported} with an undecoded structure"
        );
        for r in Release::ALL.into_iter().filter(|r| r.grammar()) {
            assert!(
                by_release.get(&r.number()).copied().unwrap_or(0) >= 9,
                "release {} has no checkpoint set",
                r.number()
            );
        }
        assert!(
            unsupported * 50 <= n,
            "{unsupported} of {n} files are unsupported"
        );
        assert!(
            failed.is_empty(),
            "{} of {n} files failed:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }

    /// The file as text with what a release may hold differently taken
    /// out: the header, the version map, the root words, every
    /// persistent id, and the display settings' x-ray opacity, which
    /// SketchUp writes as 0.5 into an older release and 0.65 into 2017
    /// (§14); and what `older` cannot hold: the instance identifiers
    /// and the classification schemas (§14, 2013).
    fn release_neutral(mut f: File17, older: Release) -> String {
        // The objects taken out, what `older` has no field for taken out
        // of them, and the file written and read again as a file of
        // `older`: every field its layouts skip is left at its default
        // and the objects are numbered as a file of that release numbers
        // them.
        let newer = f.release;
        let mut table = flatten(&mut f);
        let dibs: std::collections::HashMap<u32, Dib> = table
            .iter()
            .filter_map(|(id, b)| match b {
                Body::Dib(d) => Some((*id, d.clone())),
                _ => None,
            })
            .collect();
        let dynamic: std::collections::HashSet<u32> = table
            .iter()
            .filter_map(|(id, b)| match b {
                Body::AttrNamed(a) if a.name.text() == "dynamic_attributes" => Some(*id),
                _ => None,
            })
            .collect();
        let held = |r: &mut Rendering17| {
            let full = newer.rendering_in(&r.fields.0, &crate::build17::RENDERING);
            r.fields.0 = older.rendering_of(&full);
        };
        held(&mut f.rendering);
        if !older.has_attributes() {
            drop_attributes(&mut f, &mut table);
        }
        if older.curve_has_ends() && !newer.curve_has_ends() {
            link_curve_ends(&mut table);
        }
        for body in table.values_mut() {
            if let Body::Camera(c) = body {
                if !older.camera_is_object() && newer.camera_is_object() {
                    camera_direction(c);
                }
            }
        }
        for body in table.values_mut() {
            match body {
                Body::Scene(s) => {
                    held(&mut s.rendering);
                    if !older.scene_has_thumbnail() {
                        s.thumb_flags = [0; 2];
                        s.thumb_kind = 0;
                        s.thumb = Blob::default();
                    }
                    if !older.has_styles() {
                        s.style = Ptr::Null;
                    }
                    if !older.scene_has_photo() {
                        s.photo = Ptr::Null;
                    }
                }
                // SketchUp writes no dynamic-component attributes into a
                // file of a release before 7 (§14, 6 → 7): content, not
                // a layout.
                Body::AttrContainer(c) if !older.stores_dynamic_attributes() => {
                    c.dictionaries
                        .retain(|d| !d.id().is_some_and(|id| dynamic.contains(&id)));
                }
                // SketchUp writes a camera's aspect ratio (+129) as 0
                // into a file of a release before 6 (§14, 5 → 6): a value,
                // not a layout.
                Body::Camera(c) if !older.camera_stores_aspect() => c.body[129..137].fill(0),
                Body::Instance(v) if !older.instance_has_name() => v.name = S16::default(),
                Body::Dimension(d) if !older.dimension_has_font() => d.font = Ptr::Null,
                Body::RadialDimension(d) if !older.dimension_has_font() => d.font = Ptr::Null,
                Body::Material(m) if !older.texture_image_object() => {
                    if let Some(t) = m.texture.as_mut().filter(|t| t.embedded.is_none()) {
                        t.attrs = Ptr::Null;
                        let mut d = t.image.id().and_then(|id| dibs.get(&id)).cloned();
                        if let Some(d) = d.as_mut() {
                            d.quality = 0;
                        }
                        t.embedded = d;
                        t.image = Ptr::Null;
                    }
                }
                _ => {}
            }
        }
        if !older.has_schema_list() {
            f.schemas.clear();
        }
        if !older.has_option_sets() {
            f.properties = Ptr::Null;
            f.preview = Ptr::Null;
        }
        if !older.has_view_watermarks() {
            f.watermarks.clear();
            f.anti_aliased_textures = 1;
        }
        if !older.has_screen_text() {
            f.screen_font = Ptr::Null;
        }
        if !older.has_styles() {
            f.view_photo = Ptr::Null;
            f.styles.clear();
            f.active_style = Ptr::Null;
            f.current_style = Ptr::Null;
        }
        f.release = older;
        f.version = S16::new(older.version_string());
        let mut f = File17::read(&write_flat_keeping(&f, table, false).unwrap()).unwrap();
        let xray = |r: &mut Rendering17| {
            if let Some(x) = r.fields.0.get_mut(179..187) {
                x.fill(0);
            }
        };
        xray(&mut f.rendering);
        each_object(&mut f, &mut |o| {
            if let Body::Scene(s) = &mut o.body {
                xray(&mut s.rendering);
            }
        });
        f.release = Release::R2017;
        f.version = S16::default();
        f.guid = [0; 16];
        f.seed = 0;
        f.root = [0; 5];
        f.version_map = Ptr::Null;
        f.declared.clear();
        let text = format!("{f:?}");
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(i) = rest.find("pid") {
            let (head, tail) = rest.split_at(i);
            out.push_str(head);
            let key = if tail.starts_with("pid: ") {
                "pid: "
            } else if tail.starts_with("pid2: ") {
                "pid2: "
            } else {
                "pid"
            };
            out.push_str(key);
            rest = &tail[key.len()..];
            if key != "pid" {
                rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                out.push('_');
            }
        }
        out.push_str(rest);
        out
    }

    /// With `SKPWRIGHT_YEARS_DIR` naming the same-session saves of every
    /// release (`<release>/<name>.skp`, the 2017 baselines in `2017` and
    /// `2017b`; releases 3 to 8 beside a 2013 baseline in `v8run`): every
    /// file of an older release the grammar holds reads to the same
    /// records as its baseline save, persistent ids aside. A layout that
    /// round-trips but frames a run in the wrong place shows here. The
    /// save is the baseline of the file's own session: a session gives a
    /// style it loads a new identifier, so a file matches one baseline,
    /// the other's styles may differ.
    #[test]
    fn every_older_release_file_reads_as_its_baseline_save() {
        let (mut n, mut failed) = (0, Vec::new());
        for dir in session_dirs() {
            let Some((base, bases)) = baselines(&dir) else {
                continue;
            };
            compare_with_baselines(&dir, base, &bases, &mut n, &mut failed);
        }
        eprintln!("{n} files compared with their baseline saves");
        assert!(
            failed.is_empty(),
            "{} of {n} files differ:\n{}",
            failed.len(),
            failed.join("\n")
        );
    }

    fn compare_with_baselines(
        dir: &std::path::Path,
        base: Release,
        bases: &[PathBuf],
        n: &mut usize,
        failed: &mut Vec<String>,
    ) {
        for sub in subdirs(dir) {
            for p in std::fs::read_dir(&sub).unwrap().flatten().map(|e| e.path()) {
                let Some(r) = grammar_release(&p) else {
                    continue;
                };
                // SketchUp 3 writes a definition's edges and vertices in
                // another order than later releases, and rounds a view
                // direction its own way (§14, 3 → 4): its files are held
                // to their baseline by content (`years.rs`), not record
                // by record.
                if r >= base || !r.camera_is_object() {
                    continue;
                }
                let Ok(a) = std::fs::read(&p) else { continue };
                let twins: Vec<Vec<u8>> = bases
                    .iter()
                    .filter_map(|b| std::fs::read(b.join(p.file_name().unwrap())).ok())
                    .collect();
                if twins.is_empty() {
                    continue;
                }
                *n += 1;
                let a = match File17::read(&a) {
                    Ok(a) => release_neutral(a, r),
                    Err(e) => {
                        failed.push(format!("{}: {e}", p.display()));
                        continue;
                    }
                };
                let mut b = String::new();
                for t in &twins {
                    match File17::read(t) {
                        Ok(t) => b = release_neutral(t, r),
                        Err(e) => {
                            failed.push(format!("{} (baseline): {e}", p.display()));
                            break;
                        }
                    }
                    if a == b {
                        break;
                    }
                }
                if a != b {
                    let at = a
                        .bytes()
                        .zip(b.bytes())
                        .position(|(x, y)| x != y)
                        .unwrap_or(0);
                    let around = |t: &str| {
                        t.get(at.saturating_sub(80)..(at + 80).min(t.len()))
                            .unwrap_or("")
                            .to_string()
                    };
                    failed.push(format!(
                        "{}:\n  {}\n  {}",
                        p.display(),
                        around(&a),
                        around(&b)
                    ));
                }
            }
        }
    }

    /// A model written as a file of each release the grammar holds gives
    /// files that read to the same records, persistent ids aside: the
    /// writer changes nothing but what the release's layouts change.
    #[test]
    fn a_model_writes_alike_as_every_grammar_release() {
        let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let env = crate::WriteEnv {
            unix_time: 1_500_000_000,
            seed: 3,
        };
        let mut n = 0;
        for dir in [
            "2017", "2016", "2015", "2014", "2013", "8", "7", "6", "5", "4",
        ] {
            for p in std::fs::read_dir(corpus.join(dir))
                .unwrap()
                .flatten()
                .map(|e| e.path())
            {
                if p.extension().and_then(|e| e.to_str()) != Some("skp") {
                    continue;
                }
                let m = crate::Model::read(&p).unwrap();
                let written = |r: Release| {
                    let built = crate::env::with_env(env, || crate::build17::from_model_for(&m, r))
                        .unwrap();
                    let bytes = built.file.write().unwrap();
                    let f = File17::read(&bytes).unwrap();
                    assert_eq!(f.release, r, "{}", p.display());
                    assert!(f.write().unwrap() == bytes, "{}", p.display());
                    f
                };
                let base = written(Release::R2017);
                for r in Release::ALL
                    .into_iter()
                    .filter(|r| r.grammar() && *r < Release::R2017)
                {
                    assert!(
                        release_neutral(written(r), r) == release_neutral(base.clone(), r),
                        "{} written as {} reads otherwise than as 2017",
                        p.display(),
                        r.number()
                    );
                }
                n += 1;
            }
        }
        assert!(n >= 100, "{n} files");
    }

    /// Every object of a file, in place.
    fn each_object(f: &mut File17, visit: &mut dyn FnMut(&mut Obj)) {
        fn ptr(p: &mut Ptr, visit: &mut dyn FnMut(&mut Obj)) {
            let Ptr::New(o) = p else { return };
            visit(o);
            if let Some(pre) = o.body.pre_mut() {
                ptr(&mut pre.attrs, visit);
            }
            match &mut o.body {
                Body::AttrContainer(c) => {
                    for d in c.dictionaries.iter_mut() {
                        ptr(d, visit);
                    }
                }
                Body::Def(d) => {
                    space(&mut d.space, visit);
                    ptr(&mut d.thumbnail, visit);
                }
                Body::Thumbnail(t) => {
                    ptr(&mut t.camera, visit);
                    ptr(&mut t.image, visit);
                }
                Body::Scene(s) => ptr(&mut s.style, visit),
                Body::Layer(l) => ptr(&mut l.style, visit),
                _ => {}
            }
        }
        fn space(s: &mut Space, visit: &mut dyn FnMut(&mut Obj)) {
            for p in s.definitions.iter_mut().chain(s.entities.iter_mut()) {
                ptr(p, visit);
            }
        }
        ptr(&mut f.properties, visit);
        ptr(&mut f.rendering.pre.attrs, visit);
        ptr(&mut f.space.pre.attrs, visit);
        space(&mut f.space, visit);
        for p in f.scenes.iter_mut() {
            ptr(p, visit);
        }
    }

    /// The acceptance procedure's files (`docs/WRITING.md`, "Open
    /// questions for SketchUp"): with `SKPWRIGHT_VARIANTS_DIR` set, write
    /// there one file per question — a file SketchUp saved with that one
    /// choice of the writer applied and nothing else changed — so that a
    /// file SketchUp refuses names the choice it refuses.
    #[test]
    fn write_the_single_change_variants() {
        let Some(dir) = std::env::var_os("SKPWRIGHT_VARIANTS_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/2017");
        let read = |name: &str| File17::read(&std::fs::read(corpus.join(name)).unwrap()).unwrap();
        let flat = || Blob(crate::xml26::flat_png(128, 128, [0xF0, 0xF0, 0xF0]));
        let save = |name: &str, f: &File17| {
            let bytes = f.write().unwrap();
            assert!(
                File17::read(&bytes).unwrap().write().unwrap() == bytes,
                "{name}"
            );
            let m = crate::Model::parse(&bytes).unwrap();
            assert!(m.diagnostics.iter().all(|d| !d.is_desync()), "{name}");
            std::fs::write(dir.join(name), bytes).unwrap();
        };

        // 0. The control: SketchUp's file through the writer, unchanged.
        save("q0-control-box.skp", &read("box.skp"));

        // 1. No classification schema.
        let mut f = read("box.skp");
        f.schemas.clear();
        save("q1-no-schema-box.skp", &f);

        // 2. Flat placeholder images as the document's thumbnails.
        let mut f = read("box.skp");
        for p in [&mut f.thumbnail, &mut f.preview] {
            if let Ptr::New(o) = p {
                o.body = Body::Dib(Dib {
                    subtype: 4,
                    data: flat(),
                    quality: 0,
                });
            }
        }
        save("q2-flat-thumbnails-box.skp", &f);

        // 3. No thumbnail image on a component definition.
        let mut f = read("box.skp");
        each_object(&mut f, &mut |o| {
            if let Body::Thumbnail(t) = &mut o.body {
                t.image = Ptr::Null;
            }
        });
        save("q3-no-definition-thumbnail-box.skp", &f);

        // 4. Scenes that store no thumbnail.
        let mut f = read("two-scenes.skp");
        each_object(&mut f, &mut |o| {
            if let Body::Scene(s) = &mut o.body {
                s.thumb_flags = [1, 0];
                s.thumb_kind = 0;
                s.thumb = Blob(Vec::new());
            }
        });
        save("q4-no-scene-thumbnails-two-scenes.skp", &f);

        // 5. No attribute dictionaries on the model or its definitions.
        let mut f = read("box.skp");
        f.space.pre.attrs = Ptr::Null;
        each_object(&mut f, &mut |o| {
            if let Body::Def(d) = &mut o.body {
                d.space.pre.attrs = Ptr::Null;
            }
        });
        save("q5-no-dictionaries-box.skp", &f);

        // 7. Annotations without a material.
        let mut f = read("feature-pack.skp");
        each_object(&mut f, &mut |o| match &mut o.body {
            Body::Text(t) => t.drawbase.material = Ptr::Null,
            Body::RadialDimension(t) => t.drawbase.material = Ptr::Null,
            Body::Polyline(t) => t.drawbase.material = Ptr::Null,
            Body::Dimension(d) => d.drawbase.material = Ptr::Null,
            _ => {}
        });
        save("q7-no-annotation-material-feature-pack.skp", &f);

        // 6 and everything together: files written from the model.
        let root = corpus.join("..");
        for (name, source) in [
            ("a1-authored-empty.skp", "2017/empty.skp"),
            ("a2-authored-box.skp", "2017/box.skp"),
            ("a3-authored-box-component.skp", "2017/box-component.skp"),
            ("a4-authored-two-scenes.skp", "2017/two-scenes.skp"),
            ("a5-authored-feature-pack.skp", "2017/feature-pack.skp"),
            ("a6-authored-house.skp", "2017/house.skp"),
            ("b1-from-2026-box.skp", "2026/box.skp"),
            ("b2-from-2026-feature-pack.skp", "2026/feature-pack.skp"),
            ("b3-from-2026-house-plus.skp", "2026/house-plus.skp"),
            ("b4-from-2026-theater.skp", "third-party/theater-2026.skp"),
        ] {
            let m = crate::Model::read(root.join(source)).unwrap();
            let built = crate::build17::from_model_for(&m, Release::R2017).unwrap();
            save(name, &built.file);
        }
    }
}
