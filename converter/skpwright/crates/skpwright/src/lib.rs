//! Clean-room reader and writer for the SketchUp `.skp` format: releases
//! 3 to 8 and 2013 to 2020 (`{3.x}` to `{20.x}`, the stream container:
//! one MFC `CArchive` object stream) and the container SketchUp 2026 and
//! 2027 save (`{26.x}`, `{27.x}`: a ZIP archive of tagged records; releases
//! 2021 to 2025 use it too). Both containers yield the same [`Model`].
//! Derived solely from observed `.skp` files and
//! their COLLADA exports — no Trimble SDK. It writes both containers:
//! [`Model::write`] authors a 2026 file from a model and [`Model::write_as`]
//! a file of any release from 3 to 2020; [`Document`] and [`StreamDocument`]
//! edit a file of either container in place. See `docs/SKP_FORMAT.md` for
//! the format and `docs/SDK.md` for how to use this crate.

mod ar17;
mod build17;
mod build26;
mod build26x;
mod carchive;
pub mod ctx;
mod deflate;
mod doc17;
mod doc26;
mod edit26;
mod entity;
mod env;
mod extract;
mod features;
mod file17;
mod header;
mod integrity;
mod matwalk;
mod mesh;
mod model;
mod owners26;
mod read17;
mod read26;
mod read26x;
mod release;
mod resolve;
mod revision;
mod settings;
mod settings17;
mod settings26;
mod tree26;
mod walk;
mod walk2;
mod xml26;
mod zip;

pub use carchive::{mfc_strlen, Stall};
pub use ctx::{detect_container, Container, Ctx};
pub use doc17::StreamDocument;
pub use doc26::Document;
pub use env::WriteEnv;
pub use features::{
    Crs, CurrentStyle, Environment, EnvironmentImage, Feature, Features, HatchLine, HatchPattern,
    HatchTexture, LayerFolder, PageEnvironment, PageExtras, PbrMaterial, PbrTexture, Procedure,
    ProcedureKind, ProcedureResult, RenderHatch, Snap,
};
pub use mesh::{FaceTexture, Mesh, MeshEdge, MeshFace, Side, UvXform};
pub use tree26::{Body, Record};
// `Arc`, `Curve`, `DefinitionKind`, `GuidePoint`, `ImagePlacement` are defined below.
pub use model::{Error, Model, Node};
#[cfg(feature = "mutation")]
pub use release::mutation;
pub use release::Release;
pub use revision::{ArchiveTarget, Revision, SaveEntry};
pub use settings::{
    Anchor, Animation, Axes, Behaviour, Camera, Dimension, DimensionDefaults, Font, Leader,
    MatchPhoto, RadialDimension, RenderingOptions, Scene, SceneProperties, ShadowInfo, Strokes,
    Style, StyleItem, StyleValue, Text, TextDefaults, Units, Watermark,
};

/// Check a file of either container against everything known of its
/// format: [`Document::verify`] for a 2026 file, [`StreamDocument::verify`]
/// for a file of any release from 3 to 2020. A file that passes is
/// understood completely. A stream-container file is held to the grammar
/// alone, so one that [`StreamDocument::open`] refuses for its in-place
/// setters can still pass.
pub fn verify(d: &[u8]) -> Result<(), Error> {
    match detect_container(d) {
        Container::Zip => Document::open(d)?.verify(d),
        Container::Stream => doc17::verify(d),
        Container::Unknown => Err(Error("not a SketchUp .skp file".into())),
    }
}

/// The records of a file of either container as text:
/// [`Document::records`] or [`StreamDocument::records`].
pub fn records(d: &[u8]) -> Result<String, Error> {
    match detect_container(d) {
        Container::Zip => Document::open(d)?.records(),
        Container::Stream => doc17::records(d),
        Container::Unknown => Err(Error("not a SketchUp .skp file".into())),
    }
}

/// A file of either container written anew from its content: every
/// archive entry of a 2026 file encoded again, a stream-container file
/// written from its records. The bytes are the file's own for a file
/// SketchUp saved.
pub fn repack(d: &[u8]) -> Result<Vec<u8>, Error> {
    match detect_container(d) {
        Container::Zip => {
            let mut doc = Document::open(d)?;
            doc.repack();
            doc.write()
        }
        Container::Stream => doc17::repacked(d),
        Container::Unknown => Err(Error("not a SketchUp .skp file".into())),
    }
}

/// Header-only identification: `(version, model_guid)` — works even when the
/// container is unreadable past the fixed header, so tooling can always
/// say WHAT a file is before refusing it. The model GUID is a per-model
/// identifier the stream container's header carries (SKP_FORMAT §2);
/// it is `None` for any other container, whose header has no such field.
pub fn header_info(d: &[u8]) -> Option<(String, Option<String>)> {
    let h = header::parse_header(d)?;
    let guid = (detect_container(d) == Container::Stream).then_some(h.model_guid);
    Some((h.version, guid))
}

/// 1 metre in inches; `.skp` stores coordinates as f64 inches.
pub const INCH: f64 = 39.37007874015748;

/// What a definition is (§16.5 `0x1583`): a component, a group's
/// anonymous definition, or an imported image's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum DefinitionKind {
    #[default]
    Component,
    Group,
    Image,
}

