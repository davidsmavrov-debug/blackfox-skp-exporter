//! Typed document settings, shared by the stream-container and 2026 readers:
//! camera, rendering options, shadows, units, styles, watermarks and
//! scenes (SKP_FORMAT §10.6–§10.10 for the 2017 records, §16.9–§16.15
//! for the 2026 ones). Both readers fill the same structs, and
//! `tests/read26.rs` holds every converted corpus file to its original.

use crate::INCH;

/// A camera (§10.6, §16.9). Positions are in metres, as mesh vertices are.
#[derive(Debug, Clone, PartialEq)]
pub struct Camera {
    pub eye_m: [f64; 3],
    pub target_m: [f64; 3],
    /// Unit up vector.
    pub up: [f64; 3],
    pub perspective: bool,
    /// Vertical field of view in degrees (perspective cameras).
    pub fov_deg: f64,
    /// View height in metres (parallel-projection cameras).
    pub parallel_height_m: f64,
    pub two_point_perspective: bool,
    pub description: String,
    /// The two f64 at body +72 and +80 (§10.6; §16.9 `0x34c0` /
    /// `0x34c1`): 1.0 and 1000.0 in a view's camera, 0 and 0 in a
    /// definition thumbnail's; meaning undecoded.
    pub undecoded: [f64; 2],
    /// The width-to-height ratio the view is held to, as Match Photo
    /// sets it to its image's; 0 when the view follows the window.
    pub aspect_ratio: f64,
    /// The 2D zoom of a two-point perspective view (Zoom with the
    /// camera in two-point perspective): 1 when unzoomed.
    pub scale_2d: f64,
    /// The 2D pan of a two-point perspective view, as fractions of the
    /// view: 0, 0 when unpanned.
    pub offset_2d: [f64; 2],
    /// The width of the view in model units (inches) that a camera set to
    /// an image width holds (`0x34c9`; in a stream-container file the first f64 of the camera tail; 0 while
    /// unset).
    pub image_width: f64,
}

impl Camera {
    /// Decode the 2017 `CCamera` body at `body` (137 bytes) and its 33-byte
    /// tail at `tail`; `description` is the string between them.
    pub(crate) fn from_2017(
        d: &[u8],
        body: usize,
        tail: usize,
        description: String,
    ) -> Option<Camera> {
        let f = |o: usize| -> Option<f64> {
            Some(f64::from_le_bytes(d.get(o..o + 8)?.try_into().ok()?))
        };
        let v3 = |o: usize| -> Option<[f64; 3]> { Some([f(o)?, f(o + 8)?, f(o + 16)?]) };
        let m = |v: [f64; 3]| [v[0] / INCH, v[1] / INCH, v[2] / INCH];
        Some(Camera {
            eye_m: m(v3(body)?),
            target_m: m(v3(body + 24)?),
            up: v3(body + 48)?,
            perspective: *d.get(body + 88)? != 0,
            fov_deg: f(body + 89)?,
            parallel_height_m: f(body + 97)? / INCH,
            two_point_perspective: *d.get(tail + 8)? != 0,
            description,
            undecoded: [f(body + 72)?, f(body + 80)?],
            aspect_ratio: f(body + 129)?,
            scale_2d: f(tail + 9)?,
            offset_2d: [f(tail + 17)?, f(tail + 25)?],
            image_width: f(tail)?,
        })
    }
}

/// One rendering-option field: its 2026 record tag, its 2017 offset in
/// the §10.7 block (`None` for fields the 2017 block lacks) and the style
/// document item id that carries it (`None` for fields no style holds).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RoField {
    pub name: &'static str,
    pub kind: RoKind,
    pub tag: u16,
    pub off17: Option<usize>,
    pub item: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoKind {
    Bool,
    U32,
    Rgba,
    F64,
    /// A 4-byte float (2026 records and style items of variant type 6).
    F32,
}

/// A decoded value for one rendering-option field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RoValue {
    Bool(bool),
    U32(u32),
    Rgba([u8; 4]),
    F64(f64),
    F32(f32),
}

impl RoValue {
    /// Decode raw little-endian bytes of the field's kind; `None` when the
    /// width does not match.
    pub(crate) fn decode(kind: RoKind, b: &[u8]) -> Option<RoValue> {
        Some(match kind {
            RoKind::Bool => RoValue::Bool(*b.first()? != 0),
            RoKind::U32 => RoValue::U32(u32::from_le_bytes(b.get(..4)?.try_into().ok()?)),
            RoKind::Rgba => RoValue::Rgba(b.get(..4)?.try_into().ok()?),
            RoKind::F64 => RoValue::F64(f64::from_le_bytes(b.get(..8)?.try_into().ok()?)),
            RoKind::F32 => RoValue::F32(f32::from_le_bytes(b.get(..4)?.try_into().ok()?)),
        })
    }
    /// Decode a style document item's text (`<t:variant>`): integers for
    /// bools, u32s and colours (signed, as the XML writes them), decimals
    /// for f64s.
    pub(crate) fn parse_item(kind: RoKind, text: &str) -> Option<RoValue> {
        let t = text.trim();
        Some(match kind {
            RoKind::Bool => RoValue::Bool(t.parse::<i64>().ok()? != 0),
            RoKind::U32 => RoValue::U32(t.parse::<i64>().ok()? as u32),
            RoKind::Rgba => RoValue::Rgba((t.parse::<i64>().ok()? as u32).to_le_bytes()),
            RoKind::F64 => RoValue::F64(t.parse::<f64>().ok()?),
            RoKind::F32 => RoValue::F32(t.parse::<f32>().ok()?),
        })
    }
}