/// A component or group definition: `{ name, guid }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub name: String,
    pub guid: String,
    pub kind: DefinitionKind,
    pub description: String,
    /// The library path the definition was loaded from (empty when none).
    pub source_path: String,
    /// Component behaviour (§7.1, §16.5).
    pub behaviour: Behaviour,
    /// UNIX time of the definition's last edit; 0 when the reader could
    /// not see it.
    pub timestamp: u32,
    /// The key instance def-refs carry: on the stream container's
    /// continuous walk the definition object's GLOBAL archive map slot
    /// (SKP_FORMAT §4s); on the 2026 path the definition's entity-container
    /// persistent id (§16.5). `None` on the byte-scan fallback (which links
    /// positionally through `Model::definition_links` instead).
    pub map_index: Option<usize>,
    /// The definition's preview image, a PNG or a JPEG, as the file
    /// holds it; a group's definition has none.
    pub thumbnail: Option<Vec<u8>>,
    /// The camera the preview was rendered with.
    pub thumbnail_camera: Option<Camera>,
    /// A point a stream-container file stores with a definition (§7.1), metres:
    /// zero in most files, and not the component's axes origin (Change
    /// Axes leaves it); its meaning is not decoded. A 2026 file has no
    /// place for it.
    pub point_m: [f64; 3],
    /// A flag both containers store with a definition (§7.1, `0x1582`),
    /// set on some definitions and clear on others; its meaning is not
    /// decoded, and a new definition takes it set.
    pub flag: bool,
}

impl Default for Definition {
    /// A new definition: a component with the flag set, as the writers
    /// wrote every definition before the flag was carried.
    fn default() -> Definition {
        Definition {
            name: String::new(),
            guid: String::new(),
            kind: DefinitionKind::Component,
            description: String::new(),
            source_path: String::new(),
            behaviour: Behaviour::default(),
            timestamp: 0,
            map_index: None,
            thumbnail: None,
            thumbnail_camera: None,
            flag: true,
            point_m: [0.0; 3],
        }
    }
}

/// A placed component/group instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    /// Persistent id of the placing object — the identity a scene's
    /// `hidden_entities` names; 0 on the byte-scan fallback.
    pub pid: u32,
    /// Name of the definition this instance places (if resolvable).
    pub definition: Option<String>,
    /// The referenced definition's declared map index (§4h; on the
    /// continuous path the definition object's GLOBAL map slot, §4s —
    /// u32: big maps pass 0x7FFF slots via the MFC big-tag escape, §4t).
    pub defref: u32,
    /// File offset of the instance's transform block — used to tell
    /// top-level instances (scene roots) from in-definition children.
    pub offset: usize,
    /// Translation in metres (row 9..11 of the transform, ÷ INCH).
    pub translation_m: [f64; 3],
    /// Full 13-f64 transform: 3×3 row-major rotation/scale + translation + w.
    pub transform: [f64; 13],
    /// Drawbase matref (§4q) — material painted on the instance, inherited
    /// by default-material faces below it. `None` on the byte-scan path
    /// (the byte-scan fallback's transform-anchored scan never sees the
    /// drawbase).
    pub material: Option<u16>,
    /// The instance's own name ("Slab", "Front Wall", …). `None` on the
    /// byte-scan fallback, which cannot see it.
    pub name: Option<String>,
    /// `Some(true)` = a `CGroup`, `Some(false)` = a `CComponentInstance`
    /// (§4s: identical layouts, distinct classes). `None` on the
    /// byte-scan fallback, which cannot tell them apart.
    pub is_group: Option<bool>,
    /// §4q drawbase hidden flag; `None` on the byte-scan fallback.
    pub hidden: Option<bool>,
    /// §4q drawbase layer slot; `None` on the byte-scan fallback.
    pub layer: Option<u16>,
    /// The placing object's GLOBAL map slot (§4s) on the stream container's
    /// continuous walk; `None` on the byte-scan fallback and on 2026 files,
    /// which have no store map.
    pub slot: Option<usize>,
    /// The placement's own 16-byte GUID, hex; empty when unknown.
    pub guid: String,
    /// Whether the placement casts and receives shadows, and whether it
    /// is locked (§5.2, §16.4); `None` on the byte-scan fallback.
    pub casts_shadows: Option<bool>,
    pub receives_shadows: Option<bool>,
    pub locked: Option<bool>,
}

/// A material. `.skp` distinguishes solid from textured by on-disk shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Material {
    Solid {
        name: String,
        rgba: [u8; 4],
        /// Effective opacity 0..1 (§8.1): one minus the stored
        /// transparency when its flag is set, else 1.0.
        opacity: f64,
        /// The colorize type (§8.1, `colorizeType`): 0 on every authored
        /// material, 1 on some library glass.
        colorize_type: u32,
    },
    Textured {
        name: String,
        texture: Option<String>,
        applied_size_in: Option<(f64, f64)>,
        /// JPEG quality setting, 0–100 (§8.1, §16.6); `None` on PNG
        /// textures and when the container does not carry it.
        jpeg_quality: Option<u32>,
        /// The material's embedded image, byte-for-byte (Phase 3.5): the
        /// inline CDib payload (PNG or JPEG).
        image_bytes: Option<Vec<u8>>,
        /// The material's colour (§8.1: the RGBA right after the texture
        /// filename) — what `.dae` exports emit as the material's diffuse:
        /// the image's average colour, or the tint of a colourised
        /// texture. API-level only, like `image_bytes` (the frozen-oracle
        /// JSON predates it).
        avg_rgba: Option<[u8; 4]>,
        /// The texture is colourised (material type 2, §8.1): it is drawn
        /// tinted with `avg_rgba`, which then differs from the image's
        /// own average.
        colorized: bool,
        /// The average colour of the image itself, as the file stores it.
        /// A release-2017 file has no place for it beside a tint: a
        /// colourised texture holds the tint in both of its colours (§8.1)
        /// and reads back with the tint here.
        texture_avg_rgba: Option<[u8; 4]>,
        /// The colorize type (§8.1, `colorizeType`).
        colorize_type: u32,
        /// Effective opacity 0..1, as for solids: one minus the stored
        /// transparency when the use-transparency flag byte is set, else
        /// 1.0 (house.skp's "[Translucent Glass Tinted]" stores 0.52
        /// flag-on and reads 0.48). API-level only, like `image_bytes`.
        opacity: f64,
    },
}

/// A model layer: name, visibility, and display colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub name: String,
    pub visible: bool,
    pub rgba: [u8; 4],
}

/// A section plane (`CSectionPlane`, §10.3) as placed in its owning
/// entity list, in that list's LOCAL frame (the model's for the root run,
/// the definition's for a definition run): the stored `[A, B, C, D]` with
/// the unit normal `(A, B, C)` and the offset `D` in inches, so a point
/// `p` (inches) lies on the plane when `A·x + B·y + C·z + D = 0`.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionPlane {
    /// Persistent id — the identity a scene's `active_section_planes`
    /// names.
    pub pid: u32,
    pub plane: [f64; 4],
    pub hidden: bool,
    /// Name and symbol (§16.4; stream releases 2018 and later, §14); empty
    /// for an earlier stream release and on the continuous walk, which does
    /// not decode them.
    pub name: String,
    pub symbol: String,
    /// The layer's archive slot (0 = the default layer); link via
    /// `Model::layer_of`.
    pub layer: u16,
    /// The material's archive slot; link via `Model::material_of`.
    pub material: Option<u16>,
    /// Whether it casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// The plane's hatch pattern (`0x4460`): an index into
    /// `Model::features.hatch_patterns`; `None` on the stream container
    /// and for a plane without one.
    pub hatch: Option<usize>,
}

impl Default for SectionPlane {
    /// A visible section plane that casts and receives shadows, as
    /// SketchUp draws one; the rest zero and empty.
    fn default() -> SectionPlane {
        SectionPlane {
            pid: 0,
            plane: [0.0; 4],
            hidden: false,
            name: String::new(),
            symbol: String::new(),
            layer: 0,
            material: None,
            casts_shadows: true,
            receives_shadows: true,
            hatch: None,
        }
    }
}

/// A curve (`CCurve` / `CArcCurve`, §6.4; §16.4 `0x1396` / `0x1397`): a
/// welded run of edges, with the arc's geometry when it is an arc. Edges
/// name their curve through [`MeshEdge::curve`].
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub pid: u32,
    /// Declared member-edge count.
    pub members: u32,
    pub arc: Option<Arc>,
}

/// An arc's geometry, in metres and radians: centre, unit normal, the
/// radius-scaled x and y axes of its plane (`y = normal × x`), the radius,
/// and the start and end angles from the x axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Arc {
    pub center_m: [f64; 3],
    pub normal: [f64; 3],
    pub x_axis_m: [f64; 3],
    pub y_axis_m: [f64; 3],
    pub radius_m: f64,
    pub start_rad: f64,
    pub end_rad: f64,
}

/// A tape-measure guide point (`CConstructionPoint`, §10.3; §16.4
/// `0x1392`): the point and the anchor it was measured from, metres, in
/// the owning run's local frame.
#[derive(Debug, Clone, PartialEq)]
pub struct GuidePoint {
    pub pid: u32,
    pub point_m: [f64; 3],
    pub anchor_m: [f64; 3],
    /// The layer's archive slot (0 = the default layer); link via
    /// `Model::layer_of`.
    pub layer: u16,
    /// The material's archive slot; link via `Model::material_of`.
    pub material: Option<u16>,
    pub hidden: bool,
    /// Whether it casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// The byte after the points (§10.3, §16.4 `0x426f`): 1 for a
    /// point from the Tape Measure, 0 in some third-party components;
    /// not decoded.
    pub flag: u8,
}

impl Default for GuidePoint {
    /// A visible guide point that casts and receives shadows, as
    /// SketchUp draws one; the rest zero.
    fn default() -> GuidePoint {
        GuidePoint {
            pid: 0,
            point_m: [0.0; 3],
            anchor_m: [0.0; 3],
            layer: 0,
            material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
            flag: 1,
        }
    }
}

/// A 3D polyline (`CPolyline3d`, §10.13; §16.4 `0x1395`): a chain of
/// points, metres, in the owning run's local frame. Neither SketchUp nor
/// its web app draws one with a standard tool; the Freehand tool with
/// Shift held does in SketchUp Make 2017.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    pub pid: u32,
    pub points_m: Vec<[f64; 3]>,
    /// The layer's archive slot (0 = the default layer); link via
    /// `Model::layer_of`.
    pub layer: u16,
    /// The material's archive slot; link via `Model::material_of`.
    pub material: Option<u16>,
    pub hidden: bool,
    /// Whether it casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
}