macro_rules! rendering_options {
    ($( $(#[$doc:meta])* $name:ident : $ty:ty = $kind:ident, $tag:literal, $off:expr, $item:expr; )*) => {
        /// The display settings of the document or of a style (§10.7, §16.10).
        /// Every field is pinned by a one-change pair; the fields marked
        /// undecoded in the spec are not exposed.
        #[derive(Debug, Clone, PartialEq, Default)]
        pub struct RenderingOptions {
            $( $(#[$doc])* pub $name: $ty, )*
        }

        pub(crate) const RO_FIELDS: &[RoField] = &[
            $( RoField { name: stringify!($name), kind: RoKind::$kind, tag: $tag, off17: $off, item: $item }, )*
        ];

        impl RenderingOptions {
            /// Store one decoded field by name.
            pub(crate) fn set(&mut self, name: &str, v: RoValue) {
                match name {
                    $( stringify!($name) => { if let Some(x) = <$ty as RoStore>::from_value(v) { self.$name = x; } } )*
                    _ => {}
                }
            }
            /// Every field as `(name, value)`, in table order.
            pub(crate) fn values(&self) -> Vec<(&'static str, Option<RoValue>)> {
                vec![ $( (stringify!($name), <$ty as RoStore>::to_value(&self.$name)), )* ]
            }
        }
    };
}

/// Conversion between a field's Rust type and [`RoValue`].
pub(crate) trait RoStore: Sized {
    fn from_value(v: RoValue) -> Option<Self>;
    fn to_value(&self) -> Option<RoValue>;
}
impl RoStore for bool {
    fn from_value(v: RoValue) -> Option<Self> {
        match v {
            RoValue::Bool(b) => Some(b),
            RoValue::U32(x) => Some(x != 0),
            _ => None,
        }
    }
    fn to_value(&self) -> Option<RoValue> {
        Some(RoValue::Bool(*self))
    }
}
impl RoStore for u32 {
    fn from_value(v: RoValue) -> Option<Self> {
        match v {
            RoValue::U32(x) => Some(x),
            RoValue::Bool(b) => Some(b as u32),
            _ => None,
        }
    }
    fn to_value(&self) -> Option<RoValue> {
        Some(RoValue::U32(*self))
    }
}
impl RoStore for [u8; 4] {
    fn from_value(v: RoValue) -> Option<Self> {
        match v {
            RoValue::Rgba(c) => Some(c),
            _ => None,
        }
    }
    fn to_value(&self) -> Option<RoValue> {
        Some(RoValue::Rgba(*self))
    }
}
impl RoStore for f64 {
    fn from_value(v: RoValue) -> Option<Self> {
        match v {
            RoValue::F64(x) => Some(x),
            _ => None,
        }
    }
    fn to_value(&self) -> Option<RoValue> {
        Some(RoValue::F64(*self))
    }
}
impl RoStore for f32 {
    fn from_value(v: RoValue) -> Option<Self> {
        match v {
            RoValue::F32(x) => Some(x),
            _ => None,
        }
    }
    fn to_value(&self) -> Option<RoValue> {
        Some(RoValue::F32(*self))
    }
}
impl<T: RoStore> RoStore for Option<T> {
    fn from_value(v: RoValue) -> Option<Self> {
        Some(T::from_value(v))
    }
    fn to_value(&self) -> Option<RoValue> {
        self.as_ref().and_then(T::to_value)
    }
}

rendering_options! {
    /// 0 wireframe, 1 hidden line, 2 shaded, 5 monochrome; textures are
    /// `textures` below.
    face_style: u32 = U32, 0x733d, Some(0), Some(2001);
    xray: bool = Bool, 0x733e, Some(4), Some(2004);
    material_transparency: bool = Bool, 0x733f, Some(5), Some(2005);
    /// Shaded with textures (with `face_style` 2).
    textures: bool = Bool, 0x7340, Some(30), Some(2007);
    edges: bool = U32, 0x7341, Some(7), Some(1000);
    back_edges: bool = Bool, 0x7356, Some(187), Some(1015);
    /// The value of `0x7341` (style item 1000) when it is neither 0 nor 1
    /// (2 in a style that displays its edges another way; `edges` reads
    /// true then). `None` while the setting is 0 or 1.
    edges_mode: Option<u32> = U32, 0x0000, None, None;
    /// 2026 and later only: the edge type (`0x7342`, style item 1001): 0
    /// lines, 1 sketchy strokes. `None` follows the style's strokes.
    edge_type: Option<u32> = U32, 0x7342, Some(113), None;
    /// 2026 and later only: text is displayed (`0x7344`).
    display_text: Option<bool> = Bool, 0x7344, Some(118), None;
    /// 2026 and later only: dimensions are displayed (`0x7345`).
    display_dimensions: Option<bool> = Bool, 0x7345, Some(119), None;
    /// 2026 and later only: draw profiles only (`0x7355`).
    profiles_only: Option<bool> = Bool, 0x7355, None, None;
    /// 0 by material, 1 all the same colour, 2 by axis.
    edge_color_mode: u32 = U32, 0x7348, Some(31), Some(1002);
    edge_color: [u8; 4] = Rgba, 0x7358, Some(15), Some(1014);
    profiles: bool = Bool, 0x734f, Some(40), Some(1006);
    profiles_width: u32 = U32, 0x7350, Some(41), Some(1007);
    depth_cue: bool = Bool, 0x7351, Some(45), Some(1008);
    depth_cue_width: u32 = U32, 0x7352, Some(46), Some(1009);
    extension: bool = Bool, 0x734d, Some(35), Some(1004);
    extension_length: u32 = U32, 0x734e, Some(36), Some(1005);
    endpoints: bool = Bool, 0x7353, Some(50), Some(1010);
    endpoints_length: u32 = U32, 0x7354, Some(51), Some(1011);
    jitter: bool = Bool, 0x734b, Some(6), Some(1012);
    /// Edge settings ▸ Dashes: layer line styles drawn dashed (`0x734c`;
    /// in the stream container from 2019, §14.5). Both containers read
    /// `Some(on)`; a stream file of a release before 2019 has no such
    /// setting and reads `None`. `None` written means on, as in a new
    /// model; `Some(false)` is written off in both containers.
    dashes: Option<bool> = Bool, 0x734c, None, None;
    front_color: [u8; 4] = Rgba, 0x735c, Some(61), Some(2002);
    back_color: [u8; 4] = Rgba, 0x735d, Some(65), Some(2003);
    /// 0 faster, 2 nicer.
    transparency_quality: u32 = U32, 0x7377, Some(160), Some(2006);
    /// Candidate: X-ray opacity (the control cannot be changed in
    /// SketchUp Make 2017).
    xray_opacity: f64 = F64, 0x7378, Some(179), Some(2008);
    background: [u8; 4] = Rgba, 0x7357, Some(11), Some(4000);
    sky: bool = Bool, 0x7368, Some(133), Some(4002);
    sky_color: [u8; 4] = Rgba, 0x7365, Some(121), Some(4001);
    ground: bool = Bool, 0x7369, Some(134), Some(4004);
    ground_color: [u8; 4] = Rgba, 0x7367, Some(129), Some(4003);
    /// 0–100.
    ground_transparency: u32 = U32, 0x736b, Some(136), Some(4005);
    ground_from_below: bool = Bool, 0x736a, Some(135), Some(4006);
    watermarks: bool = Bool, 0x735e, Some(178), Some(5000);
    selected_color: [u8; 4] = Rgba, 0x7359, Some(19), Some(7000);
    locked_color: [u8; 4] = Rgba, 0x735a, Some(174), Some(7001);
    guides_color: [u8; 4] = Rgba, 0x735b, Some(23), Some(7002);
    guides_hidden: bool = Bool, 0x7346, Some(120), Some(7012);
    model_axes: bool = Bool, 0x7343, Some(117), Some(7008);
    color_by_layer: bool = Bool, 0x7347, Some(29), Some(7011);
    hidden_geometry: bool = Bool, 0x7380, Some(56), Some(7017);
    /// 2026 only; a stream-container file's single hidden-geometry setting fills
    /// both.
    hidden_objects: bool = Bool, 0x7381, Some(56), Some(7018);
    active_section_color: [u8; 4] = Rgba, 0x7370, Some(140), Some(7003);
    inactive_section_color: [u8; 4] = Rgba, 0x7371, Some(144), Some(7004);
    section_cut_color: [u8; 4] = Rgba, 0x7372, Some(148), Some(7005);
    section_cut_width: u32 = U32, 0x7374, Some(152), Some(7014);
    /// Bit 1 section planes shown, bit 2 section cuts shown.
    section_display: u32 = U32, 0x7375, Some(156), Some(7013);
    /// 2026 only.
    section_fill: Option<bool> = Bool, 0x7376, None, Some(7015);
    /// 2026 only.
    section_fill_color: Option<[u8; 4]> = Rgba, 0x7373, None, Some(7016);
    /// 2026 and later only; the style's horizon colour (transparent black
    /// while unset).
    horizon_color: Option<[u8; 4]> = Rgba, 0x7366, Some(125), Some(4007);
    /// Default section fill style enabled (candidate name: the fill takes
    /// the style's own colour and pattern instead of the section plane's);
    /// 2027 and later only.
    default_section_fill_style: Option<bool> = Bool, 0x738c, None, Some(7020);
    /// Ambient occlusion on; 2026 and later only.
    ambient_occlusion: Option<bool> = Bool, 0x7383, None, Some(8100);
    /// Ambient occlusion distance, as typed (default 10); 2026 and later
    /// only.
    ambient_occlusion_distance: Option<f32> = F32, 0x7385, None, Some(8102);
    /// Ambient occlusion intensity (default 0); 2026 and later only.
    ambient_occlusion_intensity: Option<f32> = F32, 0x7386, None, Some(8103);
    /// Ambient occlusion colour in use; 2026 and later only.
    ambient_occlusion_color_enabled: Option<bool> = Bool, 0x738b, None, Some(8105);
    /// Ambient occlusion colour (default opaque black); 2026 and later
    /// only.
    ambient_occlusion_color: Option<[u8; 4]> = Rgba, 0x738a, None, Some(8106);
    /// Ambient occlusion multiplier (default 1); 2026 and later only.
    ambient_occlusion_multiplier: Option<f32> = F32, 0x7389, None, Some(8107);
    fog: bool = Bool, 0x735f, Some(87), None;
    fog_color: [u8; 4] = Rgba, 0x7360, Some(88), None;
    fog_uses_background: bool = Bool, 0x7361, Some(92), None;
    /// Metres; −1 when unset.
    fog_start_m: f64 = F64, 0x7362, Some(93), None;
    /// Metres; −1 when unset.
    fog_end_m: f64 = F64, 0x7363, Some(101), None;
    /// Component editing: hide the rest of the model.
    hide_rest_of_model: bool = Bool, 0x736e, Some(85), None;
    /// Component editing: hide similar components.
    hide_similar_components: bool = Bool, 0x736f, Some(86), None;
    /// Show component axes.
    component_axes: bool = Bool, 0x7349, Some(28), None;
    /// Component editing: opacity of the rest of the model, 0–1.
    fade_rest_of_model: f64 = F64, 0x736c, Some(69), None;
    /// Component editing: opacity of similar components, 0–1.
    fade_similar_components: f64 = F64, 0x736d, Some(77), None;
    /// Match Photo.
    background_photo: bool = Bool, 0x737c, Some(188), Some(8000);
    background_photo_opacity: f64 = F64, 0x737d, Some(189), Some(8001);
    foreground_photo: bool = Bool, 0x737e, Some(197), Some(8002);
    foreground_photo_opacity: f64 = F64, 0x737f, Some(198), Some(8003);
}

impl RenderingOptions {
    /// The fields a style document carries, with every other field at its
    /// default: what a saved style's settings can say.
    pub fn style_view(&self) -> RenderingOptions {
        let mut out = RenderingOptions::default();
        for (f, (_, v)) in RO_FIELDS.iter().zip(self.values()) {
            if let (Some(_), Some(v)) = (f.item, v) {
                out.set(f.name, v);
            }
        }
        out
    }

    pub fn section_planes(&self) -> bool {
        self.section_display & 1 != 0
    }
    pub fn section_cuts(&self) -> bool {
        self.section_display & 2 != 0
    }

    /// The display settings of SketchUp's default document — shaded with
    /// textures, edges and profiles on — which a written file takes where
    /// the model gives none (the derived `Default`, all zero, is a
    /// wireframe without edges).
    pub fn sketchup_default() -> RenderingOptions {
        RenderingOptions::from_2017(&crate::build17::RENDERING, 0)
            .expect("the default document's block decodes")
    }

    /// Decode the 2017 block whose face-style u32 is at `base` (§10.7).
    pub(crate) fn from_2017(d: &[u8], base: usize) -> Option<RenderingOptions> {
        let mut ro = RenderingOptions::default();
        for f in RO_FIELDS {
            let Some(off) = f.off17 else { continue };
            let v = RoValue::decode(f.kind, d.get(base + off..)?)?;
            ro.set(f.name, v);
        }
        // A style that displays its edges another way stores 2 here.
        if let Some(RoValue::U32(m)) = d
            .get(base + 7..)
            .and_then(|b| RoValue::decode(RoKind::U32, b))
        {
            ro.edges_mode = (m > 1).then_some(m);
        }
        // fog distances are stored in inches
        ro.fog_start_m = inches_to_m_or_unset(ro.fog_start_m);
        ro.fog_end_m = inches_to_m_or_unset(ro.fog_end_m);
        Some(ro)
    }

    /// Store one 2026 `0x733c` child record.
    pub(crate) fn set_record(&mut self, tag: u16, b: &[u8]) {
        if tag == 0x7341 {
            if let Some(RoValue::U32(m)) = RoValue::decode(RoKind::U32, b) {
                self.edges_mode = (m > 1).then_some(m);
            }
        }
        if let Some(f) = RO_FIELDS.iter().find(|f| f.tag == tag) {
            if let Some(v) = RoValue::decode(f.kind, b) {
                let v = match (tag, v) {
                    (0x7362 | 0x7363, RoValue::F64(x)) => RoValue::F64(inches_to_m_or_unset(x)),
                    _ => v,
                };
                self.set(f.name, v);
            }
        }
    }

    /// Store one style item (`<sty:item id>` or a 2017 CSkpStyle item).
    pub(crate) fn set_item(&mut self, item: u32, v: RoValue) {
        if item == 1000 {
            if let RoValue::U32(m) = v {
                self.edges_mode = (m > 1).then_some(m);
            }
        }
        for f in RO_FIELDS.iter().filter(|f| f.item == Some(item)) {
            self.set(f.name, v);
        }
    }
}

fn inches_to_m_or_unset(x: f64) -> f64 {
    if x < 0.0 {
        x
    } else {
        x / INCH
    }
}

/// The sun and shadow settings of the document or a scene (§10.9, §16.11).
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowInfo {
    /// Date and time, UNIX seconds.
    pub time: u32,
    pub city: String,
    pub country: String,
    pub longitude: f64,
    pub latitude: f64,
    /// Hours east of UTC.
    pub tz_offset_h: f64,
    pub displayed: bool,
    pub on_faces: bool,
    pub on_ground: bool,
    pub from_edges: bool,
    /// 0–100.
    pub light: u32,
    /// 0–100.
    pub dark: u32,
    pub use_sun_for_shading: bool,
    /// Daylight saving time applies (§10.9 `0x6592`).
    pub daylight_saving: bool,
    /// The north direction in model space, a unit vector; (0, 1, 0)
    /// unless north was turned (§10.9 `0x6598`).
    pub north: [f64; 3],
    /// Whether the north direction is drawn (§16.11 `0x659a`).
    pub north_displayed: bool,
}

impl Default for ShadowInfo {
    /// Zero and empty, north along green.
    fn default() -> ShadowInfo {
        ShadowInfo {
            time: 0,
            city: String::new(),
            country: String::new(),
            longitude: 0.0,
            latitude: 0.0,
            tz_offset_h: 0.0,
            displayed: false,
            on_faces: false,
            on_ground: false,
            from_edges: false,
            light: 0,
            dark: 0,
            use_sun_for_shading: false,
            daylight_saving: false,
            north: [0.0, 1.0, 0.0],
            north_displayed: false,
        }
    }
}

/// The document's unit settings (`UnitsOptions`, §16.12).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Units {
    /// 0 decimal, 1 architectural, 2 engineering, 3 fractional.
    pub length_format: u32,
    /// 0 inches, 1 feet, 2 millimetres, 3 centimetres, 4 metres.
    pub length_unit: u32,
    /// Decimal places, or the power of two for fractions.
    pub length_precision: u32,
    pub angle_precision: u32,
    pub length_snap: bool,
    /// In the display unit.
    pub length_snap_length: f64,
    pub angle_snap: bool,
    /// Degrees.
    pub snap_angle: f64,
    pub suppress_units_display: bool,
    pub force_inch_display: bool,
    /// 0 in², 1 ft², 2 mm², 3 cm², 4 m², 5 yd². Every 2026 file holds
    /// it; a 2017 file only when a later SketchUp saved it (SketchUp
    /// 2017 ignores it and drops it on save).
    pub area_unit: Option<u32>,
    /// 0 in³ … 5 yd³, 6 litres, 7 US gallons; present as `area_unit` is.
    pub volume_unit: Option<u32>,
    pub area_precision: Option<u32>,
    pub volume_precision: Option<u32>,
}

impl Units {
    /// The units of a new SketchUp model (its default template):
    /// architectural inches to 1/16", length snapping to 1/16" and angle
    /// snapping to 15° on. A model built in code starts from these — the
    /// derived `Default`, all zero, would open as whole inches with no
    /// snapping.
    pub fn sketchup_default() -> Units {
        Units {
            length_format: 1,
            length_unit: 0,
            length_precision: 4,
            angle_precision: 1,
            length_snap: true,
            length_snap_length: 0.0625,
            angle_snap: true,
            snap_angle: 15.0,
            ..Units::default()
        }
    }

    /// Build from the flat attribute list both readers produce.
    pub(crate) fn from_attributes(attrs: &[crate::Attribute]) -> Units {
        use crate::AttrValue::*;
        let mut u = Units::default();
        let int = |v: &crate::AttrValue| match v {
            Int(x) => Some(*x as u32),
            Bool(b) => Some(*b as u32),
            F64(_) => None,
        };
        let flt = |v: &crate::AttrValue| match v {
            F64(x) => Some(*x),
            Int(x) => Some(*x as f64),
            Bool(_) => None,
        };
        let boo = |v: &crate::AttrValue| match v {
            Bool(b) => Some(*b),
            Int(x) => Some(*x != 0),
            F64(_) => None,
        };
        for a in attrs {
            match a.key.as_str() {
                "LengthFormat" => u.length_format = int(&a.value).unwrap_or(0),
                "LengthUnit" => u.length_unit = int(&a.value).unwrap_or(0),
                "LengthPrecision" => u.length_precision = int(&a.value).unwrap_or(0),
                "AnglePrecision" => u.angle_precision = int(&a.value).unwrap_or(0),
                "LengthSnapEnabled" => u.length_snap = boo(&a.value).unwrap_or(false),
                "LengthSnapLength" => u.length_snap_length = flt(&a.value).unwrap_or(0.0),
                "AngleSnapEnabled" => u.angle_snap = boo(&a.value).unwrap_or(false),
                "SnapAngle" => u.snap_angle = flt(&a.value).unwrap_or(0.0),
                "SuppressUnitsDisplay" => u.suppress_units_display = boo(&a.value).unwrap_or(false),
                "ForceInchDisplay" => u.force_inch_display = boo(&a.value).unwrap_or(false),
                "AreaUnit" => u.area_unit = int(&a.value),
                "VolumeUnit" => u.volume_unit = int(&a.value),
                "AreaPrecision" => u.area_precision = int(&a.value),
                "VolumePrecision" => u.volume_precision = int(&a.value),
                _ => {}
            }
        }
        u
    }
}

/// A watermark of a style (§10.8, §16.14).
#[derive(Debug, Clone, PartialEq)]
pub struct Watermark {
    pub name: String,
    /// The image file the watermark was loaded from.
    pub source_file: String,
    /// True for a background watermark, false for an overlay.
    pub background: bool,
    pub tiled: bool,
    pub stretched: bool,
    pub keep_aspect_ratio: bool,
    /// Grid cell 0–8, row-major from the top left (4 = centre), for
    /// positioned watermarks.
    pub position: u32,
    /// Use the image brightness as a mask.
    pub mask: bool,
    /// Blend with the model, 0–1.
    pub blend: f64,
    /// Scale, 0–1.
    pub scale: f64,
    /// The watermark's image (PNG or JPEG bytes), when the reader could
    /// reach it.
    pub image_bytes: Option<Vec<u8>>,
    /// UNIX time the image was loaded; 0 when unknown, and a writer then
    /// stores the time of writing.
    pub time: u32,
}

/// The strokes of a sketchy-edge style (§10.8): the images its edges
/// are drawn with, one for each stroke length and variant.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Strokes {
    /// The 2026 style document's `dropoutLength` (0.0–0.02 observed).
    pub dropout_length: f64,
    /// `fadeFactor` (1.0–2.1 observed).
    pub fade_factor: f64,
    /// `width`: the height of the images in most styles.
    pub width: u32,
    /// `useHalo`.
    pub halo: bool,
    /// `haloLength`.
    pub halo_length: u32,
    /// The images stored for each length (`numProtos`).
    pub variants: u32,
    /// The stroke lengths in pixels, ascending.
    pub lengths: Vec<u32>,
    /// The images, PNGs, the variants of the first length first.
    pub images: Vec<Vec<u8>>,
}

/// The photo of a Match Photo scene (§10.10).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MatchPhoto {
    /// Where the image was loaded from.
    pub path: String,
    /// The image, as the file holds it.
    pub image: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// UNIX seconds.
    pub time: u32,
    pub scale: f64,
    /// Points in the image, x and y from 0 to 1 and a zero: the ends of
    /// the perspective bars among them; which point is which is not
    /// decoded (§10.10).
    pub points: Vec<[f64; 3]>,
}

/// A saved style (§10.8, §16.14): its display settings and watermarks.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub name: String,
    pub description: String,
    /// 16 bytes, hex.
    pub guid: String,
    pub settings: RenderingOptions,
    pub watermarks: Vec<Watermark>,
    /// The strokes of a sketchy-edge style (§10.8, §16.14).
    pub strokes: Option<Strokes>,
    /// The style's items that `settings` does not name, as the file
    /// holds them (§10.8, §16.14): both containers list a style's items
    /// by the same ids, so each is written back to either. Items a later
    /// SketchUp adds (1016, 4007, 8100–8107) land here.
    pub items: Vec<StyleItem>,
}

/// A style item by its id (§10.8, §16.14).
#[derive(Debug, Clone, PartialEq)]
pub struct StyleItem {
    pub id: u32,
    pub value: StyleValue,
}

/// A style item's value: the 2017 value types and the 2026 variant types
/// (1 bool, 4 integer, 7 double; a 2017 colour is 2026 type 4).
#[derive(Debug, Clone, PartialEq)]
pub enum StyleValue {
    Bool(bool),
    Int(i32),
    Double(f64),
    Colour([u8; 4]),
    /// A 2026 variant type with no known 2017 counterpart (type 6 on
    /// items 8102, 8103 and 8107), with its text.
    Other {
        kind: u32,
        text: String,
    },
}

/// The model axes: origin in metres and three unit directions.
#[derive(Debug, Clone, PartialEq)]
pub struct Axes {
    pub origin_m: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
    pub z: [f64; 3],
}

/// Which properties a scene saves (§16.15 `0x7149`); the bits a 2017 file
/// carries are 1–64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneProperties(pub u32);

impl SceneProperties {
    pub const CAMERA: u32 = 1;
    /// Switching off either the style or the rendering options of a 2027
    /// page clears this bit alone and drops the rendering record, so the
    /// two options are not separable.
    pub const STYLE_AND_FOG: u32 = 2;
    pub const SHADOWS: u32 = 4;
    pub const AXES: u32 = 8;
    /// The 2017 hidden-geometry property.
    pub const HIDDEN_GEOMETRY: u32 = 16;
    pub const VISIBLE_LAYERS: u32 = 32;
    pub const ACTIVE_SECTION_PLANES: u32 = 64;
    /// 2026: hidden geometry and hidden objects as separate bits. In
    /// SketchUp 2027 files each switches off alone (leaving bit 16 set);
    /// switching off the page's plain "hidden" option clears 16, 128 and
    /// 256 together.
    pub const HIDDEN_GEOMETRY_2: u32 = 128;
    pub const HIDDEN_OBJECTS: u32 = 256;
    /// The page's environment (2027: the option clears this bit alone).
    pub const ENVIRONMENT: u32 = 512;
    /// The page's point cloud data (2027: the option clears this bit
    /// alone). Bit 2048 is set in every page seen and no option clears
    /// it.
    pub const POINT_CLOUD_DATA: u32 = 1024;
    pub fn has(self, bit: u32) -> bool {
        self.0 & bit != 0
    }
}