impl Default for Polyline {
    /// A visible polyline that casts and receives shadows; the rest
    /// zero.
    fn default() -> Polyline {
        Polyline {
            pid: 0,
            points_m: Vec::new(),
            layer: 0,
            material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
        }
    }
}

/// A placed image (§16.4 `0x1390`): an instance of an image definition
/// ([`DefinitionKind::Image`]), read from both containers (a stream-container
/// `CImage` carries an instance's layout, §10.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlacement {
    pub pid: u32,
    /// The image definition's `Definition::map_index`.
    pub defref: u32,
    pub transform: [f64; 13],
    pub hidden: bool,
    pub layer: u16,
    /// Whether it casts and receives shadows (§5.2, §16.4): a placed
    /// image receives shadows and casts none.
    pub casts_shadows: bool,
    pub receives_shadows: bool,
}

/// A construction line (guide): a point + unit direction, point in metres.
#[derive(Debug, Clone, PartialEq)]
pub struct Guide {
    /// The owning definition's `Definition::map_index`; `None` for a guide
    /// among the model's own entities. Coordinates are in the owner's
    /// local frame. `None` on the byte-scan fallback, which only finds
    /// guides by class name.
    pub def_index: Option<usize>,
    pub point_m: [f64; 3],
    pub direction: [f64; 3],
    /// Persistent id; 0 on the byte-scan fallback.
    pub pid: u32,
    /// Line-parameter bounds along `direction`, inches; ±1e30 = infinite.
    pub bounds: [f64; 2],
    /// The layer's archive slot (0 = the default layer); link via
    /// `Model::layer_of`.
    pub layer: u16,
    /// The material's archive slot; link via `Model::material_of`.
    pub material: Option<u16>,
    pub hidden: bool,
    /// Whether it casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// The dash pattern as a 16-bit mask: `0xFFFF` solid, `0x3333` dashed,
    /// `0x8888` dotted, `0xF0F0` long dashes, `0x1C47` dash-dot-dash (the
    /// four values SketchUp's patterns store; any mask round-trips).
    pub stipple: u16,
}

/// A typed attribute-dictionary value.
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Int(i32),
    F64(f64),
    Bool(bool),
}

/// A value of an attribute dictionary (§10.5, §16.8).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Nil,
    Int(i32),
    Float(f32),
    Double(f64),
    Bool(bool),
    /// Seconds since 1970-01-01 UTC (2017 type `0x09`): the time of a
    /// shadow study (`ShadowTime`), its sunrise and its sunset.
    Time(u32),
    Text(String),
    Array(Vec<Value>),
    /// Eight bytes of a type that is not decoded (2017 type `0x0c`).
    Bytes8([u8; 8]),
    /// A point or a vector (2017 type `0x11`).
    Vector([f64; 3]),
    /// A direction (2017 type `0x12`): the shadow study's `SunDirection`.
    Direction([f64; 3]),
}

/// An attribute dictionary: a named set of keys and values that an
/// object carries, as SketchUp's extensions and dynamic components store
/// their data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dictionary {
    pub name: String,
    /// The entries in the order the file holds them.
    pub entries: Vec<(String, Value)>,
    /// A 2017 field after the entries, 0 or 1, not decoded; kept for a
    /// round trip.
    pub flag: u32,
}

/// What owns a set of dictionaries: the model or an object of it, named
/// by its place in the model's lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    Model,
    /// Index into `Model::definitions`.
    Definition(usize),
    /// Index into `Model::materials`.
    Material(usize),
    /// Index into `Model::layers`.
    Layer(usize),
    /// Index into `Model::geometry`, then into the run's `mesh.faces`.
    Face(usize, usize),
    /// Into the run's `mesh.vertices` (a vertex carries dictionaries when
    /// an extension attaches data to it, as anchors on a surface do).
    Vertex(usize, usize),
    /// Into the run's `mesh.edges`.
    Edge(usize, usize),
    /// Into the run's `curves`.
    Curve(usize, usize),
    /// Into the run's `placed`.
    Instance(usize, usize),
    /// Into the run's `images`.
    Image(usize, usize),
    /// Into the run's `sections`.
    SectionPlane(usize, usize),
    /// Into the run's `guide_points`.
    GuidePoint(usize, usize),
    /// Into the run's `polylines`.
    Polyline(usize, usize),
    /// Index into `Model::guides`.
    Guide(usize),
    /// Index into `Model::texts`.
    Text(usize),
    /// Index into `Model::dimensions`.
    Dimension(usize),
    /// Index into `Model::radial_dimensions`.
    RadialDimension(usize),
    /// The document's display settings, `Model::rendering` (§10.7): a
    /// renderer keeps its own settings there.
    RenderingOptions,
    /// The document's current style settings (§16.10), kept apart from
    /// the style list.
    CurrentStyle,
    /// The model's axes, `Model::axes` (§16.10).
    Axes,
    /// The model's shadow settings, `Model::shadows` (§16.10).
    ShadowInfo,
    /// Index into the line styles of the file (§16.10): the twelve stock
    /// styles a written file holds, in their order.
    LineStyle(usize),
    /// Index into `Model::scenes`: the scene's own record (§16.10).
    Scene(usize),
    /// Index into `Model::scenes`: the scene's copy of the display
    /// settings (§16.10).
    SceneRendering(usize),
    /// A layer folder of `Model::features.layer_folders`, by its place
    /// in pre-order (§16.10): a folder before its own folders, the first
    /// top folder being 0.
    LayerFolder(usize),
    /// Index into `Model::features.hatch_patterns` (§16.10).
    HatchPattern(usize),
    /// Index into `Model::styles`: a style of the list, apart from the
    /// current settings (§16.8).
    Style(usize),
    /// Index into `Model::scenes`: the scene's copy of the shadow
    /// settings (§16.8).
    SceneShadowInfo(usize),
    /// Index into `Model::scenes`: the scene's copy of the axes (§16.8).
    SceneAxes(usize),
    /// The material list itself (§16.8).
    Materials,
    /// The layer list itself (§16.8).
    Layers,
    /// The definition list itself; the stream container does not hold
    /// its dictionaries (§16.8).
    Definitions,
    /// The scene list itself (§16.8).
    Scenes,
    /// The style list itself (§16.8).
    Styles,
    /// The line-style list itself (§16.8).
    LineStyles,
    /// The hatch-pattern list itself; 2026 container only (§16.8).
    HatchPatterns,
    /// The environment list itself; 2026 container only (§16.8).
    Environments,
}