/// A scene (§10.10, §16.15). Optional parts are present exactly when the
/// scene saves that property.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub name: String,
    pub description: String,
    pub saved: SceneProperties,
    pub camera: Option<Camera>,
    pub rendering: Option<RenderingOptions>,
    /// The name of the scene's style, when the style is saved.
    pub style: Option<String>,
    pub shadows: Option<ShadowInfo>,
    pub axes: Option<Axes>,
    /// Persistent ids of the entities the scene hides. Empty on the
    /// byte-scan fallback, which cannot resolve the references.
    pub hidden_entities: Vec<u32>,
    /// Persistent ids of the active section planes.
    pub active_section_planes: Vec<u32>,
    /// Indices into `Model::layers` of the layers the scene hides. Empty
    /// on the byte-scan fallback.
    pub hidden_layers: Vec<usize>,
    pub in_animation: bool,
    /// The scene's preview image, a PNG, as the file holds it.
    pub thumbnail: Option<Vec<u8>>,
    /// The photo of a Match Photo scene (§10.10, §16.15).
    pub match_photo: Option<MatchPhoto>,
}

impl Default for Scene {
    /// A nameless scene saving the seven 2017 properties and taking part
    /// in the animation, with no part of its own: a writer fills each
    /// saved property from the model's.
    fn default() -> Scene {
        Scene {
            name: String::new(),
            description: String::new(),
            saved: SceneProperties(127),
            camera: None,
            rendering: None,
            style: None,
            shadows: None,
            axes: None,
            hidden_entities: Vec::new(),
            active_section_planes: Vec::new(),
            hidden_layers: Vec::new(),
            in_animation: true,
            thumbnail: None,
            match_photo: None,
        }
    }
}