impl Owner {
    /// A settings or list owner: the model's settings and lists rather
    /// than an object of its drawing. The 2026 container holds all of
    /// them; the stream container holds those its release has a record
    /// for (`Definitions`, `HatchPatterns`, `Environments` and
    /// `HatchPattern` in none).
    pub(crate) fn is_settings(self) -> bool {
        matches!(
            self,
            Owner::CurrentStyle
                | Owner::Axes
                | Owner::ShadowInfo
                | Owner::LineStyle(_)
                | Owner::Scene(_)
                | Owner::SceneRendering(_)
                | Owner::LayerFolder(_)
                | Owner::HatchPattern(_)
                | Owner::Style(_)
                | Owner::SceneShadowInfo(_)
                | Owner::SceneAxes(_)
                | Owner::Materials
                | Owner::Layers
                | Owner::Definitions
                | Owner::Scenes
                | Owner::Styles
                | Owner::LineStyles
                | Owner::HatchPatterns
                | Owner::Environments
        )
    }
}

/// A classification schema stored with the model (§4, §16.1): the
/// SKC archive SketchUp loaded from `source_path`, as the file holds
/// it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Classification {
    pub name: String,
    pub source_path: String,
    /// The `.skc` file, a ZIP archive.
    pub data: Vec<u8>,
}

/// The dictionaries of one owner.
#[derive(Debug, Clone, PartialEq)]
pub struct Attributes {
    pub owner: Owner,
    pub dictionaries: Vec<Dictionary>,
}

/// One attribute key/value (model options, geo-location, dynamic-component params).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub key: String,
    pub value: AttrValue,
}

/// An embedded image (`CDib`): thumbnail or material texture.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// "png" or "jpg".
    pub kind: String,
    /// Declared image length in bytes.
    pub bytes: usize,
}

/// An in-list component/group instance placement (Phase 3.3): which
/// definition it places (by declared map index — match against
/// `GeometryRun::def_index`) and its 13-f64 transform.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedInstance {
    /// Persistent id of the placing object — the identity a scene's
    /// `hidden_entities` names; 0 on the byte-scan fallback.
    pub pid: u32,
    pub defref: u32,
    pub transform: [f64; 13],
    /// `true` = a `CGroup` placement, `false` = a `CComponentInstance`
    /// (§4s: identical layouts, distinct classes — the class carries the
    /// group-vs-component identity, exactly as on [`Instance`]).
    pub is_group: bool,
    /// Drawbase matref (§4q): material painted on the instance itself,
    /// inherited by default-material faces in its subtree (0 = none).
    pub material: u16,
    /// §4q drawbase hidden flag — a hidden instance hides its subtree
    /// (exports are WYSIWYG: hidden subtrees are dropped).
    pub hidden: bool,
    /// §4q drawbase layer slot (0 = default layer); an instance on a
    /// hidden layer is not displayed/exported.
    pub layer: u16,
    /// The placing object's GLOBAL map slot (§4s) on the stream container's
    /// continuous walk; `None` on the byte-scan fallback and on 2026 files,
    /// which have no store map.
    pub slot: Option<usize>,
    /// The placement's own 16-byte GUID, hex; empty when unknown, and a
    /// writer then makes one.
    pub guid: String,
    /// Whether the placement casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// Locked against editing.
    pub locked: bool,
}

impl Default for PlacedInstance {
    /// A placement that casts and receives shadows and is not locked,
    /// as SketchUp makes one; the rest zero and empty.
    fn default() -> PlacedInstance {
        PlacedInstance {
            pid: 0,
            defref: 0,
            transform: [0.0; 13],
            is_group: false,
            material: 0,
            hidden: false,
            layer: 0,
            slot: None,
            guid: String::new(),
            casts_shadows: true,
            receives_shadows: true,
            locked: false,
        }
    }
}

/// Resolved concrete topology of one geometry run (a box → 8/12/6).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Topology {
    pub vertices: usize,
    pub edges: usize,
    pub faces: usize,
    pub loops: usize,
    pub edge_uses: usize,
    pub curves: usize,
    /// Non-mesh annotation entities in the run (Phase 6.2: house-plus's
    /// extras surface as counts, never as geometry): linear dimensions,
    /// texts, section planes, placed images, construction points,
    /// radial dimensions, 3D polylines.
    pub dimensions: usize,
    pub texts: usize,
    pub section_planes: usize,
    pub images_placed: usize,
    pub construction_points: usize,
    pub radial_dimensions: usize,
    pub polylines: usize,
}

/// One user-geometry run (a top-level drawing, or one component definition).
#[derive(Debug, Clone, Default)]
pub struct GeometryRun {
    /// File offset where the run's entity stream begins.
    pub start: usize,
    /// File offset just past the last byte the walk consumed.
    pub end: usize,
    /// Top-level entity-list elements read (edges/faces/instances — inline
    /// children like a CEdge's vertices are pool objects, not list elements).
    pub top_level: usize,
    /// The list's declared element count (`u32` immediately before the list,
    /// Phase 1.1), when plausible. `top_level == frame` means the walk
    /// consumed the list EXACTLY as framed.
    pub frame: Option<usize>,
    /// The owning object's archive map index as DECLARED by the pre-list
    /// prelude (Phase 1.2). For a component definition's run this equals the
    /// def-ref u16 its instances carry — the structural instance→definition
    /// link.
    pub def_index: Option<usize>,
    /// Resolved concrete counts (from the object pool).
    pub topology: Topology,
    /// (satisfied, total) back-reference constraints.
    pub resolved: (usize, usize),
    /// The materialized mesh (Phase 3.1): vertices in metres, faces as
    /// oriented rings, edges with flags.
    pub mesh: Mesh,
    /// In-list child instances (Phase 3.3): the run's definition places
    /// these children — the hierarchy's edges.
    pub placed: Vec<PlacedInstance>,
    /// Welded/freehand `CCurve` records in the run (§4t): the declared
    /// member-edge count of each, serialization order. Continuous-path
    /// only (the byte-scan fallback never decodes CCurve bodies).
    pub curve_members: Vec<u32>,
    /// Section planes placed in the run, serialization order (§10.3), in
    /// the run's local frame. Continuous-path only.
    pub sections: Vec<SectionPlane>,
    /// The run's curves (plain and arc), serialization order; edges point
    /// at them through [`MeshEdge::curve`]. Continuous-path and 2026 only.
    pub curves: Vec<Curve>,
    /// Tape-measure guide points placed in the run, local frame.
    pub guide_points: Vec<GuidePoint>,
    /// 3D polylines drawn in the run, local frame.
    pub polylines: Vec<Polyline>,
    /// Placed images (2026 only; a stream-container file's are counted in
    /// `Topology::images_placed` but not decoded).
    pub images: Vec<ImagePlacement>,
    /// Persistent id of the run's ACTIVE section plane — the one cutting
    /// its entity list — when one is active. 2026: every container records
    /// it (§16.4, `0x1394`, present only while a plane is active). 2017:
    /// the root tail's first pointer names the ROOT list's active plane
    /// (§4l), so it is read for the root run; a definition's own active
    /// plane is not decoded there (definition tails have another shape)
    /// and reads `None`. `None` on the byte-scan fallback.
    pub active_section: Option<u32>,
    /// The run's relationships (`CRelationship`, §7.1; §16.4 `0x639c`):
    /// pairs of related entities by persistent id, in file order. The
    /// observed ones link two group placements of a dynamic component;
    /// what the relationship means is not decoded.
    pub relationships: Vec<[u32; 2]>,
    /// The 2026 container's flag byte (§16.4 `0x139f`): bit 0 says the
    /// container holds entities and is placed from the root (or is the
    /// root), which is when SketchUp caches its bounds; a writer derives
    /// it. Bits 1 (2) and 2 (4) were seen set after a text and a section
    /// plane were first created, recording history rather than content,
    /// and are written as carried. `None` from a stream-container file.
    pub container_flags: Option<u8>,
}