/// A text font (§10.1, §16.13).
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// Points.
    pub size_pt: u32,
    /// Sized in model units (the font dialog's Height) rather than in
    /// points.
    pub model_height: bool,
    /// The model-units height, metres: 10 inches until one is set.
    pub height_m: f64,
}

impl Font {
    /// The height a font is stored with until one is set: 10 inches.
    pub const DEFAULT_HEIGHT_M: f64 = 10.0 / INCH;
}

/// What a text or dimension is attached to (§16.13 anchor).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Anchor {
    /// 0 none, 1 a point, 2 a vertex, 4 an edge's midpoint, 5 a point on
    /// an edge, 7 a point on a face (a text; one 2027 sample, `entity` is
    /// the face), 9 where two edges meet (`entity` and `second`), 10 an
    /// arc's center (§10.1).
    pub kind: u32,
    /// Metres; for kind 5 the first component is the edge parameter.
    pub point_m: [f64; 3],
    /// Persistent id of the anchored entity, when one is stored.
    pub entity: Option<u32>,
    /// The instance path to that entity when it lives inside a group or
    /// component: the persistent ids of the placing instances from the
    /// entity list the annotation is in down (§10.1, §16.13 `0x53fe`).
    /// Empty for an entity in that list itself.
    pub path: Vec<u32>,
    /// The second entity of an anchor that names two (kind 9, where two
    /// edges meet), with its own instance path; `None` and empty
    /// otherwise.
    pub second: Option<u32>,
    pub second_path: Vec<u32>,
}

/// How a text is led to its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Leader {
    #[default]
    None,
    ViewBased,
    Pushpin,
}

/// A text entity (§10.2, §16.13).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub pid: u32,
    /// The owning definition's `Definition::map_index`; `None` for a text
    /// among the model's own entities.
    pub def_index: Option<usize>,
    pub content: String,
    /// Native SketchUp text placeholder (0x55FF).
    pub placeholder: String,
    /// Screen position as fractions of the view (screen text).
    pub screen_position: [f64; 2],
    pub anchor: Anchor,
    /// Model-space offset for a pushpin leader, screen-space for a
    /// view-based one.
    pub leader_offset: [f64; 3],
    pub leader: Leader,
    /// 0 none, 1 slash, 2 dot, 3 closed, 4 open.
    pub arrow: u32,
    /// Index into `Model::fonts`.
    pub font: Option<usize>,
    /// The layer's archive slot (0 = the default layer); link via
    /// `Model::layer_of`.
    pub layer: u16,
    /// The material's archive slot; link via `Model::material_of`.
    pub material: Option<u16>,
    pub hidden: bool,
    /// Whether it casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// The direction the view looked along when the text was placed
    /// (a unit vector; `[0, 0, -1]` when unknown).
    pub view_direction: [f64; 3],
    /// The leader is hidden (Entity Info ▸ Leader ▸ Hidden); `leader`
    /// keeps the kind it had.
    pub leader_hidden: bool,
}