/// A recorded parse anomaly (Phase 0.4). Clean files of releases 2013 to
/// 2020 parse with **zero desync diagnostics** on the continuous walk (every
/// release from 3 is read by the grammar first; `Resync`/`Truncated`/
/// `Skipped` — a standing regression test); `RunFiltered` is also recorded
/// but fires by design on
/// some clean files (thumbnail false-positive runs, dynamic-component
/// fragments), so it is informational, not a desync signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// The walk stalled mid-run and recovered at a later entity header.
    Resync {
        at: usize,
        resumed_at: usize,
        class: String,
    },
    /// The stream ended while a run was still being walked.
    Truncated { at: usize },
    /// An undecodable-but-delimited class body was skipped (Phase 0.3 rule).
    Skipped {
        class: String,
        at: usize,
        bytes: usize,
    },
    /// A candidate run was dropped by the reference filters — recorded, never
    /// silent (Phase 0.4).
    RunFiltered {
        start: usize,
        reason: RunFilterReason,
    },
    /// No plausible entity-list count u32 preceded this run (Phase 1.1) —
    /// the walk fell back to the gap/stall heuristics for its framing.
    /// Informational: junk candidate runs lack frames by nature.
    HeuristicFraming { at: usize },
    /// No structural entity-list candidate was found (Phase 1.3: the
    /// definition-list prelude and root-list "IEND" signatures both
    /// missing), so the walk anchored on the densest-cluster heuristic at
    /// this offset. Informational: the fallback is the long-validated
    /// pre-1.3 behavior.
    HeuristicAnchor { at: usize },
    /// Instance→definition linkage could not be established structurally
    /// (declared map indexes + name-record bracketing, Phase 1.2) and fell
    /// back to the sorted-defref ↔ file-order zip. Informational: the
    /// fallback is the previously validated behavior, but it is positional,
    /// not structural.
    HeuristicLinkage { definitions: usize },
    /// The file was read whole by its grammar (SKP_FORMAT §2–§10): every
    /// record from the header to the last byte, each in the place the
    /// format gives it. Nothing of the model was searched for.
    /// Informational — the path marker of the release 17 reader; a
    /// `ContinuousWalk` with the count of the slots before the model
    /// comes with it.
    WholeFile,
    /// The grammar did not read this release 17 file, for the reason
    /// given — a structure that is not decoded (§15) among them — and
    /// the walk served it. Informational.
    WholeFileRefused { detail: String },
    /// The CONTINUOUS single-archive walk (SKP_FORMAT §4s) parsed this file:
    /// `base` pre-model slots were calibrated, and the model section walked
    /// with zero desyncs (a continuous attempt either completes exactly or
    /// is abandoned). Informational — the path marker.
    ContinuousWalk { base: usize },
    /// The continuous walk was attempted and died at `at` during `stage`
    /// (anchor/calibration/layer-list/definitions/trailing-defs/root-list/
    /// validation); the byte-scan fallback served the file instead.
    /// Informational: the fallback IS the long-validated behavior.
    ContinuousFallback {
        stage: String,
        at: usize,
        detail: String,
    },
    /// A pre-model pad slot was bound to a class by its read-site
    /// expectation during the continuous walk (§4s context-directed slot
    /// binding: thumbnail children → CCamera/CDib, entity attribute
    /// pointers → CAttributeContainer, container children →
    /// CAttributeNamed). Informational: calibration evidence, never silent.
    PadSlotBound { slot: usize, class: String },
    /// The §4s material slot arithmetic (first slot = base − (M + D) + 1,
    /// dibs interleaved after their material) disagreed with the observed
    /// face matrefs or the manager's declared count, so face→material
    /// linkage fell back to the documented §4n suffix alignment.
    MaterialLinkFallback { declared: usize, extracted: usize },
    /// `count` shared-texture materials' back-refs (§8.1) could not be
    /// resolved to their owning material's image bytes and keep
    /// `image_bytes: None` — the anchor an owner lookup needs was missing
    /// (the §4n suffix fallback placed no slots, or the file took the
    /// byte-scan fallback, which never establishes the §4s global
    /// anchor) or a specific owner lookup came up empty. Informational: the
    /// loss is recorded on `Model::diagnostics`, like every other fallback
    /// here, rather than left silent.
    UnresolvedSharedTexture { count: usize },
    /// A 2026-container file whose header revision byte (§16.27) is not
    /// the revision `meta/meta.dat` holds; the model takes the latter.
    /// SketchUp 2027 opens such a file (`corpus/2027/rev-hdr-99.skp`).
    /// Informational.
    RevisionMismatch { meta: u16, header: u8 },
    /// A 2026-container file of a revision newer than any this crate
    /// knows (§16.27): read as `read_as`, the newest known. The record
    /// tree describes itself, so the records of the newer revision that
    /// the reader does not know are skipped: `unknown_records` of them,
    /// counted where the reader checks for unknown tags. Informational.
    NewerRevision {
        revision: u16,
        read_as: u16,
        unknown_records: usize,
    },
    /// `meta/meta.dat` of a 2026-container file is absent, damaged or
    /// lacks a record the reader takes the revision or the save history
    /// from (the revision then falls back to the header byte, the history
    /// to what could be read). One diagnostic per problem. Informational:
    /// the model's content is not affected.
    MalformedMeta { detail: String },
}

/// Why a candidate run was filtered out of [`geometry_runs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunFilterReason {
    /// No vertices and no edges — a thumbnail/scan false positive.
    Degenerate,
    /// Back-reference resolution below the 75% confidence floor.
    LowConfidence {
        satisfied: usize,
        constraints: usize,
    },
}

impl Diagnostic {
    /// Desync diagnostics indicate the walker lost and re-found (or lost and
    /// gave up on) the stream — never expected on a clean file of releases
    /// 2013 to 2020.
    pub fn is_desync(&self) -> bool {
        !matches!(
            self,
            Diagnostic::RunFiltered { .. }
                | Diagnostic::HeuristicFraming { .. }
                | Diagnostic::HeuristicLinkage { .. }
                | Diagnostic::HeuristicAnchor { .. }
                | Diagnostic::WholeFile
                | Diagnostic::WholeFileRefused { .. }
                | Diagnostic::ContinuousWalk { .. }
                | Diagnostic::ContinuousFallback { .. }
                | Diagnostic::PadSlotBound { .. }
                | Diagnostic::MaterialLinkFallback { .. }
                | Diagnostic::UnresolvedSharedTexture { .. }
                | Diagnostic::RevisionMismatch { .. }
                | Diagnostic::NewerRevision { .. }
                | Diagnostic::MalformedMeta { .. }
        )
    }
}