impl Default for Text {
    /// A new one, with the values the writers wrote before these fields
    /// were carried.
    fn default() -> Text {
        Text {
            pid: 0,
            def_index: None,
            content: String::new(),
            placeholder: String::new(),
            screen_position: [0.0; 2],
            anchor: Anchor::default(),
            leader_offset: [0.0; 3],
            leader: Leader::None,
            arrow: 0,
            font: None,
            layer: 0,
            material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
            view_direction: [0.0, 0.0, -1.0],
            leader_hidden: false,
        }
    }
}

/// A linear dimension (§10.1, §16.13).
#[derive(Debug, Clone, PartialEq)]
pub struct Dimension {
    pub pid: u32,
    /// The owning definition's `Definition::map_index`; `None` for a
    /// dimension among the model's own entities.
    pub def_index: Option<usize>,
    /// Empty for the automatic measurement text.
    pub text_override: String,
    pub start: Anchor,
    pub end: Anchor,
    pub normal: [f64; 3],
    pub x_axis: [f64; 3],
    /// Offset of the dimension line from the measured points, metres.
    pub offset_m: f64,
    /// 0 above, 1 centred, 3 outside the dimension line.
    pub text_position: u32,
    /// Text aligned to the dimension line (else to the screen).
    pub aligned: bool,
    /// Index into `Model::fonts`.
    pub font: Option<usize>,
    /// The arrow at the ends: 0 none, 1 slash, 2 dot, 3 closed, 4 open.
    pub arrow: u32,
    /// The layer's archive slot (0 = the default layer); link via
    /// `Model::layer_of`.
    pub layer: u16,
    /// The material's archive slot; link via `Model::material_of`.
    pub material: Option<u16>,
    pub hidden: bool,
    /// Whether it casts and receives shadows (§5.2, §16.4).
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    /// Which way the basis lies (§16.13 `0x5bd1`): 1 when `x_axis` runs
    /// along the measured line and the dimension line is offset along
    /// `normal × x_axis`; 2 when the frame is turned a quarter turn, the
    /// measured line running along `normal × x_axis` and the offset along
    /// `−x_axis`.
    pub variant: u32,
    /// The f64 after the offset (2017 §10.1; `0x5bd3`): 0 in every
    /// SketchUp-authored pair, nonzero on some dimensions of user models;
    /// undecoded, carried as read so a rewrite keeps it.
    pub undecoded: f64,
}

/// A radial dimension (§10.12, §16.13): the radius of an arc or the
/// diameter of a circle, measured on one of its edges.
#[derive(Debug, Clone, PartialEq)]
pub struct RadialDimension {
    pub pid: u32,
    /// The owning definition's `Definition::map_index`; `None` for a
    /// dimension among the model's own entities.
    pub def_index: Option<usize>,
    /// The persistent id of an edge of the measured arc or circle, in
    /// the same entity list.
    pub edge: Option<u32>,
    /// The measured arc itself, in the owning list's frame, when the
    /// dimension names no edge: a 2017 file then holds the arc's
    /// geometry in the dimension (§10.12). `None` when `edge` names one.
    pub arc: Option<crate::Arc>,
    /// A diameter (else a radius).
    pub diameter: bool,
    /// The leader: its angle on the curve, radians, in the frame of the
    /// edge's arc, and its end's offset from the curve, inches, negative
    /// outward. The end lies at `radius − leader[1]` from the centre on
    /// that angle (§10.12).
    pub leader: [f64; 2],
    /// Empty for the automatic measurement text.
    pub text_override: String,
    /// Index into `Model::fonts`.
    pub font: Option<usize>,
    /// Text aligned to the dimension line (else to the screen).
    pub aligned: bool,
    /// The arrow at the end: 0 none, 1 slash, 2 dot, 3 closed, 4 open.
    pub arrow: u32,
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

impl Default for RadialDimension {
    /// A visible radius dimension with a closed arrow that casts and
    /// receives shadows; the rest zero.
    fn default() -> RadialDimension {
        RadialDimension {
            pid: 0,
            def_index: None,
            edge: None,
            arc: None,
            diameter: false,
            leader: [0.0; 2],
            text_override: String::new(),
            font: None,
            aligned: false,
            arrow: 3,
            layer: 0,
            material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
        }
    }
}

impl Default for Dimension {
    /// A new one, with the values the writers wrote before these fields
    /// were carried.
    fn default() -> Dimension {
        Dimension {
            pid: 0,
            def_index: None,
            text_override: String::new(),
            start: Anchor::default(),
            end: Anchor::default(),
            normal: [0.0, 0.0, 1.0],
            x_axis: [1.0, 0.0, 0.0],
            offset_m: 0.0,
            text_position: 0,
            aligned: false,
            font: None,
            arrow: 1,
            layer: 0,
            material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
            variant: 1,
            undecoded: 0.0,
        }
    }
}

/// A component definition's behaviour (§7.1, §16.5).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Behaviour {
    pub glues_to_surface: bool,
    /// 0 any, 1 horizontal, 2 vertical, 3 sloped.
    pub glue_plane: u32,
    pub cuts_opening: bool,
    pub always_faces_camera: bool,
    pub shadows_face_sun: bool,
    /// The u32 after the behaviour bits (§7.1; §16.5 `0x1b5a`): 0, but
    /// 0x7a and 0x7f on two dynamic components; meaning undecoded.
    pub extra: u32,
}

/// Scene animation settings (`PageOptions` and `SlideshowOptions`, §16.12).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Animation {
    pub transitions: bool,
    /// Seconds.
    pub transition_s: f64,
    /// Seconds each scene is shown.
    pub delay_s: f64,
    pub loop_slideshow: bool,
}

impl Animation {
    pub(crate) fn from_attributes(attrs: &[crate::Attribute]) -> Animation {
        use crate::AttrValue::*;
        let mut a = Animation::default();
        for at in attrs {
            match (at.key.as_str(), &at.value) {
                ("ShowTransition", Bool(b)) => a.transitions = *b,
                ("ShowTransition", Int(x)) => a.transitions = *x != 0,
                ("TransitionTime", F64(x)) => a.transition_s = *x,
                ("SlideTime", F64(x)) => a.delay_s = *x,
                ("LoopSlideshow", Bool(b)) => a.loop_slideshow = *b,
                ("LoopSlideshow", Int(x)) => a.loop_slideshow = *x != 0,
                _ => {}
            }
        }
        a
    }
}

/// Defaults for new texts (§10.11, §16.13).
#[derive(Debug, Clone, PartialEq)]
pub struct TextDefaults {
    /// Index into `Model::fonts` (leader text).
    pub font: Option<usize>,
    /// Index into `Model::fonts` (screen text).
    pub screen_font: Option<usize>,
    /// 0 none, 1 slash, 2 dot, 3 closed, 4 open.
    pub arrow: u32,
    pub leader_text_color: [u8; 4],
    pub screen_text_color: [u8; 4],
    /// The leader of new texts (Model Info ▸ Text ▸ Leader): 1 view
    /// based, 2 pushpin, which a new document holds.
    pub leader: u32,
}

/// Defaults for new dimensions (§10.11, §16.13).
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionDefaults {
    /// Index into `Model::fonts`.
    pub font: Option<usize>,
    /// Text aligned to the dimension line (else to the screen).
    pub aligned: bool,
    /// 0 none, 1 slash, 2 dot, 3 closed, 4 open.
    pub arrow: u32,
    /// 0 above, 1 centred, 3 outside the dimension line.
    pub text_position: u32,
    pub color: [u8; 4],
    /// Expert Dimension Settings ▸ Highlight non-associated dimensions:
    /// SketchUp prefixes the text of a dimension not anchored to vertices
    /// with `~` (and highlights it in the colour that follows this flag in
    /// the record, `0x5fbe`). Off in a new document.
    pub highlight_non_associated: bool,
    /// Expert Dimension Settings ▸ Show radius/diam prefix: on in a new
    /// document.
    pub radius_prefix: bool,
    /// Expert Dimension Settings ▸ Hide when foreshortened.
    pub hide_foreshortened: bool,
    /// Expert Dimension Settings ▸ Hide when too small.
    pub hide_small: bool,
}