/// Walk and resolve every user-geometry run, applying the reference parser's
/// filters (drop degenerate + low-confidence runs).
pub fn geometry_runs(d: &[u8]) -> Vec<GeometryRun> {
    geometry_runs_with_diagnostics(d).0
}

/// [`geometry_runs`] plus every recorded [`Diagnostic`] (Phase 0.4): per-run
/// resync/truncation/skip events and the run-filter decisions.
pub fn geometry_runs_with_diagnostics(d: &[u8]) -> (Vec<GeometryRun>, Vec<Diagnostic>) {
    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    for run in walk::walk_definitions(d) {
        // A run the filters DROP was a false-positive candidate (the scan
        // over-generates by design — thumbnails, dynamic-component
        // fragments); its internal stall/resync churn is noise about
        // non-geometry bytes, so only the RunFiltered record survives.
        // Desync diagnostics are surfaced from runs that pass the filters —
        // there they describe real geometry the walk lost and re-found.
        let (vertices, edges) = (run.count("CVertex"), run.count("CEdge"));
        if vertices == 0 && edges == 0 {
            diagnostics.push(Diagnostic::RunFiltered {
                start: run.start,
                reason: RunFilterReason::Degenerate,
            });
            continue;
        }
        let res = resolve::resolve(&run.map, &run.objs);
        let (sat, con) = (res.satisfied, res.constraints);
        // Phase 0.6: with the template-pid filter gone, thumbnail-PNG bytes
        // can fake a list prelude and yield a tiny UNFRAMED run whose
        // handful of "objects" carry zero back-ref constraints — nothing
        // vouches for it. Real runs are framed (or, like attributes.skp's
        // fragments, carry constraints).
        if run.frame.is_none() && con == 0 && vertices + edges + run.count("CFace") < 4 {
            diagnostics.push(Diagnostic::RunFiltered {
                start: run.start,
                reason: RunFilterReason::Degenerate,
            });
            continue;
        }
        if con > 0 && (sat as f64) < 0.75 * con as f64 {
            diagnostics.push(Diagnostic::RunFiltered {
                start: run.start,
                reason: RunFilterReason::LowConfidence {
                    satisfied: sat,
                    constraints: con,
                },
            });
            continue;
        }
        diagnostics.extend(run.diagnostics.iter().cloned());
        out.push(GeometryRun {
            start: run.start,
            end: run.end,
            top_level: run.objs.len(),
            frame: run.frame,
            def_index: run.def_index,
            topology: Topology {
                vertices,
                edges,
                faces: run.count("CFace"),
                loops: run.count("CLoop"),
                edge_uses: run.count("CEdgeUse"),
                curves: run.count("CArcCurve"),
                dimensions: run.count("CDimensionLinear"),
                texts: run.count("CText"),
                section_planes: run.count("CSectionPlane"),
                images_placed: run.count("CImage"),
                construction_points: run.count("CConstructionPoint"),
                radial_dimensions: run.count("CDimensionRadial"),
                polylines: run.count("CPolyline3d"),
            },
            resolved: (sat, con),
            mesh: mesh::build(&run.map, res.base),
            placed: run
                .map
                .iter()
                .filter_map(|s| match s {
                    carchive::Slot::Object(entity::Entity::InstancePlaced {
                        pid,
                        defref,
                        transform,
                        material,
                        hidden,
                        layer,
                        is_group,
                        more,
                        ..
                    }) => Some(PlacedInstance {
                        pid: *pid,
                        slot: None,
                        defref: *defref,
                        transform: **transform,
                        material: *material,
                        hidden: *hidden,
                        layer: *layer,
                        is_group: *is_group,
                        guid: more.guid.iter().map(|x| format!("{x:02x}")).collect(),
                        casts_shadows: more.casts_shadows,
                        receives_shadows: more.receives_shadows,
                        locked: more.locked,
                    }),
                    _ => None,
                })
                .collect(),
            curve_members: Vec::new(), // byte-scan fallback never decodes CCurve
            sections: Vec::new(),
            curves: Vec::new(),
            guide_points: Vec::new(),
            polylines: Vec::new(),
            images: Vec::new(),
            active_section: None,
            relationships: vec![],
            container_flags: None,
        });
    }
    (out, diagnostics)
}

/// Enumerate every class in a file via its `CVersionMap`: `(classname, schema)`.
/// Works on any file (the version map is written up front).
pub fn inventory(d: &[u8]) -> Result<Vec<(String, u32)>, Stall> {
    let start = header::object_stream_start(d).ok_or_else(|| Stall_new("<no header>"))?;
    let mut ar = carchive::CArchive::new(d, start);
    let vm = ar.read_object()?;
    if let carchive::Child::Obj(idx) = vm {
        if let carchive::Slot::Object(entity::Entity::VersionMap { entries }) = &ar.map[idx] {
            return Ok(entries.clone());
        }
    }
    Err(Stall_new("<no version map>"))
}

#[allow(non_snake_case)]
fn Stall_new(msg: &str) -> Stall {
    Stall {
        class: msg.to_string(),
        pos: 0,
        mapindex: 0,
        expected: None,
    }
}
