use serde::Deserialize;
use skpwright::{
    PbrMaterial, PbrTexture, Behaviour, Definition, DefinitionKind, FaceTexture, GeometryRun, Layer, Material, Mesh, SectionPlane,
    MeshEdge, MeshFace, RadialDimension, Model, PlacedInstance, Anchor, Text, Leader, Dimension, GuidePoint, Guide, Camera, Scene, SceneProperties, Style, RenderingOptions,
};
use std::{error::Error, fs, path::Path};

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct InputUnits {
    length_format: u32,
    length_unit: u32,
    length_precision: u32,
    angle_precision: u32,
    length_snap: bool,
    length_snap_length: f64,
    angle_snap: bool,
    snap_angle: f64,
    suppress_units_display: bool,
    force_inch_display: bool,
    #[serde(default)]
    area_unit: Option<u32>,
    #[serde(default)]
    volume_unit: Option<u32>,
    #[serde(default)]
    area_precision: Option<u32>,
    #[serde(default)]
    volume_precision: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputDimensionDefaults {
    font: Option<usize>,
    aligned: bool,
    arrow: u32,
    text_position: u32,
    color: [u8; 4],
    highlight_non_associated: bool,
    radius_prefix: bool,
    hide_foreshortened: bool,
    hide_small: bool,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputSourceLayerLink {
    slot: u16,
    index: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputClassification {
    name: String,
    source_path: String,
    bytes: usize,
    data_hex: String,
}

fn decode_classification_hex(data: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if data.len() % 2 != 0 {
        return Err("Classification hex length must be even".into());
    }

    let mut result = Vec::with_capacity(data.len() / 2);

    for pair in data.as_bytes().chunks_exact(2) {
        let digit = |b: u8| -> Result<u8, Box<dyn Error>> {
            match b {
                b'0'..=b'9' => Ok(b - b'0'),
                b'a'..=b'f' => Ok(b - b'a' + 10),
                b'A'..=b'F' => Ok(b - b'A' + 10),
                _ => Err("Invalid classification hex digit".into()),
            }
        };

        result.push((digit(pair[0])? << 4) | digit(pair[1])?);
    }

    Ok(result)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputDictionaryGroup {
    owner: serde_json::Value,
    dictionaries: Vec<InputDictionary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputDictionary {
    name: String,
    flag: u32,
    entries: Vec<(String, serde_json::Value)>,
}

fn typed_dictionary_value(raw: &serde_json::Value) -> Result<skpwright::Value, Box<dyn Error>> {
    use skpwright::Value;
    let parts = raw.as_array().ok_or("Dictionary value must be a tagged array")?;
    let tag = parts.first().and_then(|v| v.as_str()).ok_or("Missing dictionary type tag")?;
    if tag == "nil" && parts.len() == 1 { return Ok(Value::Nil); }
    if parts.len() != 2 { return Err(format!("Invalid dictionary value: {}", tag).into()); }
    let value = &parts[1];
    Ok(match tag {
        "text" => Value::Text(value.as_str().ok_or("Invalid dictionary text")?.to_owned()),
        "int" => Value::Int(i32::try_from(value.as_i64().ok_or("Invalid dictionary integer")?)?),
        "bool" => Value::Bool(value.as_bool().ok_or("Invalid dictionary boolean")?),
        "double" => Value::Double(value.as_f64().filter(|v| v.is_finite()).ok_or("Invalid dictionary double")?),
        "array" => Value::Array(value.as_array().ok_or("Invalid dictionary array")?
            .iter().map(typed_dictionary_value).collect::<Result<Vec<_>, _>>()?),
        _ => return Err(format!("Unsupported dictionary value type: {}", tag).into()),
    })
}

 // BLACKFOX_PHASE_2P_WATERMARK_INPUT
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputWatermark {
    name: String,
    source_file: String,
    background: bool,
    tiled: bool,
    stretched: bool,
    keep_aspect_ratio: bool,
    position: u32,
    mask: bool,
    blend: f64,
    scale: f64,
    time: u32,
    image_bytes: Option<usize>,
    image_hex: Option<String>,
}

 // BLACKFOX_PHASE_2P_WATERMARK_RECONSTRUCTION
fn blackfox_native_watermark(
    input: &InputWatermark,
) -> Result<skpwright::Watermark, Box<dyn Error>> {
    if !input.blend.is_finite() || !input.scale.is_finite() {
        return Err("Non-finite watermark metadata".into());
    }

    let image_bytes = match (&input.image_hex, input.image_bytes) {
        (None, None) => None,
        (Some(hex), Some(expected)) => {
            if hex.len() != expected.checked_mul(2)
                .ok_or("Watermark image length overflow")?
            {
                return Err("Watermark image hex length mismatch".into());
            }

            let bytes = decode_classification_hex(hex)?;

            if bytes.len() != expected {
                return Err("Watermark image byte count mismatch".into());
            }

            if !bytes.is_empty()
                && !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            {
                return Err("Watermark image is not PNG".into());
            }

            Some(bytes)
        }
        _ => return Err("Incomplete watermark image payload".into()),
    };

    Ok(skpwright::Watermark {
        name: input.name.clone(),
        source_file: input.source_file.clone(),
        background: input.background,
        tiled: input.tiled,
        stretched: input.stretched,
        keep_aspect_ratio: input.keep_aspect_ratio,
        position: input.position,
        mask: input.mask,
        blend: input.blend,
        scale: input.scale,
        image_bytes,
        time: input.time,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputStyleWatermarks {
    style_index: usize,
    watermarks: Vec<InputWatermark>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputModel {
    #[serde(default)]
    source_model_guid: Option<String>,
    #[serde(default)]
    source_current_material: Option<usize>,
    #[serde(default)]
    source_active_layer: Option<usize>,
    #[serde(default)]
    source_shadows: Option<serde_json::Value>,
    #[serde(default)]
    source_geo_located: bool,
    #[serde(default)]
    source_dictionaries: Vec<InputDictionaryGroup>,
    #[serde(default)]
    source_classifications: Vec<InputClassification>,
    #[serde(default)]
    source_rendering: Option<InputRendering>,
    #[serde(default)]
    source_animation: Option<InputAnimation>,
    #[serde(default)]
    source_style_name: Option<String>,
    #[serde(default)]
    source_style_description: Option<String>,
    #[serde(default)]
    source_style_watermarks: Vec<InputStyleWatermarks>,
    #[serde(default)]
    source_current_watermarks: Vec<InputWatermark>,
    #[serde(default)]
    source_native_watermark_section_value: Option<u32>,
    #[serde(default)]
    source_units: Option<InputUnits>,
    #[serde(default)]
    source_dimension_defaults: Option<InputDimensionDefaults>,
    #[serde(default)]
    camera: Option<InputCamera>,
    #[serde(default)]
    scenes: Vec<InputScene>,
    #[serde(default)]
    selected_scene: Option<String>,
    #[serde(default)]
    section_planes: Vec<InputSectionPlane>,
    #[serde(default)]
    active_section_plane: Option<String>,
    #[serde(default)]
    layers: Vec<InputLayer>,
    #[serde(default, rename = "source_layer_links")]
    source_layer_links: Vec<InputSourceLayerLink>,
    #[serde(default)]
    objects: Vec<InputObject>,
    #[serde(default)]
    loose_geometry: Vec<InputObject>,
    #[serde(default)]
    component_definitions: Vec<InputComponentDefinition>,
    #[serde(default)]
    component_instances: Vec<InputComponentInstance>,
    #[serde(default)]
    texts: Vec<InputText>,
    #[serde(default)]
    dimensions: Vec<InputDimension>,
    #[serde(default)]
    native_texts: Vec<InputNativeText>,
    #[serde(default)]
    native_dimensions: Vec<InputNativeDimension>,
    // D4N61AE phase 1: refuse silent loss until edge-PID reconstruction is implemented.
    #[serde(default)]
    native_radial_dimensions: Vec<InputNativeRadialDimension>,
    #[serde(default)]
    native_geometry_entities: Vec<InputNativeGeometryEntities>,
    #[serde(default)]
    construction_points: Vec<InputConstructionPoint>,
    #[serde(default)]
    construction_lines: Vec<InputConstructionLine>,
    #[serde(default)]
    source_materials: Vec<InputSourceMaterial>,
    #[serde(default)]
    source_pbr: Vec<InputPbrMaterial>,

}

// Supported subset of native SketchUp rendering properties.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputAnimation {
    transitions: bool,
    transition_s: f64,
    delay_s: f64,
    loop_slideshow: bool,
}
#[derive(Deserialize, Default)]
struct InputRendering {
    section_display: Option<u32>,
    section_fill: Option<bool>,
    section_fill_color: Option<[u8; 4]>,
    face_style: Option<u32>,
    textures: Option<bool>,
    edges: Option<bool>,
    profiles: Option<bool>,
    profiles_width: Option<u32>,
    background: Option<[u8; 4]>,
    sky: Option<bool>,
    sky_color: Option<[u8; 4]>,
    ground: Option<bool>,
    ground_color: Option<[u8; 4]>,
    front_color: Option<[u8; 4]>,
    back_color: Option<[u8; 4]>,
    ambient_occlusion: Option<bool>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct InputCamera {
    eye_m: [f64; 3],
    target_m: [f64; 3],
    up: [f64; 3],
    #[serde(default = "default_true")]
    perspective: bool,
    #[serde(default = "default_fov")]
    fov_deg: f64,
    #[serde(default = "default_parallel_height")]
    parallel_height_m: f64,
    #[serde(default)]
    two_point_perspective: bool,
}

fn default_true() -> bool { true }
fn default_fov() -> f64 { 35.0 }
fn default_parallel_height() -> f64 { 3.0 }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputScene {
    name: String,
    #[serde(default)]
    description: String,
    camera: InputCamera,
    #[serde(default)]
    saved: Option<u32>,
    #[serde(default)]
    rendering: Option<serde_json::Value>,
    #[serde(default)]
    shadows: Option<serde_json::Value>,
    #[serde(default)]
    axes: Option<serde_json::Value>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    hidden_layers: Option<Vec<String>>,
    #[serde(default)]
    active_section_planes: Option<Vec<String>>,
    #[serde(default)]
    thumbnail_path: Option<String>,
}

fn blackfox_camera(c: &InputCamera) -> Result<Camera, Box<dyn Error>> {
    for value in c.eye_m.iter()
        .chain(c.target_m.iter())
        .chain(c.up.iter())
        .chain(std::iter::once(&c.fov_deg))
        .chain(std::iter::once(&c.parallel_height_m))
    {
        if !value.is_finite() {
            return Err("Camera contains a non-finite number".into());
        }
    }

    let sight = [
        c.target_m[0] - c.eye_m[0],
        c.target_m[1] - c.eye_m[1],
        c.target_m[2] - c.eye_m[2],
    ];

    let norm = |v: [f64; 3]| {
        v.iter().map(|x| x*x).sum::<f64>().sqrt()
    };

    let sight_len = norm(sight);
    let up_len = norm(c.up);

    if sight_len < 1e-9 || up_len < 1e-9 {
        return Err("Camera direction or up vector is zero".into());
    }

    let cross = [
        sight[1]*c.up[2] - sight[2]*c.up[1],
        sight[2]*c.up[0] - sight[0]*c.up[2],
        sight[0]*c.up[1] - sight[1]*c.up[0],
    ];

    if norm(cross) / (sight_len * up_len) < 1e-8 {
        return Err("Camera up vector is parallel to viewing direction".into());
    }

    if c.perspective && !(c.fov_deg > 0.0 && c.fov_deg < 180.0) {
        return Err("Perspective FOV must be between 0 and 180 degrees".into());
    }

    if c.parallel_height_m <= 0.0 {
        return Err("Parallel camera height must be positive".into());
    }

    Ok(Camera {
        eye_m: c.eye_m,
        target_m: c.target_m,
        up: c.up,
        perspective: c.perspective,
        fov_deg: c.fov_deg,
        parallel_height_m: c.parallel_height_m,
        two_point_perspective: c.two_point_perspective,
        description: String::new(),
        undecoded: [1.0, 1000.0],
        aspect_ratio: 0.0,
        scale_2d: 1.0,
        offset_2d: [0.0, 0.0],
        image_width: 0.0,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputSectionPlane {
    id: String,
    plane: [f64; 4],
    #[serde(default)]
    name: String,
    #[serde(default)]
    symbol: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    layer: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputConstructionPoint {
    point: [f64; 3],
    #[serde(default)]
    anchor: Option<[f64; 3]>,
    #[serde(default)]
    layer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputConstructionLine {
    point: [f64; 3],
    direction: [f64; 3],
    #[serde(default)]
    layer: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputNativeGeometryEntities {
    source_def_index: Option<usize>,
    #[serde(default)]
    guide_points: Vec<InputNativeGuidePoint>,
    #[serde(default)]
    sections: Vec<InputNativeSectionPlane>,
    active_section: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputNativeGuidePoint {
    pid: u32,
    point_m: [f64; 3],
    anchor_m: [f64; 3],
    layer: u16,
    #[allow(dead_code)]
    material: Option<u16>,
    hidden: bool,
    casts_shadows: bool,
    receives_shadows: bool,
    flag: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputNativeSectionPlane {
    pid: u32,
    plane: [f64; 4],
    name: String,
    symbol: String,
    layer: u16,
    #[allow(dead_code)]
    material: Option<u16>,
    hidden: bool,
    casts_shadows: bool,
    receives_shadows: bool,
    #[allow(dead_code)]
    hatch: Option<usize>,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct InputNativeAnchor {
    kind: u32,
    point_m: [f64; 3],
    #[serde(default)]
    entity: Option<u32>,
    #[serde(default)]
    path: Vec<u32>,
    #[serde(default)]
    second: Option<u32>,
    #[serde(default)]
    second_path: Vec<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputNativeText {
    content: String,
    #[serde(default)]
    placeholder: String,
    #[serde(default)]
    def_index: Option<usize>,
    screen_position: [f64; 2],
    anchor: InputNativeAnchor,
    leader_offset: [f64; 3],
    leader: String,
    arrow: u32,
    #[serde(default)]
    font: Option<usize>,
    layer: u16,
    #[serde(default)]
    #[allow(dead_code)]
    material: Option<u16>,
    hidden: bool,
    casts_shadows: bool,
    receives_shadows: bool,
    view_direction: [f64; 3],
    leader_hidden: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputNativeDimension {
    #[serde(default)]
    def_index: Option<usize>,
    text_override: String,
    start: InputNativeAnchor,
    end: InputNativeAnchor,
    normal: [f64; 3],
    x_axis: [f64; 3],
    offset_m: f64,
    text_position: u32,
    aligned: bool,
    #[serde(default)]
    font: Option<usize>,
    arrow: u32,
    layer: u16,
    #[serde(default)]
    #[allow(dead_code)]
    material: Option<u16>,
    hidden: bool,
    casts_shadows: bool,
    receives_shadows: bool,
    variant: u32,
    undecoded: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputNativeRadialDimension {
    pid: u32,
    def_index: Option<usize>,
    edge: Option<u32>,
    arc: Option<serde_json::Value>,
    diameter: bool,
    leader: [f64; 2],
    text_override: String,
    font: Option<usize>,
    aligned: bool,
    arrow: u32,
    layer: u16,
    material: Option<u16>,
    hidden: bool,
    casts_shadows: bool,
    receives_shadows: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputDimension {
    start: [f64; 3],
    end: [f64; 3],
    offset_m: f64,
    #[serde(default)]
    object_index: Option<usize>,
    #[serde(default)]
    component_instance_index: Option<usize>,
    #[serde(default)]
    start_vertex: Option<usize>,
    #[serde(default)]
    end_vertex: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputText {
    content: String,
    position: [f64; 3],
    #[serde(default)]
    leader: bool,
    #[serde(default)]
    offset: Option<[f64; 3]>,
    #[serde(default)]
    layer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputLayer {
    name: String,
    color: [u8; 4],
    #[serde(default = "default_visible")]
    visible: bool,
}

fn validated_source_guid(value: Option<&str>) -> Result<String, Box<dyn Error>> {
    match value {
        None => Ok(String::new()),
        Some(guid) => {
            if guid.len() != 32 || !guid.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("Invalid source definition GUID: {}", guid).into());
            }
            Ok(guid.to_ascii_lowercase())
        }
    }
}

fn default_visible() -> bool {
    true
}
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct InputComponentBehaviour {
    #[serde(default)]
    glues_to_surface: bool,
    #[serde(default)]
    glue_plane: u32,
    #[serde(default)]
    cuts_opening: bool,
    #[serde(default)]
    always_faces_camera: bool,
    #[serde(default)]
    shadows_face_sun: bool,
    #[serde(default)]
    extra: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputComponentDefinition {
    #[serde(default)]
    is_group: bool,
    #[serde(default)]
    source_definition_guid: Option<String>,
    #[serde(default)]
    source_definition_flag: Option<bool>,
    #[serde(default)]
    behaviour: InputComponentBehaviour,
    #[serde(default)]
    source_vertex_pids: Vec<u32>,
    #[serde(default)]
    source_edge_pids: Vec<u32>,
    #[serde(default)]
    source_def_index: Option<usize>,
    #[serde(default)]
    edge_properties: Vec<InputEdgeProperties>,
    #[serde(default)]
    curves: Vec<InputCurve>,
    id: String,
    #[serde(default)]
    source_description: String,
    name: String,
    vertices: Vec<[f64; 3]>,
    edges: Vec<[u32; 2]>,
    faces: Vec<InputFace>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputComponentInstance {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    material_slot: Option<u16>,
    #[serde(default)]
    source_instance_pid: Option<u32>,
    #[serde(default)]
    source_instance_guid: Option<String>,
    definition_id: String,
    #[serde(default)]
    parent_definition_id: Option<String>,
    transform: [f64; 13],
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    color: Option<[u8; 4]>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    locked: bool,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputTexture {
    file: String,
    width_m: f64,
    height_m: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputArc {
    center_m: [f64; 3],
    normal: [f64; 3],
    x_axis_m: [f64; 3],
    y_axis_m: [f64; 3],
    radius_m: f64,
    start_rad: f64,
    end_rad: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputCurve {
    edges: Vec<usize>,
    #[serde(default)]
    arc: Option<InputArc>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct InputEdgeProperties {
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    soft: bool,
    #[serde(default)]
    smooth: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputObject {
    #[serde(default)]
    source_definition_guid: Option<String>,
    #[serde(default)]
    source_definition_flag: Option<bool>,
    #[serde(default)]
    source_vertex_pids: Vec<u32>,
    #[serde(default)]
    source_edge_pids: Vec<u32>,
    #[serde(default)]
    source_instance_pid: Option<u32>,
    #[serde(default)]
    source_instance_guid: Option<String>,
    #[serde(default)]
    transform: Option<[f64; 13]>,
    #[serde(default)]
    source_def_index: Option<usize>,
    #[serde(default)]
    edge_properties: Vec<InputEdgeProperties>,
    #[serde(default)]
    curves: Vec<InputCurve>,
    #[serde(default)]
    texture: Option<InputTexture>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    locked: bool,
    name: String,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    color: Option<[u8; 4]>,
    vertices: Vec<[f64; 3]>,
    edges: Vec<[u32; 2]>,
    faces: Vec<InputFace>,
}


#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputPbrTexture {
    id: u32,
    path: String,
    file_name: String,
    asset_path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputPbrMaterial {
    material: usize,
    workflow: u32,
    promo_state: u32,
    trans_unused: Option<f64>,
    enable_metalness: bool,
    enable_roughness: bool,
    enable_normal: bool,
    enable_occlusion: bool,
    roughness_texture_invert: bool,
    metallic_factor: f32,
    roughness_factor: f32,
    normal_map_style: u32,
    normal_scale: f32,
    occlusion_strength: f32,
    base_color_factor: u32,
    scale: Option<[f64; 2]>,
    metallic: Option<InputPbrTexture>,
    roughness: Option<InputPbrTexture>,
    normal: Option<InputPbrTexture>,
    occlusion: Option<InputPbrTexture>,
}

fn blackfox_pbr_texture(
    input: &Option<InputPbrTexture>,
) -> Result<Option<PbrTexture>, Box<dyn Error>> {
    let Some(t) = input else { return Ok(None); };

    let png = fs::read(&t.asset_path)?;
    if !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(format!("Invalid PBR PNG: {}", t.asset_path).into());
    }

    Ok(Some(PbrTexture {
        id: t.id,
        path: t.path.clone(),
        file_name: t.file_name.clone(),
        png,
        other_attrs: Vec::new(),
    }))
}

fn blackfox_pbr_material(
    input: &InputPbrMaterial,
) -> Result<PbrMaterial, Box<dyn Error>> {
    let numbers = [
        input.metallic_factor,
        input.roughness_factor,
        input.normal_scale,
        input.occlusion_strength,
    ];
    if numbers.iter().any(|v| !v.is_finite())
        || input.trans_unused.is_some_and(|v| !v.is_finite())
        || input.scale.is_some_and(|s| s.iter().any(|v| !v.is_finite()))
    {
        return Err("Non-finite PBR property".into());
    }

    Ok(PbrMaterial {
        workflow: input.workflow,
        promo_state: input.promo_state,
        trans_unused: input.trans_unused,
        enable_metalness: input.enable_metalness,
        enable_roughness: input.enable_roughness,
        enable_normal: input.enable_normal,
        enable_occlusion: input.enable_occlusion,
        roughness_texture_invert: input.roughness_texture_invert,
        metallic_factor: input.metallic_factor,
        roughness_factor: input.roughness_factor,
        normal_map_style: input.normal_map_style,
        normal_scale: input.normal_scale,
        occlusion_strength: input.occlusion_strength,
        base_color_factor: input.base_color_factor,
        scale: input.scale,
        metallic: blackfox_pbr_texture(&input.metallic)?,
        roughness: blackfox_pbr_texture(&input.roughness)?,
        normal: blackfox_pbr_texture(&input.normal)?,
        occlusion: blackfox_pbr_texture(&input.occlusion)?,
        ..PbrMaterial::default()
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputSourceMaterial {
    kind: String,
    name: String,
    #[serde(default)]
    rgba: Option<[u8; 4]>,
    #[serde(default)]
    opacity: Option<f64>,
    #[serde(default)]
    colorize_type: Option<u32>,
    #[serde(default)]
    texture: Option<String>,
    #[serde(default)]
    applied_size_in: Option<[f64; 2]>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputFace {
    #[serde(default)]
    source_face_pid: Option<u32>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    layer: Option<String>,
    outer: Vec<u32>,
    #[serde(default)]
    holes: Vec<Vec<u32>>,
    normal: [f64; 3],

    #[serde(default)]
    material_slot: Option<u16>,

    #[serde(default)]
    color: Option<[u8; 4]>,

    #[serde(default)]
    texture: Option<InputTexture>,

    #[serde(default)]
    uvw: Option<[f64; 8]>,

    // Back-side material is independent of the front-side material.
    #[serde(default)]
    back_material_slot: Option<u16>,
    #[serde(default)]
    back_color: Option<[u8; 4]>,
    #[serde(default)]
    back_texture: Option<InputTexture>,
    #[serde(default)]
    back_uvw: Option<[f64; 8]>,
}

// Register a material for a single face. Slots are 1-based; slot 0 is unpainted.
fn register_face_material(
    face: &InputFace,
    owner: &str,
    face_index: usize,
    back: bool,
    materials: &mut Vec<Material>,
    material_links: &mut Vec<(u16, usize)>,
) -> Result<Option<u16>, Box<dyn Error>> {
    let (material_slot, color, texture, uvw, side) = if back {
        (face.back_material_slot, face.back_color, face.back_texture.as_ref(), face.back_uvw, "Back")
    } else {
        (face.material_slot, face.color, face.texture.as_ref(), face.uvw, "Front")
    };
    if material_slot.is_some() && color.is_some() {
        return Err(format!("{} face {}: material_slot cannot be combined with color or texture", owner, face_index).into());
    }
    if color.is_some() && texture.is_some() {
        return Err(format!("{} face {}: specify color or texture, not both", owner, face_index).into());
    }
    if let Some(uvw) = uvw {
        if !uvw.iter().all(|v| v.is_finite()) {
            return Err(format!("{} face {}: UVW contains non-finite values", owner, face_index).into());
        }
    }
    if let Some(slot) = material_slot {
        if slot != 0 && !material_links.iter().any(|(registered, _)| *registered == slot) {
            return Err(format!("{} face {}: material slot {} is not registered", owner, face_index, slot).into());
        }

        if let Some(t) = texture {
            let index = material_links.iter()
                .find(|(registered, _)| *registered == slot)
                .map(|(_, index)| *index)
                .ok_or_else(|| format!(
                    "{} face {}: unregistered texture slot {}",
                    owner, face_index, slot
                ))?;

            match &materials[index] {
                Material::Textured { applied_size_in, .. } => {
                    let size = applied_size_in.as_ref().ok_or_else(|| {
                        format!("{} face {}: material has no texture size", owner, face_index)
                    })?;

                    let width = size.0 * 0.0254;
                    let height = size.1 * 0.0254;

                    if !t.width_m.is_finite()
                        || !t.height_m.is_finite()
                        || t.width_m <= 0.0
                        || t.height_m <= 0.0
                        || (t.width_m - width).abs() > 1e-7
                        || (t.height_m - height).abs() > 1e-7
                    {
                        return Err(format!(
                            "{} face {}: texture dimensions disagree with slot {}",
                            owner, face_index, slot
                        ).into());
                    }
                }

                _ => {
                    return Err(format!(
                        "{} face {}: UV texture metadata references solid slot {}",
                        owner, face_index, slot
                    ).into());
                }
            }
        }

        return Ok(Some(slot));
    }
    let material = if let Some(texture) = texture {
        if !texture.width_m.is_finite() || !texture.height_m.is_finite()
            || texture.width_m <= 0.0 || texture.height_m <= 0.0 {
            return Err(format!("{} face {}: invalid texture dimensions", owner, face_index).into());
        }
        let bytes = fs::read(&texture.file)?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
            && !bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Err(format!("{} face {}: texture must be PNG or JPEG", owner, face_index).into());
        }
        Some(Material::Textured {
            name: format!("BlackFox_Face_{}_Texture_{}_{}", side, owner, face_index),
            texture: Some(texture.file.clone()),
            applied_size_in: Some((texture.width_m / 0.0254, texture.height_m / 0.0254)),
            jpeg_quality: None,
            image_bytes: Some(bytes),
            avg_rgba: Some([180, 180, 180, 255]),
            colorized: false,
            texture_avg_rgba: Some([180, 180, 180, 255]),
            colorize_type: 0,
            opacity: 1.0,
        })
    } else {
        color.map(|rgba| Material::Solid {
            name: format!("BlackFox_Face_{}_{}_{}", side, owner, face_index),
            rgba,
            opacity: rgba[3] as f64 / 255.0,
            colorize_type: 0,
        })
    };
    if let Some(material) = material {
        let slot = u16::try_from(materials.len() + 1)?;
        materials.push(material);
        material_links.push((slot, materials.len() - 1));
        Ok(Some(slot))
    } else {
        Ok(None)
    }
}

fn attach_native_curves(
    mesh: &mut Mesh,
    input_edges: &[[u32; 2]],
    input_curves: &[InputCurve],
    name: &str,
) -> Result<Vec<skpwright::Curve>, Box<dyn Error>> {
    use std::collections::{HashMap, HashSet};

    let mut result = Vec::new();
    let mut assigned = HashSet::new();

    for (curve_index, input_curve) in input_curves.iter().enumerate() {
        if input_curve.edges.is_empty() {
            return Err(format!(
                "Object '{}': curve {} has no edges",
                name, curve_index
            ).into());
        }

        let mut selected = Vec::new();
        let mut degree: HashMap<u32, usize> = HashMap::new();
        let mut adjacency: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut local = HashSet::new();

        for &input_index in &input_curve.edges {
            let [a, b] = *input_edges.get(input_index).ok_or_else(|| {
                format!(
                    "Object '{}': curve {} references missing input edge {}",
                    name, curve_index, input_index
                )
            })?;

            let key = (a.min(b), a.max(b));

            if !local.insert(key) {
                return Err(format!(
                    "Object '{}': duplicate edge in curve {}",
                    name, curve_index
                ).into());
            }

            if assigned.contains(&key) {
                return Err(format!(
                    "Object '{}': edge belongs to multiple curves",
                    name
                ).into());
            }

            let mesh_index = mesh.edges.iter().position(|e| {
                (e.v0.min(e.v1), e.v0.max(e.v1)) == key
            }).ok_or_else(|| {
                format!("Object '{}': curve edge missing from mesh", name)
            })?;

            selected.push((mesh_index, key));

            *degree.entry(a).or_default() += 1;
            *degree.entry(b).or_default() += 1;

            adjacency.entry(a).or_default().push(b);
            adjacency.entry(b).or_default().push(a);
        }

        // A valid welded curve is one continuous, non-branching
        // open chain or closed loop.
        if degree.values().any(|&d| d > 2) {
            return Err(format!(
                "Object '{}': curve {} branches",
                name, curve_index
            ).into());
        }

        let endpoints = degree.values().filter(|&&d| d == 1).count();

        if endpoints != 0 && endpoints != 2 {
            return Err(format!(
                "Object '{}': curve {} has invalid endpoints",
                name, curve_index
            ).into());
        }

        let start = *degree.keys().next().ok_or("Empty curve")?;
        let mut visited = HashSet::new();
        let mut stack = vec![start];

        while let Some(v) = stack.pop() {
            if visited.insert(v) {
                if let Some(neighbors) = adjacency.get(&v) {
                    stack.extend(neighbors.iter().copied());
                }
            }
        }

        if visited.len() != degree.len() {
            return Err(format!(
                "Object '{}': curve {} is disconnected",
                name, curve_index
            ).into());
        }


        let arc = if let Some(a) = &input_curve.arc {
            let vectors = [
                a.center_m, a.normal, a.x_axis_m, a.y_axis_m
            ];

            if !vectors.iter().flatten().all(|v| v.is_finite())
                || !a.radius_m.is_finite()
                || !a.start_rad.is_finite()
                || !a.end_rad.is_finite()
                || a.radius_m <= 0.0
                || a.end_rad <= a.start_rad
                || a.end_rad - a.start_rad > std::f64::consts::TAU + 1e-9
            {
                return Err(format!(
                    "Object '{}': curve {} has invalid arc parameters",
                    name, curve_index
                ).into());
            }

            let dot = |u: [f64; 3], v: [f64; 3]| -> f64 {
                u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
            };

            let norm = |v: [f64; 3]| -> f64 {
                dot(v, v).sqrt()
            };

            let unit_tolerance = 1e-6;

            if (norm(a.normal) - 1.0).abs() > unit_tolerance
                || (norm(a.x_axis_m) - 1.0).abs() > unit_tolerance
                || (norm(a.y_axis_m) - 1.0).abs() > unit_tolerance
                || dot(a.normal, a.x_axis_m).abs() > unit_tolerance
                || dot(a.normal, a.y_axis_m).abs() > unit_tolerance
                || dot(a.x_axis_m, a.y_axis_m).abs() > unit_tolerance
            {
                return Err(format!(
                    "Object '{}': curve {} has a non-orthonormal arc frame",
                    name, curve_index
                ).into());
            }

            let cross = [
                a.x_axis_m[1] * a.y_axis_m[2] - a.x_axis_m[2] * a.y_axis_m[1],
                a.x_axis_m[2] * a.y_axis_m[0] - a.x_axis_m[0] * a.y_axis_m[2],
                a.x_axis_m[0] * a.y_axis_m[1] - a.x_axis_m[1] * a.y_axis_m[0],
            ];

            if dot(cross, a.normal) < 1.0 - unit_tolerance {
                return Err(format!(
                    "Object '{}': curve {} has an invalid arc frame orientation",
                    name, curve_index
                ).into());
            }

            // Mission 121.17: verify mesh vertices against the declared arc.
            // The input axes are unit vectors; the serialized axes are radius-scaled.
            let r = a.radius_m;
            let linear_tol = (r * 1e-6).max(1e-8);
            let angular_tol = (linear_tol / r).min(1e-3);
            let sweep = a.end_rad - a.start_rad;
            let full_circle = (sweep - std::f64::consts::TAU).abs() <= angular_tol;
            if full_circle != (endpoints == 0) {
                return Err(format!(
                    "Object '{}': curve {} closure does not match arc sweep",
                    name, curve_index
                ).into());
            }
            let mut angles: HashMap<u32, f64> = HashMap::new();
            for &vertex_index in degree.keys() {
                let p = *mesh.vertices.get(vertex_index as usize).ok_or_else(|| {
                    format!("Object '{}': curve {} has invalid vertex index {}", name, curve_index, vertex_index)
                })?;
                let delta = [p[0] - a.center_m[0], p[1] - a.center_m[1], p[2] - a.center_m[2]];
                let plane_distance = dot(delta, a.normal);
                let x = dot(delta, a.x_axis_m);
                let y = dot(delta, a.y_axis_m);
                if plane_distance.abs() > linear_tol || (x.hypot(y) - r).abs() > linear_tol {
                    return Err(format!(
                        "Object '{}': curve {} vertex {} is not on the specified arc circle",
                        name, curve_index, vertex_index
                    ).into());
                }
                let theta = y.atan2(x);
                let relative = (theta - a.start_rad).rem_euclid(std::f64::consts::TAU);
                if !full_circle && relative > sweep + angular_tol
                    && (std::f64::consts::TAU - relative) > angular_tol
                {
                    return Err(format!(
                        "Object '{}': curve {} vertex {} lies outside arc sweep",
                        name, curve_index, vertex_index
                    ).into());
                }
                angles.insert(vertex_index, relative);
            }
            if !full_circle {
                let end_vertices: Vec<u32> = degree.iter()
                    .filter_map(|(&v, &d)| if d == 1 { Some(v) } else { None })
                    .collect();
                let near_start = end_vertices.iter().any(|v| {
                    let t = angles[v]; t <= angular_tol || (std::f64::consts::TAU - t) <= angular_tol
                });
                let near_end = end_vertices.iter().any(|v| (angles[v] - sweep).abs() <= angular_tol);
                if !near_start || !near_end {
                    return Err(format!(
                        "Object '{}': curve {} endpoints do not match arc angles",
                        name, curve_index
                    ).into());
                }
            }
            // Every selected edge must connect adjacent angular samples. This
            // prevents a chord jumping over another vertex on the same arc.
            let mut sorted: Vec<f64> = angles.values().copied().collect();
            sorted.sort_by(|a, b| a.total_cmp(b));
            for pair in sorted.windows(2) {
                if (pair[1] - pair[0]).abs() <= angular_tol {
                    return Err(format!("Object '{}': curve {} has duplicate angular vertices", name, curve_index).into());
                }
            }
            for &input_index in &input_curve.edges {
                let [u, v] = input_edges[input_index];
                let t0 = angles[&u];
                let t1 = angles[&v];
                let adjacent = sorted.windows(2).any(|pair| {
                    (pair[0] - t0).abs() <= angular_tol && (pair[1] - t1).abs() <= angular_tol
                    || (pair[0] - t1).abs() <= angular_tol && (pair[1] - t0).abs() <= angular_tol
                });
                let closing = full_circle && (
                    (t0 - sorted[0]).abs() <= angular_tol && (t1 - sorted[sorted.len()-1]).abs() <= angular_tol
                    || (t1 - sorted[0]).abs() <= angular_tol && (t0 - sorted[sorted.len()-1]).abs() <= angular_tol
                );
                if !adjacent && !closing {
                    return Err(format!("Object '{}': curve {} has out-of-order arc edges", name, curve_index).into());
                }
            }
            Some(skpwright::Arc {
                center_m: a.center_m,
                normal: a.normal,
                // SketchUp stores radius-scaled arc frame axes.
                x_axis_m: a.x_axis_m.map(|v| v * a.radius_m),
                y_axis_m: a.y_axis_m.map(|v| v * a.radius_m),
                radius_m: a.radius_m,
                start_rad: a.start_rad,
                end_rad: a.end_rad,
            })
        } else {
            None
        };

        // Commit edge assignments only after all curve and arc checks pass.
        for (mesh_index, key) in selected {
            mesh.edges[mesh_index].curve = Some(curve_index as u32);
            assigned.insert(key);
        }
        result.push(skpwright::Curve {
            pid: 0,
            members: u32::try_from(input_curve.edges.len())?,
            arc,
        });
    }

    Ok(result)
}

// Recognize a retraced bridge connecting two contours within a face hole.
// The original loop is retained for native serialization.
fn split_retraced_bridge(hole: &[u32]) -> Option<(Vec<u32>, Vec<u32>)> {
    use std::collections::HashMap;

    let n = hole.len();
    if n < 8 {
        return None;
    }

    let mut positions: HashMap<u32, Vec<usize>> = HashMap::new();
    for (i, &v) in hole.iter().enumerate() {
        positions.entry(v).or_default().push(i);
    }

    let repeated: Vec<u32> = positions.iter()
        .filter(|(_, p)| p.len() > 1)
        .map(|(&v, _)| v)
        .collect();

    if repeated.len() != 2 {
        return None;
    }

    let a = repeated[0];
    let b = repeated[1];

    if positions[&a].len() != 2 || positions[&b].len() != 2 {
        return None;
    }

    let forward: Vec<usize> = (0..n)
        .filter(|&i| hole[i] == a && hole[(i + 1) % n] == b)
        .collect();

    let backward: Vec<usize> = (0..n)
        .filter(|&i| hole[i] == b && hole[(i + 1) % n] == a)
        .collect();

    if forward.len() != 1 || backward.len() != 1 {
        return None;
    }

    let f = forward[0];
    let r = backward[0];

    // Contour one: B after forward bridge, up to B before return bridge.
    let mut first = Vec::new();
    let mut i = (f + 1) % n;
    loop {
        first.push(hole[i]);
        if i == r {
            break;
        }
        i = (i + 1) % n;
        if first.len() > n {
            return None;
        }
    }

    // Contour two: A after return bridge, up to A before forward bridge.
    let mut second = Vec::new();
    let mut i = (r + 1) % n;
    loop {
        second.push(hole[i]);
        if i == f {
            break;
        }
        i = (i + 1) % n;
        if second.len() > n {
            return None;
        }
    }

    // The extracted contours explicitly repeat their starting vertex
    // at the end. Remove that closing entry for polygon validation.
    // The original native hole is never modified.
    if first.first() == first.last() {
        first.pop();
    }
    if second.first() == second.last() {
        second.pop();
    }

    if first.len() < 3 || second.len() < 3 {
        return None;
    }

    if first.iter().copied().collect::<std::collections::HashSet<_>>().len()
        != first.len()
        || second.iter().copied().collect::<std::collections::HashSet<_>>().len()
        != second.len()
    {
        return None;
    }

    Some((first, second))
}

fn validate_face_hole_topology(
    vertices: &[[f64; 3]],
    outer: &[u32],
    holes: &[Vec<u32>],
    normal: [f64; 3],
    tolerance: f64,
) -> Result<(), Box<dyn Error>> {
    if holes.is_empty() {
        return Ok(());
    }

    // Project onto the coordinate plane most perpendicular to the face normal.
    let axis = if normal[0].abs() >= normal[1].abs()
        && normal[0].abs() >= normal[2].abs() {
        0
    } else if normal[1].abs() >= normal[2].abs() {
        1
    } else {
        2
    };

    let project = |index: u32| -> [f64; 2] {
        let p = vertices[index as usize];
        match axis {
            0 => [p[1], p[2]],
            1 => [p[0], p[2]],
            _ => [p[0], p[1]],
        }
    };

    let mut loops: Vec<Vec<[f64; 2]>> = Vec::new();
    loops.push(outer.iter().map(|&i| project(i)).collect());
    for hole in holes {
        if let Some((first, second)) = split_retraced_bridge(hole) {
            loops.push(first.iter().map(|&i| project(i)).collect());
            loops.push(second.iter().map(|&i| project(i)).collect());
        } else {
            loops.push(hole.iter().map(|&i| project(i)).collect());
        }
    }

    let orient = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1])
            - (b[1] - a[1]) * (c[0] - a[0])
    };

    let eps = tolerance.max(1.0e-12);

    let on_segment = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let len = (dx * dx + dy * dy).sqrt();

        len > 0.0
            && orient(a, b, p).abs() <= eps * len
            && p[0] >= a[0].min(b[0]) - eps
            && p[0] <= a[0].max(b[0]) + eps
            && p[1] >= a[1].min(b[1]) - eps
            && p[1] <= a[1].max(b[1]) + eps
    };

    let intersects = |a: [f64; 2], b: [f64; 2],
                      c: [f64; 2], d: [f64; 2]| {
        let ab = ((b[0]-a[0]).powi(2) + (b[1]-a[1]).powi(2)).sqrt();
        let cd = ((d[0]-c[0]).powi(2) + (d[1]-c[1]).powi(2)).sqrt();
        let o1 = orient(a, b, c);
        let o2 = orient(a, b, d);
        let o3 = orient(c, d, a);
        let o4 = orient(c, d, b);

        ((o1 > eps*ab && o2 < -eps*ab || o1 < -eps*ab && o2 > eps*ab)
            && (o3 > eps*cd && o4 < -eps*cd || o3 < -eps*cd && o4 > eps*cd))
            || on_segment(a,b,c)
            || on_segment(a,b,d)
            || on_segment(c,d,a)
            || on_segment(c,d,b)
    };

    let point_inside = |p: [f64; 2], polygon: &[[f64; 2]]| {
        let mut inside = false;

        for i in 0..polygon.len() {
            let a = polygon[i];
            let b = polygon[(i + 1) % polygon.len()];

            if on_segment(a, b, p) {
                return false;
            }

            if (a[1] > p[1]) != (b[1] > p[1]) {
                let x = a[0]
                    + (p[1] - a[1]) * (b[0] - a[0]) / (b[1] - a[1]);
                if p[0] < x {
                    inside = !inside;
                }
            }
        }

        inside
    };

    // Reject self-intersections, including nonadjacent touching edges.
    for (li, polygon) in loops.iter().enumerate() {
        let n = polygon.len();

        for i in 0..n {
            let a = polygon[i];
            let b = polygon[(i + 1) % n];

            for j in (i + 1)..n {
                if j == i + 1 || (i == 0 && j == n - 1) {
                    continue;
                }

                let c = polygon[j];
                let d = polygon[(j + 1) % n];

                if intersects(a, b, c, d) {
                    return Err(format!(
                        "Face loop {} has a self-intersection at segments {} and {}", li, i, j
                    ).into());
                }
            }
        }
    }

    // Reject intersections or touching between different loops.
    for i in 0..loops.len() {
        for j in (i + 1)..loops.len() {
            let a = &loops[i];
            let b = &loops[j];

            for ai in 0..a.len() {
                for bi in 0..b.len() {
                    if intersects(
                        a[ai], a[(ai + 1) % a.len()],
                        b[bi], b[(bi + 1) % b.len()],
                    ) {
                        return Err(format!(
                            "Face loops {} and {} intersect or touch", i, j
                        ).into());
                    }
                }
            }
        }
    }

    // Every hole must lie strictly inside the outer boundary.
    for i in 1..loops.len() {
        for &p in &loops[i] {
            if !point_inside(p, &loops[0]) {
                return Err(format!(
                    "Hole {} lies outside or touches the outer boundary", i - 1
                ).into());
            }
        }
    }

    // Independent holes must not contain one another.
    for i in 1..loops.len() {
        for j in (i + 1)..loops.len() {
            if point_inside(loops[i][0], &loops[j])
                || point_inside(loops[j][0], &loops[i]) {
                return Err(format!(
                    "Holes {} and {} are nested", i - 1, j - 1
                ).into());
            }
        }
    }

    Ok(())
}

fn apply_entity_layers(
    mesh: &mut Mesh,
    input_edges: &[[u32; 2]],
    edge_properties: &[InputEdgeProperties],
    input_faces: &[InputFace],
    face_sources: &[usize],
    layer_slots: &std::collections::HashMap<String, u16>,
) -> Result<(), Box<dyn Error>> {
    let resolve = |name: Option<&str>| -> Result<u16, Box<dyn Error>> {
        match name {
            None | Some("") | Some("Layer0") => Ok(0),
            Some(name) => layer_slots.get(name).copied()
                .ok_or_else(|| format!("Unregistered entity layer: '{}'", name).into()),
        }
    };
    let mut edge_layers = std::collections::HashMap::<(u32, u32), u16>::new();
    for (index, &[a, b]) in input_edges.iter().enumerate() {
        let name = edge_properties.get(index).and_then(|p| p.layer.as_deref());
        let slot = resolve(name)?;
        let key = (a.min(b), a.max(b));
        if let Some(previous) = edge_layers.insert(key, slot) {
            if previous != slot {
                return Err(format!("Conflicting layer assignments for edge ({}, {})", a, b).into());
            }
        }
    }
    for edge in &mut mesh.edges {
        let key = (edge.v0.min(edge.v1), edge.v0.max(edge.v1));
        // Reconstructed boundary edges without explicit source stay on Layer0.
        if let Some(&slot) = edge_layers.get(&key) { edge.layer = slot; }
    }
    if mesh.faces.len() != face_sources.len() {
        return Err("Face provenance count does not match mesh faces".into());
    }
    for (face, &source_index) in mesh.faces.iter_mut().zip(face_sources.iter()) {
        let input_face = input_faces.get(source_index)
            .ok_or("Face provenance index out of range")?;
        face.layer = resolve(input_face.layer.as_deref())?;
        face.hidden = input_face.hidden;
    }
    Ok(())
}

// LAB11: provenance only; no changes to face geometry or mesh construction.
fn record_face_owners(
    owners: &mut std::collections::HashMap<u32, Vec<(usize, usize)>>,
    faces: &[InputFace],
    sources: &[usize],
    run_index: usize,
) -> Result<(), Box<dyn Error>> {
    for (output_index, &source_index) in sources.iter().enumerate() {
        let face = faces.get(source_index).ok_or("LAB11: invalid source face index")?;
        if let Some(pid) = face.source_face_pid {
            if pid == 0 { return Err("LAB11: zero source face PID".into()); }
            owners.entry(pid).or_default().push((run_index, output_index));
        }
    }
    Ok(())
}

fn build_mesh(
    name: &str,
    vertices: Vec<[f64; 3]>,
    input_edges: Vec<[u32; 2]>,
    input_faces: Vec<InputFace>,
    edge_properties: Vec<InputEdgeProperties>,
) -> Result<(Mesh, Vec<usize>), Box<dyn Error>> {
    if vertices.is_empty()
        && (!input_edges.is_empty()
            || !input_faces.is_empty()
            || !edge_properties.is_empty())
    {
        return Err(format!(
            "Object '{}' has geometry references but no vertices",
            name
        ).into());
    }

    for vertex in &vertices {
        if !vertex.iter().all(|v| v.is_finite()) {
            return Err(format!("Invalid vertex in '{}'", name).into());
        }
    }

    if !edge_properties.is_empty() && edge_properties.len() != input_edges.len() {
        return Err(format!(
            "Object '{}': edge_properties count {} must match input edge count {}",
            name, edge_properties.len(), input_edges.len()
        ).into());
    }
    let count = vertices.len();
    let mut edges = Vec::new();
    let mut first_edge_properties = std::collections::HashMap::new();

    // Maintain one undirected mesh edge per vertex pair.
    // Preserve the first occurrence of every input edge.
    let mut edge_keys = std::collections::HashSet::new();

    for (edge_index, [a, b]) in input_edges.into_iter().enumerate() {
        if a as usize >= count || b as usize >= count || a == b {
            return Err(format!("Invalid edge in '{}'", name).into());
        }

        let key = (a.min(b), a.max(b));

        let flags = edge_properties.get(edge_index).cloned().unwrap_or_default();
        if let Some(previous) = first_edge_properties.get(&key) {
            if *previous != flags {
                return Err(format!(
                    "Object '{}': conflicting properties for duplicate edge ({}, {})",
                    name, key.0, key.1
                ).into());
            }
        }
        if !edge_keys.insert(key) {
            continue;
        }
        first_edge_properties.insert(key, flags.clone());

        edges.push(MeshEdge {
            pid: 0,
            v0: a,
            v1: b,
            hidden: flags.hidden,
            soft: flags.soft,
            smooth: flags.smooth,
            ..Default::default()
        });
    }

    // Every face boundary, including inner hole loops, must have mesh edges.
    // Preserve existing edge properties and reconstruct missing edges once.
    for face in &input_faces {
        for boundary in std::iter::once(&face.outer).chain(face.holes.iter()) {
            if boundary.len() < 3 {
                continue;
            }

            // Face validation below handles invalid indices.
            for i in 0..boundary.len() {
                let a = boundary[i];
                let b = boundary[(i + 1) % boundary.len()];

                if a as usize >= count || b as usize >= count || a == b {
                    continue;
                }

                let key = (a.min(b), a.max(b));

                if edge_keys.insert(key) {
                    edges.push(MeshEdge {
                        pid: 0,
                        v0: a,
                        v1: b,
                        soft: false,
                        smooth: false,
                        ..Default::default()
                    });
                }
            }
        }
    }
    let mut faces = Vec::new();
    let mut face_sources: Vec<usize> = Vec::new();

    for (source_index, face) in input_faces.into_iter().enumerate() {
        if face.outer.len() < 3 {
            return Err(format!("Invalid face in '{}'", name).into());
        }

        if face.outer.iter().any(|&i| i as usize >= count) {
            return Err(format!("Face index out of range in '{}'", name).into());
        }

        if !face.normal.iter().all(|v| v.is_finite()) {
            return Err(format!("Invalid normal in '{}'", name).into());
        }

        // Reject repeated polygon boundary vertices.
        // A valid face boundary must not reuse a vertex index.
        let mut seen_indices = std::collections::HashSet::new();

        for &index in &face.outer {
            if !seen_indices.insert(index) {
                return Err(format!(
                    "Repeated face boundary vertex index {} in '{}'",
                    index, name
                ).into());
            }
        }

        // Different indices can still reference identical coordinates.
        // Reject zero-length boundary segments.
        for j in 0..face.outer.len() {
            let a = vertices[face.outer[j] as usize];
            let b = vertices[face.outer[(j + 1) % face.outer.len()] as usize];

            if a == b {
                return Err(format!(
                    "Coincident consecutive face vertices in '{}'",
                    name
                ).into());
            }
        }
        // Reject zero-area polygon boundaries.
        // Newell's area vector supports arbitrary face orientations.
        let mut area_vector = [0.0_f64; 3];
        let mut max_edge_squared = 0.0_f64;

        for j in 0..face.outer.len() {
            let a = vertices[face.outer[j] as usize];
            let b = vertices[face.outer[(j + 1) % face.outer.len()] as usize];

            area_vector[0] += (a[1] - b[1]) * (a[2] + b[2]);
            area_vector[1] += (a[2] - b[2]) * (a[0] + b[0]);
            area_vector[2] += (a[0] - b[0]) * (a[1] + b[1]);

            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let dz = b[2] - a[2];

            max_edge_squared = max_edge_squared.max(dx * dx + dy * dy + dz * dz);
        }

        let double_area = (
            area_vector[0] * area_vector[0]
            + area_vector[1] * area_vector[1]
            + area_vector[2] * area_vector[2]
        ).sqrt();

        if !double_area.is_finite()
            || !max_edge_squared.is_finite()
            || max_edge_squared == 0.0
            || double_area <= max_edge_squared * 1.0e-12
        {
            return Err(format!(
                "Zero-area or degenerate face in '{}'",
                name
            ).into());
        }
        // Reconstruct the unit normal
        // directly from the polygon's winding.
        let reconstructed_normal = [
            area_vector[0] / double_area,
            area_vector[1] / double_area,
            area_vector[2] / double_area,
        ];
        // Validate face planarity.
        // Reconstructed normal comes from the face boundary winding.
        let plane_origin = vertices[face.outer[0] as usize];
        let mut max_plane_deviation = 0.0_f64;

        for &index in &face.outer {
            let point = vertices[index as usize];

            let dx = point[0] - plane_origin[0];
            let dy = point[1] - plane_origin[1];
            let dz = point[2] - plane_origin[2];

            let deviation = (
                dx * reconstructed_normal[0]
                + dy * reconstructed_normal[1]
                + dz * reconstructed_normal[2]
            ).abs();

            max_plane_deviation = max_plane_deviation.max(deviation);
        }

        let characteristic_length = max_edge_squared.sqrt();
        let planarity_tolerance = characteristic_length * 1.0e-8;

        if !max_plane_deviation.is_finite() {
            return Err(format!(
                "Non-finite planarity deviation in '{}'",
                name
            ).into());
        }

        // Validate inner face loops before constructing MeshFace.
        let mut all_indices: std::collections::HashSet<u32> =
            face.outer.iter().copied().collect();

        for (hole_index, hole) in face.holes.iter().enumerate() {
            // Only vertices belonging to earlier loops are
            // forbidden here. A verified retraced bridge may repeat its
            // two endpoints within this hole.
            let prior_indices = all_indices.clone();
            let bridge_endpoints: std::collections::HashSet<u32> =
                if let Some((first, second)) = split_retraced_bridge(hole) {
                    let first_set: std::collections::HashSet<u32> =
                        first.iter().copied().collect();
                    let second_set: std::collections::HashSet<u32> =
                        second.iter().copied().collect();
                    // A valid split has two individually simple contours.
                    // The repeated endpoints are the only indices
                    // occurring twice in the original hole.
                    let _ = (first_set, second_set);
                    let mut counts = std::collections::HashMap::new();
                    for &index in hole {
                        *counts.entry(index).or_insert(0_usize) += 1;
                    }
                    counts.into_iter()
                        .filter(|(_, count)| *count == 2)
                        .map(|(index, _)| index)
                        .collect()
                } else {
                    std::collections::HashSet::new()
                };

            if hole.len() < 3 {
                return Err(format!(
                    "Face in '{}': hole {} has fewer than three vertices",
                    name, hole_index
                ).into());
            }

            let mut hole_area = [0.0_f64; 3];

            for j in 0..hole.len() {
                let a_index = hole[j];
                let b_index = hole[(j + 1) % hole.len()];

                if a_index as usize >= count || b_index as usize >= count {
                    return Err(format!(
                        "Face in '{}': hole {} index out of range",
                        name, hole_index
                    ).into());
                }

                if prior_indices.contains(&a_index) {
                    return Err(format!(
                        "Face in '{}': hole {} shares a vertex with another loop",
                        name, hole_index
                    ).into());
                }

                if !all_indices.insert(a_index)
                    && !bridge_endpoints.contains(&a_index)
                {
                    return Err(format!(
                        "Face in '{}': hole {} reuses a boundary vertex",
                        name, hole_index
                    ).into());
                }

                let a = vertices[a_index as usize];
                let b = vertices[b_index as usize];

                if a == b {
                    return Err(format!(
                        "Face in '{}': hole {} has coincident consecutive vertices",
                        name, hole_index
                    ).into());
                }

                hole_area[0] += (a[1] - b[1]) * (a[2] + b[2]);
                hole_area[1] += (a[2] - b[2]) * (a[0] + b[0]);
                hole_area[2] += (a[0] - b[0]) * (a[1] + b[1]);

                let deviation = (
                    (a[0] - plane_origin[0]) * reconstructed_normal[0]
                    + (a[1] - plane_origin[1]) * reconstructed_normal[1]
                    + (a[2] - plane_origin[2]) * reconstructed_normal[2]
                ).abs();

                if !deviation.is_finite() || deviation > planarity_tolerance {
                    return Err(format!(
                        "Face in '{}': hole {} is not coplanar",
                        name, hole_index
                    ).into());
                }
            }

            let hole_double_area = (
                hole_area[0] * hole_area[0]
                + hole_area[1] * hole_area[1]
                + hole_area[2] * hole_area[2]
            ).sqrt();

            if !hole_double_area.is_finite()
                || hole_double_area <= max_edge_squared * 1.0e-12
            {
                return Err(format!(
                    "Face in '{}': hole {} has zero area",
                    name, hole_index
                ).into());
            }
        }

        // Hole containment and intersection validation is still pending.
        // Do not consider hole support complete at this stage.
        validate_face_hole_topology(
            &vertices,
            &face.outer,
            &face.holes,
            reconstructed_normal,
            planarity_tolerance,
        )?;

        if max_plane_deviation > planarity_tolerance {
            if !face.holes.is_empty() {
                return Err(format!(
                    "Nonplanar face with holes in '{}': safe triangulation not implemented",
                    name
                ).into());
            }

            // Triangulate nonplanar quadrilaterals when a valid diagonal exists.
            if face.outer.len() != 4 {
                return Err(format!(
                    "Nonplanar polygon with {} vertices in '{}': triangulation currently supports quadrilaterals only",
                    face.outer.len(),
                    name
                ).into());
            }

            // Validate projected quadrilateral topology and
            // choose a diagonal that stays inside the boundary.
            //
            // Use the dominant normal axis for the polygon projection.
            // Additional projections detect clear boundary crossings
            // even when the Newell vector is unreliable for bow-ties.
            let points = [
                vertices[face.outer[0] as usize],
                vertices[face.outer[1] as usize],
                vertices[face.outer[2] as usize],
                vertices[face.outer[3] as usize],
            ];

            let project = |p: [f64; 3], axis: usize| -> [f64; 2] {
                match axis {
                    0 => [p[1], p[2]],
                    1 => [p[0], p[2]],
                    _ => [p[0], p[1]],
                }
            };

            let cross2 = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| -> f64 {
                (b[0] - a[0]) * (c[1] - a[1])
                    - (b[1] - a[1]) * (c[0] - a[0])
            };

            let mut projected_sets = [[[0.0_f64; 2]; 4]; 3];

            for axis in 0..3 {
                for i in 0..4 {
                    projected_sets[axis][i] = project(points[i], axis);
                }
            }

            // A crossing in any coordinate projection is treated
            // conservatively as unsupported by the exporter.
            // Collinear and touching configurations are handled by
            // the subsequent triangle-area checks.
            for projected in &projected_sets {
                let mut max_extent = 0.0_f64;

                for i in 0..4 {
                    for j in (i + 1)..4 {
                        let dx = projected[i][0] - projected[j][0];
                        let dy = projected[i][1] - projected[j][1];
                        max_extent = max_extent.max(dx * dx + dy * dy);
                    }
                }

                let eps = max_extent * 1.0e-12;

                for &(a, b, c, d) in &[(0, 1, 2, 3), (1, 2, 3, 0)] {
                    let o1 = cross2(projected[a], projected[b], projected[c]);
                    let o2 = cross2(projected[a], projected[b], projected[d]);
                    let o3 = cross2(projected[c], projected[d], projected[a]);
                    let o4 = cross2(projected[c], projected[d], projected[b]);

                    let proper_crossing =
                        ((o1 > eps && o2 < -eps) || (o1 < -eps && o2 > eps))
                        && ((o3 > eps && o4 < -eps) || (o3 < -eps && o4 > eps));

                    if proper_crossing {
                        return Err(format!(
                            "Self-intersecting quadrilateral in '{}'",
                            name
                        ).into());
                    }
                }
            }

            // Pick the projection with the largest absolute signed
            // polygon area for stable diagonal classification.
            let mut best_axis = 0_usize;
            let mut best_area = 0.0_f64;

            for axis in 0..3 {
                let p = &projected_sets[axis];
                let mut area = 0.0_f64;

                for i in 0..4 {
                    let j = (i + 1) % 4;
                    area += p[i][0] * p[j][1] - p[j][0] * p[i][1];
                }

                if area.abs() > best_area {
                    best_area = area.abs();
                    best_axis = axis;
                }
            }

            let projected = &projected_sets[best_axis];

            let mut polygon_area = 0.0_f64;
            let mut max_extent = 0.0_f64;

            for i in 0..4 {
                let j = (i + 1) % 4;

                polygon_area += projected[i][0] * projected[j][1]
                    - projected[j][0] * projected[i][1];

                for k in (i + 1)..4 {
                    let dx = projected[i][0] - projected[k][0];
                    let dy = projected[i][1] - projected[k][1];
                    max_extent = max_extent.max(dx * dx + dy * dy);
                }
            }

            let area_epsilon = max_extent * 1.0e-12;

            if polygon_area.abs() <= area_epsilon {
                return Err(format!(
                    "Degenerate projected quadrilateral in '{}'",
                    name
                ).into());
            }

            let candidates = [
                [[0_usize, 1, 2], [0, 2, 3]],
                [[0_usize, 1, 3], [1, 2, 3]],
            ];

            let mut selected = None;

            for candidate in candidates {
                let mut sum = 0.0_f64;
                let mut valid = true;

                for triangle in candidate {
                    let area = cross2(
                        projected[triangle[0]],
                        projected[triangle[1]],
                        projected[triangle[2]],
                    );

                    if area.abs() <= area_epsilon
                        || area.signum() != polygon_area.signum()
                    {
                        valid = false;
                        break;
                    }

                    sum += area.abs();
                }

                if valid
                    && (sum - polygon_area.abs()).abs()
                        <= area_epsilon * 4.0
                {
                    selected = Some(candidate);
                    break;
                }
            }

            let selected = selected.ok_or_else(|| {
                format!(
                    "No safe triangulation diagonal for quadrilateral in '{}'",
                    name
                )
            })?;

            let diagonal_a = face.outer[selected[0][0]];
            let diagonal_b = face.outer[selected[0][2]];

            // The first candidate uses 0->2. The second uses 1->3.
            let (diagonal_a, diagonal_b) =
                if selected[0] == [0, 1, 3] {
                    (face.outer[1], face.outer[3])
                } else {
                    (diagonal_a, diagonal_b)
                };

            let diagonal_exists = edges.iter().any(|edge| {
                (edge.v0 == diagonal_a && edge.v1 == diagonal_b)
                    || (edge.v0 == diagonal_b && edge.v1 == diagonal_a)
            });

            if !diagonal_exists {
                edges.push(MeshEdge {
                    pid: 0,
                    v0: diagonal_a,
                    v1: diagonal_b,
                    soft: true,
                    smooth: true,
                    ..Default::default()
                });
            }

            let triangles = [
                [
                    face.outer[selected[0][0]],
                    face.outer[selected[0][1]],
                    face.outer[selected[0][2]],
                ],
                [
                    face.outer[selected[1][0]],
                    face.outer[selected[1][1]],
                    face.outer[selected[1][2]],
                ],
            ];

            for triangle in triangles {
                let a = vertices[triangle[0] as usize];
                let b = vertices[triangle[1] as usize];
                let c = vertices[triangle[2] as usize];

                let ab = [
                    b[0] - a[0],
                    b[1] - a[1],
                    b[2] - a[2],
                ];
                let ac = [
                    c[0] - a[0],
                    c[1] - a[1],
                    c[2] - a[2],
                ];

                let cross = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];

                let length = (
                    cross[0] * cross[0]
                    + cross[1] * cross[1]
                    + cross[2] * cross[2]
                ).sqrt();

                if !length.is_finite()
                    || length <= max_edge_squared * 1.0e-12
                {
                    return Err(format!(
                        "Degenerate triangle during triangulation in '{}'",
                        name
                    ).into());
                }

                let triangle_normal = [
                    cross[0] / length,
                    cross[1] / length,
                    cross[2] / length,
                ];

                face_sources.push(source_index);
                faces.push(MeshFace {
                    pid: 0,
                    outer: triangle.to_vec(),
                    holes: vec![],
                    normal: triangle_normal,
                    front_material: face.material_slot,
                    back_material: face.back_material_slot,
                    ..Default::default()
                });
            }

            continue;
        }
        face_sources.push(source_index);
        faces.push(MeshFace {
            pid: 0,
            outer: face.outer,
            holes: face.holes,
            normal: reconstructed_normal,
            front_material: face.material_slot,
            back_material: face.back_material_slot,
            ..Default::default()
        });
    }

    Ok((Mesh {
        vertices,
        edges,
        faces,
        ..Default::default()
    }, face_sources))
}
// Skpwright: texture inches [u,v,1] * K -> face-local inches [x,y,1].
// K is the inverse of the local-inches -> texture-inches affine transform.
const METRES_PER_INCH: f64 = 0.0254;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn face_axes(normal: [f64; 3]) -> Result<([f64; 3], [f64; 3]), Box<dyn Error>> {
    let length = dot(normal, normal).sqrt();
    if !length.is_finite() || length < 1e-12 {
        return Err("UVW face normal must be nonzero".into());
    }
    let n = normal.map(|x| x / length);
    // Matches MeshFace::uv_xform face-frame convention.
    let mut v = if n[2].abs() > 1.0 - 1e-9 {
        [0.0, 1.0, 0.0]
    } else {
        [-n[2] * n[0], -n[2] * n[1], 1.0 - n[2] * n[2]]
    };
    let len = dot(v, v).sqrt();
    if len < 1e-12 { return Err("Cannot establish UVW face frame".into()); }
    v = v.map(|x| x / len);
    let u = [v[1]*n[2] - v[2]*n[1],
             v[2]*n[0] - v[0]*n[2],
             v[0]*n[1] - v[1]*n[0]];
    Ok((u, v))
}

fn uvw_matrix(
    uvw: [f64; 8], size_m: (f64, f64), normal: [f64; 3],
    origin_m: [f64; 3],
) -> Result<[f64; 9], Box<dyn Error>> {
    if !uvw.iter().all(|x| x.is_finite()) ||
       !size_m.0.is_finite() || !size_m.1.is_finite() ||
       size_m.0 <= 0.0 || size_m.1 <= 0.0 {
        return Err("Invalid UVW coefficients or texture dimensions".into());
    }
    let (eu, ev) = face_axes(normal)?;
    let gu = [uvw[0], uvw[1], uvw[2]];
    let gv = [uvw[4], uvw[5], uvw[6]];
    let width_in = size_m.0 / METRES_PER_INCH;
    let height_in = size_m.1 / METRES_PER_INCH;
    let a = size_m.0 * dot(gu, eu);
    let b = size_m.1 * dot(gv, eu);
    let c = size_m.0 * dot(gu, ev);
    let d = size_m.1 * dot(gv, ev);
    let x0 = dot(origin_m, eu) / METRES_PER_INCH;
    let y0 = dot(origin_m, ev) / METRES_PER_INCH;
    let u0 = dot(origin_m, gu) + uvw[3];
    let v0 = dot(origin_m, gv) + uvw[7];
    let e = width_in * u0 - a*x0 - c*y0;
    let f = height_in * v0 - b*x0 - d*y0;
    let det = a*d - b*c;
    let scale = (a.abs()+b.abs()+c.abs()+d.abs()).powi(2);
    if !det.is_finite() || !scale.is_finite() ||
       scale == 0.0 || det.abs() <= 1e-12 * scale {
        return Err("Degenerate UVW mapping: face-local texture gradients are not invertible".into());
    }
    let k = [d/det, -b/det, 0.0,
             -c/det, a/det, 0.0,
             (c*f-d*e)/det, (b*e-a*f)/det, 1.0];
    if !k.iter().all(|x| x.is_finite()) {
        return Err("UVW transformation overflow".into());
    }
    Ok(k)
}

fn attach_uvw(
    mesh: &mut Mesh, face_inputs: &[InputFace], face_sources: &[usize], owner: &str,
) -> Result<(), Box<dyn Error>> {
    if mesh.faces.len() != face_sources.len() {
        return Err(format!(
            "{}: mesh has {} faces but {} provenance records",
            owner, mesh.faces.len(), face_sources.len()
        ).into());
    }

    for (i, target) in mesh.faces.iter_mut().enumerate() {
        let source_index = face_sources[i];
        let input = face_inputs.get(source_index).ok_or_else(|| {
            format!("{}: invalid source face index {}", owner, source_index)
        })?;
        if input.uvw.is_none() && input.back_uvw.is_none() { continue; }
        let first = *target.outer.first().ok_or("UVW face has no vertices")? as usize;
        let origin = *mesh.vertices.get(first).ok_or("UVW vertex index out of range")?;
        let mut ft = FaceTexture {
    front: [1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,
            0.0, 0.0, 1.0],
    back: [1.0, 0.0, 0.0,
           0.0, 1.0, 0.0,
           0.0, 0.0, 1.0],
    front_extra: [0.0; 3],
    back_extra: [0.0; 3],
    front_pins: Vec::new(),
    back_pins: Vec::new(),
    flags: [0, 0],
};
        if let Some(uvw) = input.uvw {
            let t = input.texture.as_ref().ok_or_else(|| format!("{} face {}: front UVW requires front texture", owner, i))?;
            ft.front = uvw_matrix(uvw, (t.width_m, t.height_m), target.normal, origin)?;
            ft.flags[0] = 1; // Affine matrix mode: must verify in SketchUp.
        }
        if let Some(uvw) = input.back_uvw {
            let t = input.back_texture.as_ref().ok_or_else(|| format!("{} face {}: back UVW requires back texture", owner, i))?;
            ft.back = uvw_matrix(uvw, (t.width_m, t.height_m), target.normal, origin)?;
            ft.flags[1] = 1; // Affine matrix mode: must verify in SketchUp.
        }
        target.texture = Some(Box::new(ft));
    }
    Ok(())
}



fn remap_native_anchor(
    source: InputNativeAnchor,
    vertex_map: &std::collections::HashMap<u32, u32>,
    instance_map: &std::collections::HashMap<u32, u32>,
) -> Result<Anchor, Box<dyn Error>> {
    if !source.point_m.iter().all(|v| v.is_finite()) {
        return Err("Native annotation anchor has non-finite coordinates".into());
    }
    if !matches!(source.kind, 0 | 1 | 2) {
        return Err(format!(
            "Unsupported native annotation anchor kind {}",
            source.kind
        ).into());
    }
    if source.kind != 2 &&
        (source.entity.is_some() || source.second.is_some()
         || !source.path.is_empty() || !source.second_path.is_empty())
    {
        return Err("Non-vertex native anchor unexpectedly references entities".into());
    }
    if source.kind == 2 && source.entity.is_none() {
        return Err("Vertex native anchor has no entity PID".into());
    }

    let remap_entity = |original: Option<u32>| -> Result<Option<u32>, Box<dyn Error>> {
        match original {
            None => Ok(None),
            Some(old) => vertex_map.get(&old).copied().map(Some)
                .ok_or_else(|| format!("Unresolved native annotation entity PID {}", old).into()),
        }
    };

    let remap_path = |original: Vec<u32>| -> Result<Vec<u32>, Box<dyn Error>> {
        original.into_iter().map(|old| {
            instance_map.get(&old).copied()
                .ok_or_else(|| format!("Unresolved native instance path PID {}", old).into())
        }).collect()
    };

    Ok(Anchor {
        kind: source.kind,
        point_m: source.point_m,
        entity: remap_entity(source.entity)?,
        path: remap_path(source.path)?,
        second: remap_entity(source.second)?,
        second_path: remap_path(source.second_path)?,
    })
}
fn convert(input: InputModel, output: &Path, version: Option<&str>) -> Result<(), Box<dyn Error>> {
    // Preserve native annotations for associative reconstruction.
    let source_native_texts = input.native_texts;
    let source_native_dimensions = input.native_dimensions;
    // D4N61AE phase 2a: source edge provenance is carried intact.
    // Validate now; do not silently accept malformed PID arrays.
    for (kind, name, edges, pids) in input.objects.iter()
        .map(|o| ("group", o.name.as_str(), o.edges.len(), &o.source_edge_pids))
        .chain(input.component_definitions.iter()
            .map(|o| ("component", o.name.as_str(), o.edges.len(), &o.source_edge_pids)))
        .chain(input.loose_geometry.iter()
            .map(|o| ("loose", o.name.as_str(), o.edges.len(), &o.source_edge_pids)))
    {
        if !pids.is_empty() {
            if pids.len() != edges {
                return Err(format!("{} '{}': {} source edge PIDs for {} edges",
                    kind, name, pids.len(), edges).into());
            }
            let mut unique = std::collections::HashSet::new();
            for &pid in pids {
                if pid == 0 || !unique.insert(pid) {
                    return Err(format!("{} '{}': zero or duplicate source edge PID {}",
                        kind, name, pid).into());
                }
            }
        }
    }
    // D4N61AE phase 2B: keep native records for reconstruction after PID allocation.
    let source_native_radial_dimensions = input.native_radial_dimensions;

    // Capture original SketchUp identities before input collections
    // are consumed by geometry construction.
    let source_groups: Vec<(Vec<u32>, Option<u32>, Option<usize>)> =
        input.objects.iter()
            .map(|o| (
                o.source_vertex_pids.clone(),
                o.source_instance_pid,
                o.source_def_index,
            ))
            .collect();

    let source_components: Vec<(Vec<u32>, Option<usize>)> =
        input.component_definitions.iter()
            .map(|c| (
                c.source_vertex_pids.clone(),
                c.source_def_index,
            ))
            .collect();

    // Edge provenance is parallel to each input object's edge endpoint array.
    // Copy it before build_mesh consumes and deduplicates the geometry.
    let source_group_edges: Vec<(Vec<[u32; 2]>, Vec<u32>)> = input.objects.iter()
        .map(|o| (o.edges.clone(), o.source_edge_pids.clone())).collect();
    let source_component_edges: Vec<(Vec<[u32; 2]>, Vec<u32>)> = input.component_definitions.iter()
        .map(|o| (o.edges.clone(), o.source_edge_pids.clone())).collect();
    let source_loose_edges: Vec<(Vec<[u32; 2]>, Vec<u32>)> = input.loose_geometry.iter()
        .map(|o| (o.edges.clone(), o.source_edge_pids.clone())).collect();

    let source_instances: Vec<Option<u32>> =
        input.component_instances.iter()
            .map(|i| i.source_instance_pid)
            .collect();

    // Gate 0.6: retain component placement ownership in original input order.
    // None means the model root; Some identifies a parent component definition.
    let source_instance_parents: Vec<Option<String>> =
        input.component_instances.iter()
            .map(|i| i.parent_definition_id.clone())
            .collect();

    let source_loose_vertices: Vec<u32> =
        input.loose_geometry.first()
            .map(|o| o.source_vertex_pids.clone())
            .unwrap_or_default();


    let mut definition_ids = std::collections::HashSet::new();

    for definition in &input.component_definitions {
        if definition.id.trim().is_empty() {
            return Err("Component definition has an empty ID".into());
        }

        if !definition_ids.insert(definition.id.as_str()) {
            return Err(format!(
                "Duplicate component definition ID: '{}'",
                definition.id
            ).into());
        }
    }

    for instance in &input.component_instances {
        if let Some(parent) = instance.parent_definition_id.as_deref() {
            if !definition_ids.contains(parent) {
                return Err(format!(
                    "Component instance references unknown parent definition: '{}'",
                    parent
                ).into());
            }
            if parent == instance.definition_id {
                return Err("Component definition cannot contain an instance of itself".into());
            }
        }

        if !definition_ids.contains(instance.definition_id.as_str()) {
            return Err(format!(
                "Component instance references unknown definition: '{}'",
                instance.definition_id
            ).into());
        }

        if !instance.transform.iter().all(|value| value.is_finite()) {
            return Err("Component instance contains invalid transform values".into());
        }

        if (instance.transform[12] - 1.0).abs() > 1e-9 {
            return Err("Component transform must have final element 1.0".into());
        }
    }





    // Gate 0.6: reject indirect cycles in component-definition hierarchy.
    // An edge parent -> child means the parent definition contains an
    // instance of the child definition. Root-level placements add no edge.
    let mut component_children =
        std::collections::HashMap::<&str, Vec<&str>>::new();

    for instance in &input.component_instances {
        if let Some(parent) = instance.parent_definition_id.as_deref() {
            component_children
                .entry(parent)
                .or_default()
                .push(instance.definition_id.as_str());
        }
    }

    fn check_component_cycle<'a>(
        node: &'a str,
        children: &std::collections::HashMap<&'a str, Vec<&'a str>>,
        visiting: &mut std::collections::HashSet<&'a str>,
        visited: &mut std::collections::HashSet<&'a str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if visited.contains(node) {
            return Ok(());
        }

        if !visiting.insert(node) {
            return Err(format!(
                "Circular component definition hierarchy involving '{}'",
                node
            ).into());
        }

        if let Some(next) = children.get(node) {
            for &child in next {
                check_component_cycle(child, children, visiting, visited)?;
            }
        }

        visiting.remove(node);
        visited.insert(node);
        Ok(())
    }

    let mut visiting = std::collections::HashSet::new();
    let mut visited = std::collections::HashSet::new();

    for definition in &input.component_definitions {
        check_component_cycle(
            definition.id.as_str(),
            &component_children,
            &mut visiting,
            &mut visited,
        )?;
    }
    let input_object_transforms: Vec<Option<[f64; 13]>> =
        input.objects.iter().map(|o| o.transform).collect();
    let input_locked: Vec<bool> =
        input.objects.iter().map(|o| o.locked).collect();
    let input_hidden: Vec<bool> =
        input.objects.iter().map(|o| o.hidden).collect();
    let input_names: Vec<String> =
        input.objects.iter().map(|o| o.name.clone()).collect();

    let input_layers: Vec<Option<String>> =
        input.objects.iter().map(|o| o.layer.clone()).collect();

    let input_colors: Vec<Option<[u8; 4]>> =
        input.objects.iter().map(|o| o.color).collect();

    let input_textures: Vec<Option<InputTexture>> =
        input.objects.iter()
            .map(|object| object.texture.as_ref().map(|t| InputTexture {
                file: t.file.clone(),
                width_m: t.width_m,
                height_m: t.height_m,
            }))
            .collect();

    // Keep face metadata alongside the geometry runs; build_mesh consumes InputFace.
    let object_face_materials: Vec<Vec<InputFace>> = input.objects.iter()
        .map(|object| object.faces.iter().map(|face| InputFace {
            source_face_pid: face.source_face_pid,
            hidden: face.hidden,
            outer: face.outer.clone(), holes: face.holes.clone(), normal: face.normal,
            layer: face.layer.clone(),
            material_slot: face.material_slot, color: face.color,
            texture: face.texture.as_ref().map(|t| InputTexture {
                file: t.file.clone(), width_m: t.width_m, height_m: t.height_m,
            }), uvw: face.uvw,
            back_material_slot: face.back_material_slot,
            back_color: face.back_color,
            back_texture: face.back_texture.as_ref().map(|t| InputTexture {
                file: t.file.clone(), width_m: t.width_m, height_m: t.height_m,
            }),
            back_uvw: face.back_uvw,
        }).collect()).collect();

    let object_edge_layers: Vec<(Vec<[u32; 2]>, Vec<InputEdgeProperties>)> = input.objects.iter()
        .map(|object| (object.edges.clone(), object.edge_properties.clone()))
        .collect();

    let mut geometry = Vec::new();
    let mut object_face_sources: Vec<Vec<usize>> = Vec::new();
    let mut face_pid_owners = std::collections::HashMap::<u32, Vec<(usize, usize)>>::new();

    let source_group_identity: Vec<_> = input.objects.iter()
        .map(|object| (
            object.name.clone(),
            object.source_definition_guid.clone(),
            object.source_definition_flag,
            object.source_instance_guid.clone(),
        ))
        .collect();

    for object in input.objects {
        let name = object.name;
        let input_edges_for_curves = object.edges.clone();
        let (mut mesh, face_sources) = build_mesh(
            &name,
            object.vertices,
            object.edges,
            object.faces,
            object.edge_properties,
        )?;

        let native_curves = attach_native_curves(
            &mut mesh,
            &input_edges_for_curves,
            &object.curves,
            &name,
        )?;

        println!(
            "OBJECT '{}': {} vertices, {} edges, {} faces",
            name,
            mesh.vertices.len(),
            mesh.edges.len(),
            mesh.faces.len()
        );

        record_face_owners(&mut face_pid_owners, &object_face_materials[object_face_sources.len()], &face_sources, geometry.len())?;
        object_face_sources.push(face_sources);
        geometry.push(GeometryRun {
            def_index: None,
            mesh,
            curves: native_curves,
            ..Default::default()
        });
    }
    let mut definitions = Vec::new();
    let mut definition_links = Vec::new();
    let mut group_runs = Vec::new();
    let mut root_run = GeometryRun::default();

    let input_layer_settings: std::collections::HashMap<String, ([u8; 4], bool)> =
        input.layers.iter()
            .map(|layer| (layer.name.clone(), (layer.color, layer.visible)))
            .collect();
    let mut layers = vec![Layer {
        name: "Layer0".into(),
        visible: true,
        rgba: [255, 255, 255, 255],
    }];
    let mut layer_links = vec![(0, 0)];

    // Register explicitly declared layers, including empty ones.
    for input_layer in &input.layers {
        if input_layer.name.trim().is_empty() {
            return Err("Layer name cannot be empty".into());
        }

        // Layer0 already exists as the default layer.
        if input_layer.name == "Layer0" {
            layers[0].rgba = input_layer.color;
            layers[0].visible = input_layer.visible;
            continue;
        }

        if layers.iter().any(|layer| layer.name == input_layer.name) {
            return Err(format!(
                "Duplicate layer name: '{}'",
                input_layer.name
            ).into());
        }

        let slot = u16::try_from(layers.len())?;

        layers.push(Layer {
            name: input_layer.name.clone(),
            visible: input_layer.visible,
            rgba: input_layer.color,
        });

        layer_links.push((slot, layers.len() - 1));
    }
    // Register names referenced by individual faces and edges, including
    // names not explicitly listed in input.layers. Match existing object-layer
    // behavior for undeclared names.
    let mut entity_layer_names = std::collections::BTreeSet::<String>::new();
    for object in &object_face_materials {
        for face in object {
            if let Some(name) = &face.layer { entity_layer_names.insert(name.clone()); }
        }
    }
    for (_, properties) in &object_edge_layers {
        for edge in properties {
            if let Some(name) = &edge.layer { entity_layer_names.insert(name.clone()); }
        }
    }
    for component in &input.component_definitions {
        for face in &component.faces {
            if let Some(name) = &face.layer { entity_layer_names.insert(name.clone()); }
        }
        for edge in &component.edge_properties {
            if let Some(name) = &edge.layer { entity_layer_names.insert(name.clone()); }
        }
    }
    for loose in &input.loose_geometry {
        for face in &loose.faces {
            if let Some(name) = &face.layer { entity_layer_names.insert(name.clone()); }
        }
        for edge in &loose.edge_properties {
            if let Some(name) = &edge.layer { entity_layer_names.insert(name.clone()); }
        }
    }
    for name in entity_layer_names {
        if name.is_empty() || name == "Layer0" { continue; }
        if name.trim().is_empty() {
            return Err("Entity layer name cannot be whitespace-only".into());
        }
        if layers.iter().any(|layer| layer.name == name) { continue; }
        let slot = u16::try_from(layers.len())?;
        let (rgba, visible) = input_layer_settings.get(&name)
            .copied().unwrap_or(([180, 180, 180, 255], true));
        layers.push(Layer { name, visible, rgba });
        layer_links.push((slot, slot as usize));
    }
    let entity_layer_slots: std::collections::HashMap<String, u16> = layers.iter()
        .enumerate()
        .map(|(index, layer)| Ok((layer.name.clone(), u16::try_from(index)?)))
        .collect::<Result<_, std::num::TryFromIntError>>()?;


    // Translate source native archive slots into destination archive slots.
    let mut native_layer_slot_map =
        std::collections::HashMap::<u16, u16>::new();

    for link in &input.source_layer_links {
        let source_layer = input.layers.get(link.index)
            .ok_or_else(|| format!(
                "Source layer link {} references invalid layer index {}",
                link.slot, link.index
            ))?;

        let destination_slot = *entity_layer_slots.get(&source_layer.name)
            .ok_or_else(|| format!(
                "Source layer '{}' is missing from destination",
                source_layer.name
            ))?;

        if native_layer_slot_map.insert(link.slot, destination_slot).is_some() {
            return Err(format!(
                "Duplicate source layer slot {}",
                link.slot
            ).into());
        }
    }

    let translate_native_layer = |slot: u16| -> Result<u16, Box<dyn Error>> {
        if input.source_layer_links.is_empty() {
            return Ok(slot);
        }

        native_layer_slot_map.get(&slot).copied()
            .ok_or_else(|| format!(
                "Unmapped source native layer slot {}",
                slot
            ).into())
    };

    let restored_active_layer = if let Some(source_index) = input.source_active_layer {
        let source_slot = u16::try_from(source_index)?;
        let destination_slot = translate_native_layer(source_slot)?;
        let destination_index = usize::from(destination_slot);
        if destination_index >= layers.len() {
            return Err("Active layer references missing destination layer".into());
        }
        Some(destination_index)
    } else {
        None
    };

    let mut materials = Vec::new();
    let mut material_links = Vec::new();

    // Preserve source SketchUp material order and one-based slots.
    for source in &input.source_materials {
        if source.name.is_empty() {
            return Err("Source material name cannot be empty".into());
        }

        let opacity = source.opacity.unwrap_or(1.0);
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(format!(
                "Invalid opacity for '{}'", source.name
            ).into());
        }

        let material = match source.kind.as_str() {
            "solid" => {
                if source.texture.is_some() || source.applied_size_in.is_some() {
                    return Err(format!(
                        "Solid material '{}' has texture properties",
                        source.name
                    ).into());
                }

                Material::Solid {
                    name: source.name.clone(),
                    rgba: source.rgba.ok_or_else(|| format!(
                        "Solid material '{}' missing RGBA",
                        source.name
                    ))?,
                    opacity,
                    colorize_type: source.colorize_type.unwrap_or(0),
                }
            }

            "textured" => {
                let texture_path = source.texture.as_ref().ok_or_else(|| format!(
                    "Textured material '{}' missing texture path",
                    source.name
                ))?;

                let dimensions = source.applied_size_in.ok_or_else(|| format!(
                    "Textured material '{}' missing dimensions",
                    source.name
                ))?;

                if dimensions.iter().any(|v| !v.is_finite() || *v <= 0.0) {
                    return Err(format!(
                        "Invalid texture dimensions for '{}'",
                        source.name
                    ).into());
                }

                let bytes = fs::read(texture_path)?;

                if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    && !bytes.starts_with(&[0xFF, 0xD8, 0xFF])
                {
                    return Err(format!(
                        "Texture '{}' must be PNG or JPEG",
                        source.name
                    ).into());
                }

                Material::Textured {
                    name: source.name.clone(),
                    texture: Some(texture_path.clone()),
                    applied_size_in: Some((dimensions[0], dimensions[1])),
                    jpeg_quality: None,
                    image_bytes: Some(bytes),
                    avg_rgba: None,
                    colorized: false,
                    texture_avg_rgba: None,
                    colorize_type: source.colorize_type.unwrap_or(0),
                    opacity,
                }
            }

            other => return Err(format!(
                "Unsupported material kind '{}' for '{}'",
                other, source.name
            ).into()),
        };

        let slot = u16::try_from(materials.len() + 1)?;
        materials.push(material);
        material_links.push((slot, materials.len() - 1));
    }

    for (index, mut run) in geometry.into_iter().enumerate() {
        apply_entity_layers(
            &mut run.mesh,
            &object_edge_layers[index].0,
            &object_edge_layers[index].1,
            &object_face_materials[index],
            &object_face_sources[index],
            &entity_layer_slots,
        )?;
        let defref = u32::try_from(index + 1)?;
        let name = input_names[index].clone();

        let layer_slot: u16 = match input_layers[index].as_deref() {
            None | Some("") | Some("Layer0") => 0,
            Some(layer_name) => {
                if let Some(existing) =
                    layers.iter().position(|layer| layer.name == layer_name)
                {
                    u16::try_from(existing)?
                } else {
                    let slot = u16::try_from(layers.len())?;
                    layers.push(Layer {
                        name: layer_name.to_string(),
                        visible: input_layer_settings
                            .get(layer_name)
                            .map(|settings| settings.1)
                            .unwrap_or(true),
                        rgba: input_layer_settings
                            .get(layer_name)
                            .map(|settings| settings.0)
                            .unwrap_or([180, 180, 180, 255]),
                    });
                    layer_links.push((slot, slot as usize));
                    slot
                }
            }
        };

        let material_slot: u16 = if let Some(texture) = &input_textures[index] {
            if !texture.width_m.is_finite()
                || !texture.height_m.is_finite()
                || texture.width_m <= 0.0
                || texture.height_m <= 0.0
            {
                return Err(format!("Invalid texture dimensions for '{}'", name).into());
            }

            let bytes = fs::read(&texture.file)?;

            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && !bytes.starts_with(&[0xFF, 0xD8, 0xFF])
            {
                return Err(format!("Texture for '{}' must be PNG or JPEG", name).into());
            }

            let slot = u16::try_from(materials.len() + 1)?;

            materials.push(Material::Textured {
                name: format!("BlackFox_Texture_{}", name),
                texture: Some(texture.file.clone()),
                applied_size_in: Some((
                    texture.width_m / 0.0254,
                    texture.height_m / 0.0254,
                )),
                jpeg_quality: None,
                image_bytes: Some(bytes),
                avg_rgba: Some([180, 180, 180, 255]),
                colorized: false,
                texture_avg_rgba: Some([180, 180, 180, 255]),
                colorize_type: 0,
                opacity: 1.0,
            });

            material_links.push((slot, materials.len() - 1));
            slot
        } else {
            match input_colors[index] {
                None => 0,
                Some(rgba) => {
                    let slot = u16::try_from(materials.len() + 1)?;
                    materials.push(Material::Solid {
                        name: format!("BlackFox_{}", name),
                        rgba,
                        opacity: rgba[3] as f64 / 255.0,
                        colorize_type: 0,
                    });
                    material_links.push((slot, materials.len() - 1));
                    slot
                }
            }
        };

        attach_uvw(
            &mut run.mesh,
            &object_face_materials[index],
            &object_face_sources[index],
            &name,
        )?;

        let mut face_slots = Vec::new();
        for (face_index, face) in object_face_materials[index].iter().enumerate() {
            let front = register_face_material(
                face, &name, face_index, false, &mut materials, &mut material_links,
            )?;
            let back = register_face_material(
                face, &name, face_index, true, &mut materials, &mut material_links,
            )?;
            face_slots.push((front, back));
        }
        for (output_index, &source_index) in object_face_sources[index].iter().enumerate() {
            let (front, back) = *face_slots.get(source_index).ok_or("Invalid object face provenance")?;
            if let Some(slot) = front { run.mesh.faces[output_index].front_material = Some(slot); }
            if let Some(slot) = back { run.mesh.faces[output_index].back_material = Some(slot); }
        }

        run.def_index = Some(index + 1);

        let matching_source_objects: Vec<_> = source_group_identity.iter()
            .filter(|object| object.0 == name)
            .collect();
        if matching_source_objects.len() != 1 {
            return Err(format!(
                "Expected one source group object named '{}', found {}",
                name, matching_source_objects.len()
            ).into());
        }
        let source_object = matching_source_objects[0];

        definitions.push(Definition {
            name: name.clone(),
            guid: validated_source_guid(
                source_object.1.as_deref()
            )?,
            flag: source_object.2.unwrap_or(true),
            kind: DefinitionKind::Group,
            description: "BlackFox exported group".into(),
            source_path: String::new(),
            map_index: Some(index + 1),
            ..Default::default()
        });

        definition_links.push((defref, index));

        root_run.placed.push(PlacedInstance {
            pid: 0,
            guid: validated_source_guid(
                source_object.3.as_deref()
            )?,
            defref,
            transform: {
                let mut transform = input_object_transforms[index].unwrap_or([
                    1.0, 0.0, 0.0,
                    0.0, 1.0, 0.0,
                    0.0, 0.0, 1.0,
                    0.0, 0.0, 0.0,
                    1.0,
                ]);
                const METRES_PER_INCH: f64 = 0.0254;
                for axis in 9..12 {
                    transform[axis] /= METRES_PER_INCH;
                }
                if !transform.iter().all(|v| v.is_finite()) {
                    return Err("Group transform contains non-finite values".into());
                }
                transform
            },
            is_group: true,
            material: material_slot,
            hidden: input_hidden[index],
            locked: input_locked[index],
            layer: layer_slot,
            casts_shadows: true,
            receives_shadows: true,
            ..Default::default()
        });

        println!("GROUP: {}", name);
        group_runs.push(run);
    }

    // Component definitions share the numbering space with groups.
    // Every component definition gets one reusable geometry run.
    let mut next_defref = u32::try_from(definitions.len() + 1)?;
    let mut group_definition_refs = std::collections::HashSet::new();
    let mut component_refs = std::collections::HashMap::new();

    for component in input.component_definitions {
        component_refs.insert(component.id.clone(), next_defref);
        let component_face_inputs = component.faces.clone();

        // Register face materials before consuming component face input.
        let mut face_slots = Vec::new();
        for (face_index, face) in component.faces.iter().enumerate() {
            let front = register_face_material(
                face, &component.name, face_index, false, &mut materials, &mut material_links,
            )?;
            let back = register_face_material(
                face, &component.name, face_index, true, &mut materials, &mut material_links,
            )?;
            face_slots.push((front, back));
        }
        let component_edges_for_curves = component.edges.clone();
        let component_edge_properties = component.edge_properties.clone();
        let (mut mesh, face_sources) = build_mesh(
            &component.name,
            component.vertices,
            component.edges,
            component.faces,
            component.edge_properties,
        )?;

        apply_entity_layers(
            &mut mesh,
            &component_edges_for_curves,
            &component_edge_properties,
            &component_face_inputs,
            &face_sources,
            &entity_layer_slots,
        )?;
        let native_curves = attach_native_curves(
            &mut mesh,
            &component_edges_for_curves,
            &component.curves,
            &component.name,
        )?;

        attach_uvw(
            &mut mesh,
            &component_face_inputs,
            &face_sources,
            &component.name,
        )?;

        for (output_index, &source_index) in face_sources.iter().enumerate() {
            let (front, back) = *face_slots.get(source_index).ok_or("Invalid component face provenance")?;
            let face = &mut mesh.faces[output_index];
            if let Some(slot) = front { face.front_material = Some(slot); }
            if let Some(slot) = back { face.back_material = Some(slot); }
        }

        let def_index = definitions.len() + 1;

        definitions.push(Definition {
            name: component.name.clone(),
            guid: validated_source_guid(
                component.source_definition_guid.as_deref()
            )?,
            flag: component.source_definition_flag.unwrap_or(true),
            kind: if component.is_group {
                group_definition_refs.insert(next_defref);
                DefinitionKind::Group
            } else {
                DefinitionKind::Component
            },
            description: component.source_description.clone(),
            source_path: String::new(),
            behaviour: Behaviour {
                glues_to_surface: component.behaviour.glues_to_surface,
                glue_plane: component.behaviour.glue_plane,
                cuts_opening: component.behaviour.cuts_opening,
                always_faces_camera: component.behaviour.always_faces_camera,
                shadows_face_sun: component.behaviour.shadows_face_sun,
                extra: component.behaviour.extra,
            },
            map_index: Some(def_index),
            ..Default::default()
        });

        definition_links.push((next_defref, def_index - 1));

        record_face_owners(&mut face_pid_owners, &component_face_inputs, &face_sources, group_runs.len())?;
        group_runs.push(GeometryRun {
            def_index: Some(def_index),
            mesh,
            curves: native_curves,
            ..Default::default()
        });

        println!(
            "COMPONENT DEFINITION: {} (reference {})",
            component.name,
            next_defref
        );

        next_defref = next_defref.checked_add(1)
            .ok_or("Too many component definitions")?;
    }

    // Gate 0.6: preserve each component instance's actual placement location.
    // None identifies the root run, which is appended after all definitions.
    let mut component_placement_locations: Vec<(Option<usize>, usize)> = Vec::new();
    let mut placement_names = Vec::new();

    // Place instances using shared native component definitions.
    for instance in input.component_instances {
        let defref = *component_refs
            .get(&instance.definition_id)
            .ok_or("Component definition reference not found")?;

        let layer_slot: u16 = match instance.layer.as_deref() {
            None | Some("") | Some("Layer0") => 0,
            Some(layer_name) => {
                if let Some(existing) =
                    layers.iter().position(|layer| layer.name == layer_name)
                {
                    u16::try_from(existing)?
                } else {
                    let slot = u16::try_from(layers.len())?;
                    layers.push(Layer {
                        name: layer_name.to_string(),
                        visible: input_layer_settings
                            .get(layer_name)
                            .map(|settings| settings.1)
                            .unwrap_or(true),
                        rgba: input_layer_settings
                            .get(layer_name)
                            .map(|settings| settings.0)
                            .unwrap_or([180, 180, 180, 255]),
                    });
                    layer_links.push((slot, slot as usize));
                    slot
                }
            }
        };

        let material_slot = match (instance.material_slot, instance.color) {
            (Some(_), Some(_)) => return Err("Instance has both material slot and color".into()),
            (Some(slot), None) => {
                if slot != 0 && !material_links.iter().any(|(registered, _)| *registered == slot) {
                    return Err(format!("Instance material slot {slot} is not registered").into());
                }
                slot
            }
            (None, None) => 0,
            (None, Some(rgba)) => {
                let slot = u16::try_from(materials.len() + 1)?;
                materials.push(Material::Solid {
                    name: format!(
                        "BlackFox_Component_{}",
                        instance.definition_id
                    ),
                    rgba,
                    opacity: rgba[3] as f64 / 255.0,
                    colorize_type: 0,
                });
                material_links.push((slot, materials.len() - 1));
                slot
            }
        };

        // Resolve the owning geometry run before constructing the placement.
        let parent_run_index = if let Some(parent_id) =
            instance.parent_definition_id.as_deref()
        {
            let parent_ref = *component_refs.get(parent_id)
                .ok_or("Parent component definition reference not found")?;

            let definition_index = definition_links.iter()
                .find(|(reference, _)| *reference == parent_ref)
                .map(|(_, index)| *index)
                .ok_or("Parent component definition link missing")?;

            Some(group_runs.iter()
                .position(|run| run.def_index == Some(definition_index + 1))
                .ok_or("Parent component geometry run missing")?)
        } else {
            None
        };

        let placement = PlacedInstance {
            pid: 0,
            guid: validated_source_guid(
                instance.source_instance_guid.as_deref()
            )?,
            defref,
            transform: {
                // BlackFox JSON uses metres; Skpwright stores instance
                // translation in SketchUp internal inches.
                // Rotation and scale are dimensionless.
                let mut transform = instance.transform;
                const METRES_PER_INCH: f64 = 0.0254;
                for axis in 9..12 {
                    transform[axis] /= METRES_PER_INCH;
                    if !transform[axis].is_finite() {
                        return Err("Component translation exceeds supported range".into());
                    }
                }
                transform
            },
            is_group: group_definition_refs.contains(&defref),
            material: material_slot,
            hidden: instance.hidden,
            locked: instance.locked,
            layer: layer_slot,
            casts_shadows: true,
            receives_shadows: true,
            ..Default::default()
        };

        let placement_index = match parent_run_index {
            Some(run_index) => {
                let index = group_runs[run_index].placed.len();
                group_runs[run_index].placed.push(placement);
                index
            }
            None => {
                let index = root_run.placed.len();
                root_run.placed.push(placement);
                index
            }
        };

        placement_names.push(instance.name);
        component_placement_locations.push((
            parent_run_index,
            placement_index,
        ));

        println!(
            "COMPONENT INSTANCE: {} -> reference {}",
            instance.definition_id,
            defref
        );
    }

    let mut section_ids = std::collections::HashMap::new();

    for section in input.section_planes {
        if section.id.trim().is_empty() {
            return Err("Section plane ID cannot be empty".into());
        }

        if section_ids.contains_key(&section.id) {
            return Err(format!(
                "Duplicate section plane ID: {}",
                section.id
            ).into());
        }

        if !section.plane.iter().all(|v| v.is_finite()) {
            return Err("Section plane contains non-finite values".into());
        }

        let normal_length = (
            section.plane[0] * section.plane[0] +
            section.plane[1] * section.plane[1] +
            section.plane[2] * section.plane[2]
        ).sqrt();

        if (normal_length - 1.0).abs() > 1e-8 {
            return Err(format!(
                "Section plane '{}' requires a unit normal (length {})",
                section.id, normal_length
            ).into());
        }

        let layer: u16 = match section.layer.as_deref() {
            None | Some("") | Some("Layer0") => 0,
            Some(name) => layers.iter()
                .position(|l| l.name == name)
                .ok_or_else(|| format!(
                    "Unknown section plane layer: {}",
                    name
                ))?
                .try_into()?,
        };

        let index = root_run.sections.len();

        root_run.sections.push(SectionPlane {
            pid: 0,
            plane: section.plane,
            hidden: section.hidden,
            name: section.name,
            symbol: section.symbol,
            layer,
            ..Default::default()
        });

        section_ids.insert(section.id, index);
    }

    let active_section_index =
        match input.active_section_plane.as_deref() {
            None => None,
            Some(id) => Some(
                *section_ids.get(id).ok_or_else(|| {
                    format!("Unknown active section plane: {}", id)
                })?
            ),
        };
    // Native loose geometry is independent of groups and components.
    if input.loose_geometry.len() > 1 {
        return Err("Only one loose geometry collection is currently supported".into());
    }

    if let Some(loose) = input.loose_geometry.into_iter().next() {
        // Preserve face materials and UVs on the native root mesh.
        // Existing unpainted loose geometry continues through the same path.
        let loose_face_inputs = loose.faces.clone();
        let loose_edges = loose.edges.clone();
        let loose_edge_properties = loose.edge_properties.clone();
        let (mut mesh, face_sources) = build_mesh(
            &loose.name,
            loose.vertices,
            loose.edges,
            loose.faces,
            loose.edge_properties,
        )?;

        apply_entity_layers(
            &mut mesh,
            &loose_edges,
            &loose_edge_properties,
            &loose_face_inputs,
            &face_sources,
            &entity_layer_slots,
        )?;
        let curves = attach_native_curves(
            &mut mesh,
            &loose_edges,
            &loose.curves,
            &loose.name,
        )?;

        attach_uvw(&mut mesh, &loose_face_inputs, &face_sources, &loose.name)?;

        let mut face_slots = Vec::new();
        for (face_index, face) in loose_face_inputs.iter().enumerate() {
            let front = register_face_material(
                face, &loose.name, face_index, false, &mut materials, &mut material_links,
            )?;
            let back = register_face_material(
                face, &loose.name, face_index, true, &mut materials, &mut material_links,
            )?;
            face_slots.push((front, back));
        }
        for (output_index, &source_index) in face_sources.iter().enumerate() {
            let (front, back) = *face_slots.get(source_index)
                .ok_or("Invalid loose face provenance")?;
            let face = &mut mesh.faces[output_index];
            if let Some(slot) = front { face.front_material = Some(slot); }
            if let Some(slot) = back { face.back_material = Some(slot); }
        }

        record_face_owners(&mut face_pid_owners, &loose_face_inputs, &face_sources, group_runs.len())?;
        root_run.mesh = mesh;
        root_run.curves = curves;
    }

    root_run.def_index = None;
    group_runs.push(root_run);

    // Mission 119.8C: explicit unique persistent IDs for vertices and placements.
    // Keep these IDs separate from writer-generated IDs; the writer starts above max_pid.
    let object_count = input_names.len();
    let mut next_pid: u32 = 1_000_000;
    for run in &mut group_runs {
        run.mesh.vertex_pids = (0..run.mesh.vertices.len())
            .map(|_| {
                let pid = next_pid;
                next_pid = next_pid.checked_add(1).expect("Persistent ID overflow");
                pid
            }).collect();
        for placed in &mut run.placed {
            placed.pid = next_pid;
            next_pid = next_pid.checked_add(1).expect("Persistent ID overflow");
        }
    }
    // Give every final mesh edge a unique PID, including generated boundaries
    // and triangulation diagonals. Do this AFTER all geometry is constructed.
    for run in &mut group_runs {
        for edge in &mut run.mesh.edges {
            edge.pid = next_pid;
            next_pid = next_pid.checked_add(1)
                .ok_or("Mesh edge persistent ID overflow")?;
        }
    }

    // Map source edge identity by undirected endpoint pair WITHIN its owning run.
    // Never use input index as an output index: build_mesh deduplicates edges.
    let mut edge_pid_map = std::collections::HashMap::<u32, (u32, Option<usize>)>::new();
    let mut map_edges = |source: &(Vec<[u32; 2]>, Vec<u32>),
                         run: &GeometryRun, label: &str|
        -> Result<(), Box<dyn Error>>
    {
        let (endpoints, source_pids) = source;
        if source_pids.is_empty() { return Ok(()); }
        if endpoints.len() != source_pids.len() {
            return Err(format!("{}: edge PID count mismatch", label).into());
        }
        let mut exported = std::collections::HashMap::<(u32, u32), u32>::new();
        for e in &run.mesh.edges {
            let key = (e.v0.min(e.v1), e.v0.max(e.v1));
            if exported.insert(key, e.pid).is_some() {
                return Err(format!("{}: duplicate exported edge endpoints {:?}", label, key).into());
            }
        }
        for (&[a, b], &old_pid) in endpoints.iter().zip(source_pids.iter()) {
            let key = (a.min(b), a.max(b));
            let new_pid = *exported.get(&key).ok_or_else(||
                format!("{}: source edge {} with endpoints {:?} was not exported", label, old_pid, key))?;
            if old_pid == 0 || edge_pid_map.insert(old_pid, (new_pid, run.def_index)).is_some() {
                return Err(format!("{}: duplicate or zero source edge PID {}", label, old_pid).into());
            }
        }
        Ok(())
    };
    for (i, source) in source_group_edges.iter().enumerate() {
        map_edges(source, &group_runs[i], "Group")?;
    }
    for (i, source) in source_component_edges.iter().enumerate() {
        map_edges(source, &group_runs[source_group_edges.len() + i], "Component")?;
    }
    if source_loose_edges.len() > 1 {
        return Err("Multiple loose geometry records cannot be mapped to one root run".into());
    }
    if let Some(source) = source_loose_edges.first() {
        map_edges(source, group_runs.last().ok_or("Missing root run")?, "Loose geometry")?;
    }

    // Remap original SketchUp persistent IDs to exported IDs.
    let mut vertex_pid_map = std::collections::HashMap::<u32, u32>::new();
    let mut instance_pid_map = std::collections::HashMap::<u32, u32>::new();
    let mut definition_index_map = std::collections::HashMap::<usize, usize>::new();

    let mut map_vertices = |source: &[u32], run: &GeometryRun, label: &str|
        -> Result<(), Box<dyn Error>>
    {
        if source.is_empty() {
            return Ok(());
        }

        if source.len() != run.mesh.vertex_pids.len() {
            return Err(format!(
                "{}: source vertex count {} differs from exported count {}",
                label, source.len(), run.mesh.vertex_pids.len()
            ).into());
        }

        for (&old_pid, &new_pid) in source.iter().zip(run.mesh.vertex_pids.iter()) {
            if old_pid == 0 {
                return Err(format!("{}: source vertex PID cannot be zero", label).into());
            }
            if vertex_pid_map.insert(old_pid, new_pid).is_some() {
                return Err(format!("Duplicate source vertex PID {}", old_pid).into());
            }
        }

        Ok(())
    };

    for (index, (vertices, _placement, old_definition)) in source_groups.iter().enumerate() {
        let run = group_runs.get(index).ok_or("Missing exported group geometry run")?;
        map_vertices(vertices, run, "Group")?;

        if let Some(old) = old_definition {
            let new = run.def_index.ok_or("Group missing definition index")?;
            if definition_index_map.insert(*old, new).is_some() {
                return Err(format!("Duplicate source definition index {}", old).into());
            }
        }
    }

    for (index, (vertices, old_definition)) in source_components.iter().enumerate() {
        let run_index = source_groups.len() + index;
        let run = group_runs.get(run_index).ok_or("Missing exported component geometry run")?;
        map_vertices(vertices, run, "Component")?;

        if let Some(old) = old_definition {
            let new = run.def_index.ok_or("Component missing definition index")?;
            if definition_index_map.insert(*old, new).is_some() {
                return Err(format!("Duplicate source definition index {}", old).into());
            }
        }
    }

    let root_run_index = group_runs.len() - 1;
    map_vertices(&source_loose_vertices, &group_runs[root_run_index], "Loose geometry")?;

    let placements = &group_runs[root_run_index].placed;
    let expected_root_placements = source_groups.len()
        + source_instance_parents.iter().filter(|parent| parent.is_none()).count();

    if placements.len() != expected_root_placements {
        return Err("Source and exported root placement counts differ".into());
    }

    if component_placement_locations.len() != source_instances.len() {
        return Err("Component placement location count differs from input".into());
    }

    for (index, (_, old_pid, _)) in source_groups.iter().enumerate() {
        if let Some(old) = old_pid {
            let new = placements[index].pid;
            if *old == 0 || instance_pid_map.insert(*old, new).is_some() {
                return Err(format!("Invalid or duplicate source group PID {}", old).into());
            }
        }
    }

    for (index, old_pid) in source_instances.iter().enumerate() {
        if let Some(old) = old_pid {
            let (owner, placement_index) = component_placement_locations[index];
            let run_index = owner.unwrap_or(root_run_index);
            let new = group_runs[run_index].placed
                .get(placement_index)
                .ok_or("Component placement provenance index out of range")?
                .pid;

            if *old == 0 || instance_pid_map.insert(*old, new).is_some() {
                return Err(format!("Invalid or duplicate source component PID {}", old).into());
            }
        }
    }

    println!(
        "NATIVE PROVENANCE: {} vertices, {} instances, {} definitions",
        vertex_pid_map.len(),
        instance_pid_map.len(),
        definition_index_map.len()
    );

    // Assign persistent IDs to native section planes.
    let root_index_for_sections = group_runs.len() - 1;

    for section in &mut group_runs[root_index_for_sections].sections {
        section.pid = next_pid;
        next_pid = next_pid.checked_add(1)
            .ok_or("Section plane persistent ID overflow")?;
    }

    if let Some(index) = active_section_index {
        let pid =
            group_runs[root_index_for_sections].sections[index].pid;

        group_runs[root_index_for_sections].active_section =
            Some(pid);
    }
    // Restore native construction points and section planes.
    // Source definition indices refer to the original SketchUp archive.
    // Exported definition indices are resolved through definition_index_map.
    let mut native_point_count = 0usize;
    let mut native_section_count = 0usize;
    let mut seen_native_entity_pids = std::collections::HashSet::<u32>::new();

    for native_run in input.native_geometry_entities {
        let target_index = match native_run.source_def_index {
            Some(old_definition) => {
                let new_definition = *definition_index_map.get(&old_definition)
                    .ok_or_else(|| format!(
                        "Unresolved native geometry definition {}",
                        old_definition
                    ))?;

                group_runs.iter().position(|run| {
                    run.def_index == Some(new_definition)
                }).ok_or_else(|| format!(
                    "Missing exported geometry run for definition {}",
                    old_definition
                ))?
            }
            None => root_index_for_sections,
        };

        let run = &mut group_runs[target_index];
        let mut section_pid_map = std::collections::HashMap::<u32, u32>::new();

        for point in native_run.guide_points {
            if point.pid == 0 || !seen_native_entity_pids.insert(point.pid) {
                return Err(format!(
                    "Invalid or duplicate native construction point PID {}",
                    point.pid
                ).into());
            }

            if !point.point_m.iter().chain(point.anchor_m.iter())
                .all(|value| value.is_finite())
            {
                return Err(format!(
                    "Non-finite native construction point PID {}",
                    point.pid
                ).into());
            }

            run.guide_points.push(skpwright::GuidePoint {
                pid: next_pid,
                point_m: point.point_m,
                anchor_m: point.anchor_m,
                layer: translate_native_layer(point.layer)?,
                material: None,
                hidden: point.hidden,
                casts_shadows: point.casts_shadows,
                receives_shadows: point.receives_shadows,
                flag: point.flag,
            });

            next_pid = next_pid.checked_add(1)
                .ok_or("Native construction point PID overflow")?;

            native_point_count += 1;
        }

        for section in native_run.sections {
            if section.pid == 0 || !seen_native_entity_pids.insert(section.pid) {
                return Err(format!(
                    "Invalid or duplicate native section plane PID {}",
                    section.pid
                ).into());
            }

            if !section.plane.iter().all(|value| value.is_finite()) {
                return Err(format!(
                    "Non-finite native section plane PID {}",
                    section.pid
                ).into());
            }

            if section.plane[..3].iter()
                .map(|value| value * value)
                .sum::<f64>() < 1e-20
            {
                return Err(format!(
                    "Zero-normal native section plane PID {}",
                    section.pid
                ).into());
            }

            let new_pid = next_pid;

            section_pid_map.insert(section.pid, new_pid);

            run.sections.push(skpwright::SectionPlane {
                pid: new_pid,
                plane: section.plane,
                name: section.name,
                symbol: section.symbol,
                layer: translate_native_layer(section.layer)?,
                material: None,
                hidden: section.hidden,
                casts_shadows: section.casts_shadows,
                receives_shadows: section.receives_shadows,
                hatch: None,
            });

            next_pid = next_pid.checked_add(1)
                .ok_or("Native section plane PID overflow")?;

            native_section_count += 1;
        }

        if let Some(original_active_pid) = native_run.active_section {
            let mapped_pid = *section_pid_map.get(&original_active_pid)
                .ok_or_else(|| format!(
                    "Native active section PID {} missing from owning run",
                    original_active_pid
                ))?;

            if run.active_section.is_some() {
                return Err(format!(
                    "Conflicting active section planes in exported run {}",
                    target_index
                ).into());
            }

            run.active_section = Some(mapped_pid);
        }
    }

    println!(
        "NATIVE GEOMETRY ENTITIES: {} construction points, {} section planes",
        native_point_count, native_section_count
    );
    // The final run is the root; its first placements are the exported object groups.
    let root_index = group_runs.len() - 1;
    let root_placements = &group_runs[root_index].placed;
    if root_placements.len() < object_count {
        return Err("Group placement count mismatch".into());
    }

    let mut native_texts = Vec::new();

    for input_text in input.texts {
        if !input_text.position.iter().all(|v| v.is_finite()) {
            return Err("Text annotation contains invalid coordinates".into());
        }

        let offset = input_text.offset.unwrap_or([0.5, 0.5, 0.0]);

        if !offset.iter().all(|v| v.is_finite()) {
            return Err("Text annotation contains invalid offset".into());
        }

        let layer = match input_text.layer.as_deref() {
            None | Some("") | Some("Layer0") => 0,
            Some(name) => *entity_layer_slots.get(name)
                .ok_or_else(|| format!("Text annotation references unknown layer '{name}'"))?,
        };

        native_texts.push(Text {
            content: input_text.content,
            layer,
            screen_position: [0.5, 0.5],
            leader: if input_text.leader {
                Leader::Pushpin
            } else {
                Leader::None
            },
            leader_offset: if input_text.leader {
                offset
            } else {
                [0.0; 3]
            },
            anchor: Anchor {
                kind: 1,
                point_m: input_text.position,
                ..Default::default()
            },
            ..Default::default()
        });
    }

    let mut native_dimensions = Vec::new();

    for d in input.dimensions {
        if !d.start.iter().chain(d.end.iter()).all(|v| v.is_finite())
            || !d.offset_m.is_finite()
        {
            return Err("Dimension contains invalid coordinates or offset".into());
        }

        if d.start == d.end {
            return Err("Dimension endpoints must be different".into());
        }

        let dx = d.end[0] - d.start[0];
        let dy = d.end[1] - d.start[1];
        let length = (dx * dx + dy * dy).sqrt();

        if length <= 1.0e-12 || !length.is_finite() {
            return Err("First dimension test requires a nonzero XY projection".into());
        }

        let (start_anchor, end_anchor) = match (d.object_index, d.component_instance_index, d.start_vertex, d.end_vertex) {
            (None, None, None, None) => (
                Anchor { kind: 1, point_m: d.start, ..Default::default() },
                Anchor { kind: 1, point_m: d.end, ..Default::default() },
            ),
            (Some(object_index), None, Some(start_vertex), Some(end_vertex)) => {
                if object_index >= object_count { return Err("Dimension object_index out of range".into()); }
                let mesh = &group_runs[object_index].mesh;
                let a = *mesh.vertices.get(start_vertex).ok_or("Dimension start_vertex out of range")?;
                let b = *mesh.vertices.get(end_vertex).ok_or("Dimension end_vertex out of range")?;
                if a != d.start || b != d.end {
                    return Err("Dimension coordinates must match referenced group vertices".into());
                }
                let placement_pid = root_placements[object_index].pid;
                if placement_pid == 0 { return Err("Group placement missing persistent ID".into()); }
                let make_anchor = |index: usize, point: [f64; 3]| Anchor {
                    kind: 2,
                    point_m: point,
                    entity: Some(mesh.vertex_pids[index]),
                    path: vec![placement_pid],
                    ..Default::default()
                };
                (make_anchor(start_vertex, a), make_anchor(end_vertex, b))
            },
            (None, Some(instance_index), Some(start_vertex), Some(end_vertex)) => {
                // Nested associative anchors require a complete instance path.
                // Reject them explicitly until that reconstruction is implemented.
                if source_instance_parents.get(instance_index)
                    .ok_or("Dimension component_instance_index out of range")?
                    .is_some()
                {
                    return Err("Associative dimensions referencing nested components are not yet supported".into());
                }

                let root_offset = source_instance_parents[..instance_index]
                    .iter()
                    .filter(|parent| parent.is_none())
                    .count();

                let placement = root_placements.get(object_count + root_offset)
                    .ok_or("Dimension component_instance_index out of range")?;
                if placement.is_group { return Err("Dimension expected a component instance".into()); }
                if placement.pid == 0 { return Err("Component instance missing persistent ID".into()); }
                let definition_index = definition_links.iter()
                    .find(|(ref_id, _)| *ref_id == placement.defref)
                    .map(|(_, index)| *index)
                    .ok_or("Component definition reference missing")?;
                let mesh = &group_runs.iter()
                    .find(|run| run.def_index == Some(definition_index + 1))
                    .ok_or("Component geometry run missing")?.mesh;
                let a = *mesh.vertices.get(start_vertex).ok_or("Component start_vertex out of range")?;
                let b = *mesh.vertices.get(end_vertex).ok_or("Component end_vertex out of range")?;
                // Initial component test: translation-only transform, with world-space
                // dimension coordinates and local-space anchor coordinates.
                let t = &placement.transform;
                let identity = [1.0,0.0,0.0, 0.0,1.0,0.0, 0.0,0.0,1.0];
                if t[..9] != identity || t[12] != 1.0 {
                    return Err("Component dimension references require translation-only transforms".into());
                }
                let world_a = [a[0]+t[9], a[1]+t[10], a[2]+t[11]];
                let world_b = [b[0]+t[9], b[1]+t[10], b[2]+t[11]];
                if world_a != d.start || world_b != d.end {
                    return Err("Dimension coordinates must match transformed component vertices".into());
                }
                let make_anchor = |index: usize, point: [f64; 3]| Anchor {
                    kind: 2,
                    point_m: point,
                    entity: Some(mesh.vertex_pids[index]),
                    path: vec![placement.pid],
                    ..Default::default()
                };
                (make_anchor(start_vertex, a), make_anchor(end_vertex, b))
            },
            _ => return Err("Associative dimension requires either object_index or component_instance_index, plus start_vertex and end_vertex".into()),
        };
        native_dimensions.push(Dimension {
            start: start_anchor,
            end: end_anchor,
            normal: [0.0, 0.0, 1.0],
            x_axis: [dx / length, dy / length, 0.0],
            offset_m: d.offset_m,
            text_override: String::new(),
            aligned: true,
            arrow: 3,
            ..Default::default()
        });
    }

    // Mission 120: root-level construction geometry.
    let mut native_guides = Vec::new();

    for point in input.construction_points {
        if !point.point.iter().all(|v| v.is_finite()) {
            return Err("Construction point contains invalid coordinates".into());
        }

        let anchor = point.anchor.unwrap_or(point.point);

        if !anchor.iter().all(|v| v.is_finite()) {
            return Err("Construction point contains invalid anchor".into());
        }

        let layer: u16 = match point.layer.as_deref() {
            None | Some("") | Some("Layer0") => 0,
            Some(name) => layers.iter().position(|l| l.name == name)
                .ok_or("Construction point references unknown layer")?
                .try_into()?,
        };

        group_runs[root_index].guide_points.push(GuidePoint {
            pid: next_pid,
            point_m: point.point,
            anchor_m: anchor,
            layer,
            ..Default::default()
        });

        next_pid = next_pid.checked_add(1).ok_or("Persistent ID overflow")?;
    }

    for line in input.construction_lines {
        if !line.point.iter().chain(line.direction.iter()).all(|v| v.is_finite()) {
            return Err("Construction line contains invalid coordinates".into());
        }

        let length = line.direction.iter().map(|v| v * v).sum::<f64>().sqrt();

        if !length.is_finite() || length < 1e-12 {
            return Err("Construction line direction must be finite and nonzero".into());
        }

        let layer: u16 = match line.layer.as_deref() {
            None | Some("") | Some("Layer0") => 0,
            Some(name) => layers.iter().position(|l| l.name == name)
                .ok_or("Construction line references unknown layer")?
                .try_into()?,
        };

        native_guides.push(Guide {
            pid: next_pid,
            def_index: None,
            point_m: line.point,
            direction: line.direction.map(|v| v / length),
            bounds: [-1e30, 1e30],
            layer,
            material: None,
            hidden: false,
            casts_shadows: true,
            receives_shadows: true,
            stipple: 0x3333,
        });

        next_pid = next_pid.checked_add(1).ok_or("Persistent ID overflow")?;
    }
    // Reconstruct original associative SketchUp annotations.
    // Source material slots are one-based and registered in original order.
    // Preserve native annotation material slots with strict validation.
    for t in source_native_texts {
        if !t.screen_position.iter()
            .chain(t.leader_offset.iter())
            .chain(t.view_direction.iter())
            .all(|v| v.is_finite())
        {
            return Err("Native text contains non-finite coordinates".into());
        }
        if !input.source_layer_links.is_empty() && !native_layer_slot_map.contains_key(&t.layer) {
            return Err(format!("Native text layer {} is invalid", t.layer).into());
        }
        if t.font != Some(0) && t.font.is_some() {
            return Err("Native text references an unsupported font".into());
        }
        let owner = match t.def_index {
            Some(old) => Some(*definition_index_map.get(&old)
                .ok_or_else(|| format!("Unresolved native text definition {}", old))?),
            None => None,
        };
        let leader = match t.leader.as_str() {
            "none" => Leader::None,
            "pushpin" => Leader::Pushpin,
            "view_based" => Leader::ViewBased,
            other => return Err(format!("Unsupported native text leader '{}'", other).into()),
        };
        let anchor = remap_native_anchor(
            t.anchor, &vertex_pid_map, &instance_pid_map
        )?;
        native_texts.push(Text {
            pid: next_pid,
            def_index: owner,
            content: t.content,
            placeholder: t.placeholder,
            screen_position: t.screen_position,
            anchor,
            leader_offset: t.leader_offset,
            leader,
            arrow: t.arrow,
            font: t.font,
            layer: translate_native_layer(t.layer)?,
            material: {
                if let Some(slot) = t.material {
                    if slot != 0
                        && !material_links.iter().any(
                            |(registered, _)| *registered == slot
                        )
                    {
                        return Err(format!(
                            "Native text references unregistered material slot {}",
                            slot
                        ).into());
                    }
                }
                t.material
            },
            hidden: t.hidden,
            casts_shadows: t.casts_shadows,
            receives_shadows: t.receives_shadows,
            view_direction: t.view_direction,
            leader_hidden: t.leader_hidden,
        });
        next_pid = next_pid.checked_add(1)
            .ok_or("Native text persistent ID overflow")?;
    }

    for d in source_native_dimensions {
        if !d.normal.iter()
            .chain(d.x_axis.iter())
            .all(|v| v.is_finite())
            || !d.offset_m.is_finite()
            || !d.undecoded.is_finite()
        {
            return Err("Native dimension contains non-finite values".into());
        }
        if !input.source_layer_links.is_empty() && !native_layer_slot_map.contains_key(&d.layer) {
            return Err(format!("Native dimension layer {} is invalid", d.layer).into());
        }
        if d.font != Some(0) && d.font.is_some() {
            return Err("Native dimension references an unsupported font".into());
        }
        if !matches!(d.variant, 1 | 2) {
            return Err(format!("Unsupported native dimension variant {}", d.variant).into());
        }
        let owner = match d.def_index {
            Some(old) => Some(*definition_index_map.get(&old)
                .ok_or_else(|| format!("Unresolved native dimension definition {}", old))?),
            None => None,
        };
        let start = remap_native_anchor(
            d.start, &vertex_pid_map, &instance_pid_map
        )?;
        let end = remap_native_anchor(
            d.end, &vertex_pid_map, &instance_pid_map
        )?;
        native_dimensions.push(Dimension {
            pid: next_pid,
            def_index: owner,
            text_override: d.text_override,
            start,
            end,
            normal: d.normal,
            x_axis: d.x_axis,
            offset_m: d.offset_m,
            text_position: d.text_position,
            aligned: d.aligned,
            font: d.font,
            arrow: d.arrow,
            layer: translate_native_layer(d.layer)?,
            material: {
                if let Some(slot) = d.material {
                    if slot != 0
                        && !material_links.iter().any(
                            |(registered, _)| *registered == slot
                        )
                    {
                        return Err(format!(
                            "Native dimension references unregistered material slot {}",
                            slot
                        ).into());
                    }
                }
                d.material
            },
            hidden: d.hidden,
            casts_shadows: d.casts_shadows,
            receives_shadows: d.receives_shadows,
            variant: d.variant,
            undecoded: d.undecoded,
        });
        next_pid = next_pid.checked_add(1)
            .ok_or("Native dimension persistent ID overflow")?;
    }

    let mut native_radial_dimensions = Vec::new();
    for d in source_native_radial_dimensions {
        if d.pid == 0 || !d.leader.iter().all(|v| v.is_finite()) {
            return Err("Native radial dimension has invalid PID or leader".into());
        }
        if d.arc.is_some() {
            return Err(format!("Native radial dimension {}: edgeless embedded arcs are not supported", d.pid).into());
        }
        let old_edge = d.edge.ok_or_else(||
            format!("Native radial dimension {} has no edge", d.pid))?;
        let &(new_edge, actual_owner) = edge_pid_map.get(&old_edge).ok_or_else(||
            format!("Native radial dimension {} references unmapped edge {}", d.pid, old_edge))?;
        let requested_owner = match d.def_index {
            Some(old) => Some(*definition_index_map.get(&old).ok_or_else(||
                format!("Native radial dimension {}: unknown definition {}", d.pid, old))?),
            None => None,
        };
        if actual_owner != requested_owner {
            return Err(format!("Native radial dimension {}: edge {} belongs to {:?}, dimension belongs to {:?}",
                d.pid, old_edge, actual_owner, requested_owner).into());
        }
        if d.font != Some(0) && d.font.is_some() {
            return Err(format!("Native radial dimension {}: unsupported font", d.pid).into());
        }
        if !input.source_layer_links.is_empty() && !native_layer_slot_map.contains_key(&d.layer) {
            return Err(format!("Native radial dimension {}: invalid layer {}", d.pid, d.layer).into());
        }
        if let Some(slot) = d.material {
            if slot != 0 && !material_links.iter().any(|(registered, _)| *registered == slot) {
                return Err(format!("Native radial dimension {}: unregistered material {}", d.pid, slot).into());
            }
        }
        native_radial_dimensions.push(RadialDimension {
            pid: next_pid,
            def_index: requested_owner,
            edge: Some(new_edge),
            arc: None,
            diameter: d.diameter,
            leader: d.leader,
            text_override: d.text_override,
            font: d.font,
            aligned: d.aligned,
            arrow: d.arrow,
            layer: translate_native_layer(d.layer)?,
            material: d.material,
            hidden: d.hidden,
            casts_shadows: d.casts_shadows,
            receives_shadows: d.receives_shadows,
        });
        next_pid = next_pid.checked_add(1)
            .ok_or("Native radial dimension persistent ID overflow")?;
    }

    println!(
        "NATIVE ANNOTATIONS: {} texts, {} dimensions",
        native_texts.len(), native_dimensions.len()
    );

    // D4N.33: resolve dictionary provenance only after final placement PID allocation.
    let mut restored_dictionaries = Vec::<skpwright::Attributes>::new();
    for group in &input.source_dictionaries {
        let parts = group.owner.as_array().ok_or("Dictionary owner must be an array")?;
        let kind = parts.first().and_then(|v| v.as_str()).ok_or("Missing dictionary owner kind")?;
        let owner = match kind {
            "model" if parts.len() == 1 => skpwright::Owner::Model,
            "definition_map" if parts.len() == 2 => {
                let old = usize::try_from(parts[1].as_u64().ok_or("Invalid source definition index")?)?;
                let new = *definition_index_map.get(&old).ok_or("Unmapped dictionary definition")?;
                // skpwright Owner::Definition is zero-based; run.def_index is one-based.
                skpwright::Owner::Definition(new.checked_sub(1).ok_or("Invalid target definition index")?)
            },
            "face_pid" if parts.len() == 2 => {
                let pid = u32::try_from(parts[1].as_u64().ok_or("LAB11: invalid face PID")?)?;
                let locations = face_pid_owners.get(&pid)
                    .ok_or_else(|| format!("LAB11: face PID {} has no output face", pid))?;
                if locations.len() != 1 {
                    return Err(format!("LAB11: face PID {} maps to {} output faces; dictionary ownership ambiguous", pid, locations.len()).into());
                }
                let (ri, fi) = locations[0];
                skpwright::Owner::Face(ri, fi)
            },
            "instance_pid" if parts.len() == 2 => {
                let old = u32::try_from(parts[1].as_u64().ok_or("Invalid source instance PID")?)?;
                let new = *instance_pid_map.get(&old).ok_or("Unmapped dictionary instance")?;
                let mut found = None;
                for (ri, run) in group_runs.iter().enumerate() {
                    for (pi, placement) in run.placed.iter().enumerate() {
                        if placement.pid == new {
                            if found.replace((ri, pi)).is_some() { return Err("Ambiguous dictionary instance PID".into()); }
                        }
                    }
                }
                let (ri, pi) = found.ok_or("Dictionary instance placement not found")?;
                skpwright::Owner::Instance(ri, pi)
            },
            _ => return Err(format!("Unsupported dictionary owner: {}", group.owner).into()),
        };
        let mut dictionaries = Vec::new();
        for d in &group.dictionaries {
            let mut entries = Vec::new();
            for (key, raw) in &d.entries {
                entries.push((key.clone(), typed_dictionary_value(raw)?));
            }
            dictionaries.push(skpwright::Dictionary { name: d.name.clone(), flag: d.flag, entries });
        }
        restored_dictionaries.push(skpwright::Attributes { owner, dictionaries });
    }

    // Phase 2N.30: restore original embedded SketchUp classification archives.
    let mut restored_classifications = Vec::new();
    let mut classification_names = std::collections::HashSet::new();

    for record in &input.source_classifications {
        if record.name.is_empty() || record.name.contains('/') || record.name.contains('\\') {
            return Err("Invalid classification archive name".into());
        }
        if !classification_names.insert(record.name.clone()) {
            return Err(format!("Duplicate classification: {}", record.name).into());
        }
        if record.data_hex.len() != record.bytes.checked_mul(2)
            .ok_or("Classification archive length overflow")?
        {
            return Err(format!("Classification length mismatch: {}", record.name).into());
        }

        let data = decode_classification_hex(&record.data_hex)?;

        if data.len() != record.bytes || !data.starts_with(b"PK") {
            return Err(format!("Invalid classification archive: {}", record.name).into());
        }

        restored_classifications.push(skpwright::Classification {
            name: record.name.clone(),
            source_path: record.source_path.clone(),
            data,
        });
    }
    let requested_units = input.source_units.clone();

    // The writers look up placement names by the final persistent ID.
    let mut named_instances = Vec::new();
    for (name, &(owner, index)) in placement_names.into_iter().zip(&component_placement_locations) {
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            let placed = &group_runs[owner.unwrap_or(root_run_index)].placed[index];
            named_instances.push(skpwright::Instance {
                pid: placed.pid,
                definition: None,
                defref: placed.defref,
                offset: 0,
                translation_m: [0.0; 3],
                transform: placed.transform,
                material: Some(placed.material),
                name: Some(name),
                is_group: Some(placed.is_group),
                hidden: Some(placed.hidden),
                layer: Some(placed.layer),
                slot: None,
                guid: placed.guid.clone(),
                casts_shadows: Some(placed.casts_shadows),
                receives_shadows: Some(placed.receives_shadows),
                locked: Some(placed.locked),
            });
        }
    }
    let mut model = Model {
        instances: named_instances,
        geo_located: input.source_geo_located,
        model_guid: input.source_model_guid.clone(),
        current_material: input.source_current_material,
        active_layer: restored_active_layer,
        dictionaries: restored_dictionaries,
        classifications: restored_classifications,
        guides: native_guides,
        texts: native_texts,
        dimensions: native_dimensions,
        radial_dimensions: native_radial_dimensions,
        definitions,
        definition_links,
        geometry: group_runs,
        layers,
        layer_links,
        materials,
        material_links,
        fonts: vec![skpwright::Font {
            family: "Tahoma".to_string(),
            bold: false,
            italic: false,
            size_pt: 12,
            model_height: false,
            height_m: 0.254,
        }],
        ..Default::default()
    };

    // Restore document-level SketchUp shadow and geolocation settings.
    if let Some(value) = &input.source_shadows {
        model.shadows = Some(skpwright::ShadowInfo {
                time: serde_json::from_value::<u32>(value.get("time").ok_or("Missing shadow field time")?.clone())?,
                city: serde_json::from_value::<String>(value.get("city").ok_or("Missing shadow field city")?.clone())?,
                country: serde_json::from_value::<String>(value.get("country").ok_or("Missing shadow field country")?.clone())?,
                longitude: serde_json::from_value::<f64>(value.get("longitude").ok_or("Missing shadow field longitude")?.clone())?,
                latitude: serde_json::from_value::<f64>(value.get("latitude").ok_or("Missing shadow field latitude")?.clone())?,
                tz_offset_h: serde_json::from_value::<f64>(value.get("tz_offset_h").ok_or("Missing shadow field tz_offset_h")?.clone())?,
                displayed: serde_json::from_value::<bool>(value.get("displayed").ok_or("Missing shadow field displayed")?.clone())?,
                on_faces: serde_json::from_value::<bool>(value.get("on_faces").ok_or("Missing shadow field on_faces")?.clone())?,
                on_ground: serde_json::from_value::<bool>(value.get("on_ground").ok_or("Missing shadow field on_ground")?.clone())?,
                from_edges: serde_json::from_value::<bool>(value.get("from_edges").ok_or("Missing shadow field from_edges")?.clone())?,
                light: serde_json::from_value::<u32>(value.get("light").ok_or("Missing shadow field light")?.clone())?,
                dark: serde_json::from_value::<u32>(value.get("dark").ok_or("Missing shadow field dark")?.clone())?,
                use_sun_for_shading: serde_json::from_value::<bool>(value.get("use_sun_for_shading").ok_or("Missing shadow field use_sun_for_shading")?.clone())?,
                daylight_saving: serde_json::from_value::<bool>(value.get("daylight_saving").ok_or("Missing shadow field daylight_saving")?.clone())?,
                north: serde_json::from_value::<[f64; 3]>(value.get("north").ok_or("Missing shadow field north")?.clone())?,
                north_displayed: serde_json::from_value::<bool>(value.get("north_displayed").ok_or("Missing shadow field north_displayed")?.clone())?,
            });
    }

    // BlackFox 0.9.7: preserve SketchUp PBR material properties.
    let mut seen_pbr = std::collections::HashSet::new();
    for source in &input.source_pbr {
        if source.material >= model.materials.len() {
            return Err(format!(
                "PBR references missing material {}", source.material
            ).into());
        }
        if !seen_pbr.insert(source.material) {
            return Err(format!(
                "Duplicate PBR material {}", source.material
            ).into());
        }
        model.features.material_pbr.push((
            source.material,
            blackfox_pbr_material(source)?,
        ));
    }
    println!(
        "NATIVE PBR: {} material records",
        model.features.material_pbr.len()
    );

    // Preserve native document dimension preferences.
    if let Some(d) = input.source_dimension_defaults {
        if d.arrow > 4 {
            return Err("Unsupported native dimension arrow style".into());
        }

        model.dimension_defaults = Some(skpwright::DimensionDefaults {
            font: d.font,
            aligned: d.aligned,
            arrow: d.arrow,
            text_position: d.text_position,
            color: d.color,
            highlight_non_associated: d.highlight_non_associated,
            radius_prefix: d.radius_prefix,
            hide_foreshortened: d.hide_foreshortened,
            hide_small: d.hide_small,
        });
    }
    // Restore original SketchUp scene animation.
    if model.geo_located {
        let geo = model.shadows.as_ref().ok_or("Missing geolocation coordinates")?;
        let attributes = [
            ("Latitude", skpwright::AttrValue::F64(geo.latitude)),
            ("Longitude", skpwright::AttrValue::F64(geo.longitude)),
            ("GeoReferenceNorthAngle", skpwright::AttrValue::F64(geo.north[0].atan2(geo.north[1]))),
            ("UsesGeoReferencing", skpwright::AttrValue::Bool(true)),
        ];
        model.attributes.extend(attributes.into_iter().map(|(key, value)| {
            skpwright::Attribute { key: key.into(), value }
        }));
    }

    if let Some(animation) = input.source_animation {
        if !animation.transition_s.is_finite()
            || !animation.delay_s.is_finite()
            || animation.transition_s < 0.0
            || animation.delay_s < 0.0
        {
            return Err("Invalid native scene animation timing".into());
        }

        model.animation.transitions = animation.transitions;
        model.animation.transition_s = animation.transition_s;
        model.animation.delay_s = animation.delay_s;
        model.animation.loop_slideshow = animation.loop_slideshow;
    }
    // Native SketchUp unit preferences are independent of geometry.
    // BlackFox geometry coordinates remain metres.
    if let Some(u) = requested_units {
        if u.length_unit > 4 {
            return Err("Unsupported SketchUp length unit".into());
        }
        if u.length_format > 3 {
            return Err("Unsupported SketchUp length format".into());
        }
        if !u.length_snap_length.is_finite() || u.length_snap_length < 0.0
            || !u.snap_angle.is_finite() {
            return Err("Invalid native unit settings".into());
        }

        model.units.length_format = u.length_format;
        model.units.length_unit = u.length_unit;
        model.units.length_precision = u.length_precision;
        model.units.angle_precision = u.angle_precision;
        model.units.length_snap = u.length_snap;
        model.units.length_snap_length = u.length_snap_length;
        model.units.angle_snap = u.angle_snap;
        model.units.snap_angle = u.snap_angle;
        model.units.suppress_units_display = u.suppress_units_display;
        model.units.force_inch_display = u.force_inch_display;
        model.units.area_unit = u.area_unit;
        model.units.volume_unit = u.volume_unit;
        model.units.area_precision = u.area_precision;
        model.units.volume_precision = u.volume_precision;
    } else {
        // Backward-compatible default for existing metre-based inputs.
        model.units.length_format = 0;
        model.units.length_unit = 4;
        model.units.length_precision = 3;
        model.units.length_snap = false;
    }
    // Preserve supported source rendering; otherwise use a textured fallback.
    let mut rendering = RenderingOptions::sketchup_default();
    rendering.face_style = 6;
    rendering.textures = true;
    if let Some(source) = &input.source_rendering {
        if let Some(v) = source.section_display { rendering.section_display = v; }
        if let Some(v) = source.section_fill { rendering.section_fill = Some(v); }
        if let Some(v) = source.section_fill_color { rendering.section_fill_color = Some(v); }
        if let Some(v) = source.face_style { rendering.face_style = v; }
        if let Some(v) = source.textures { rendering.textures = v; }
        if let Some(v) = source.edges { rendering.edges = v; }
        if let Some(v) = source.profiles { rendering.profiles = v; }
        if let Some(v) = source.profiles_width { rendering.profiles_width = v; }
        if let Some(v) = source.background { rendering.background = v; }
        if let Some(v) = source.sky { rendering.sky = v; }
        if let Some(v) = source.sky_color { rendering.sky_color = v; }
        if let Some(v) = source.ground { rendering.ground = v; }
        if let Some(v) = source.ground_color { rendering.ground_color = v; }
        if let Some(v) = source.front_color { rendering.front_color = v; }
        if let Some(v) = source.back_color { rendering.back_color = v; }
        if let Some(v) = source.ambient_occlusion { rendering.ambient_occlusion = Some(v); }
    }
    let style_name = input.source_style_name.as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("BlackFox Default");
    model.styles = vec![Style {
        name: style_name.into(),
        description: input.source_style_description.clone().unwrap_or_default(),
        guid: String::new(),
        settings: rendering.clone(),
        watermarks: Vec::new(),
        strokes: None,
        items: Vec::new(),
    }];
    // BLACKFOX_PHASE_2P_ATTACH_WATERMARKS
    let mut seen_style_indices = std::collections::HashSet::new();

    for group in &input.source_style_watermarks {
        if !seen_style_indices.insert(group.style_index) {
            return Err(format!(
                "Duplicate watermark style index: {}",
                group.style_index
            ).into());
        }

        if group.style_index != 0 {
            return Err(format!(
                "Unsupported watermark style index {}: exporter currently supports only style 0",
                group.style_index
            ).into());
        }

        let mut restored = Vec::with_capacity(group.watermarks.len());

        for watermark in &group.watermarks {
            restored.push(blackfox_native_watermark(watermark)?);
        }

        model.styles[0].watermarks = restored;
    }

    let mut restored_current_watermarks =
        Vec::with_capacity(input.source_current_watermarks.len());

    for watermark in &input.source_current_watermarks {
        restored_current_watermarks.push(
            blackfox_native_watermark(watermark)?
        );
    }

    model.watermarks = restored_current_watermarks;
    // BLACKFOX_PHASE_2P_NATIVE_WATERMARK_SECTION_VALUE
    model.native_watermark_section_value = input.source_native_watermark_section_value;
    model.active_style = Some(0);
    model.rendering = Some(rendering);
    // Mission 124: native current camera and saved scenes.
    if let Some(c) = &input.camera {
        model.camera = Some(blackfox_camera(c)?);
    }

    let mut scene_names = std::collections::HashSet::new();
    let mut native_scenes = Vec::new();

    for scene in &input.scenes {
        if scene.name.trim().is_empty() {
            return Err("Scene name cannot be empty".into());
        }

        if !scene_names.insert(scene.name.as_str()) {
            return Err(format!("Duplicate scene name: {}", scene.name).into());
        }

        let mut native = Scene {
            name: scene.name.clone(),
            description: scene.description.clone(),
            saved: SceneProperties(SceneProperties::CAMERA),
            camera: Some(blackfox_camera(&scene.camera)?),
            ..Default::default()
        };


        // C1: Preserve original scene thumbnail bytes.
        if let Some(path) = &scene.thumbnail_path {
            let bytes = fs::read(path)?;

            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                return Err(format!(
                    "Scene '{}' thumbnail is not a PNG",
                    scene.name
                ).into());
            }

            native.thumbnail = Some(bytes);
        }

        // B3: Restore native per-scene appearance and saved flags.
        if let Some(saved) = scene.saved {
            native.saved = SceneProperties(saved);
        }

        if let Some(value) = &scene.rendering {
            native.saved.0 |= SceneProperties::STYLE_AND_FOG;
            let mut restored = RenderingOptions::sketchup_default();
            let object = value.as_object().ok_or(
                "Scene rendering must be an object"
            )?;
            if let Some(v) = object.get("face_style") {
                if !v.is_null() {
                    restored.face_style = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("xray") {
                if !v.is_null() {
                    restored.xray = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("material_transparency") {
                if !v.is_null() {
                    restored.material_transparency = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("textures") {
                if !v.is_null() {
                    restored.textures = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("edges") {
                if !v.is_null() {
                    restored.edges = v.as_bool().ok_or(
                        "Scene edges must be boolean"
                    )?;
                }
            }
            if let Some(v) = object.get("back_edges") {
                if !v.is_null() {
                    restored.back_edges = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("edges_mode") {
                if !v.is_null() {
                    restored.edges_mode = serde_json::from_value::<Option<u32>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("edge_type") {
                if !v.is_null() {
                    restored.edge_type = serde_json::from_value::<Option<u32>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("display_text") {
                if !v.is_null() {
                    restored.display_text = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("display_dimensions") {
                if !v.is_null() {
                    restored.display_dimensions = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("profiles_only") {
                if !v.is_null() {
                    restored.profiles_only = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("edge_color_mode") {
                if !v.is_null() {
                    restored.edge_color_mode = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("edge_color") {
                if !v.is_null() {
                    restored.edge_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("profiles") {
                if !v.is_null() {
                    restored.profiles = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("profiles_width") {
                if !v.is_null() {
                    restored.profiles_width = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("depth_cue") {
                if !v.is_null() {
                    restored.depth_cue = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("depth_cue_width") {
                if !v.is_null() {
                    restored.depth_cue_width = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("extension") {
                if !v.is_null() {
                    restored.extension = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("extension_length") {
                if !v.is_null() {
                    restored.extension_length = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("endpoints") {
                if !v.is_null() {
                    restored.endpoints = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("endpoints_length") {
                if !v.is_null() {
                    restored.endpoints_length = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("jitter") {
                if !v.is_null() {
                    restored.jitter = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("dashes") {
                if !v.is_null() {
                    restored.dashes = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("front_color") {
                if !v.is_null() {
                    restored.front_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("back_color") {
                if !v.is_null() {
                    restored.back_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("transparency_quality") {
                if !v.is_null() {
                    restored.transparency_quality = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("xray_opacity") {
                if !v.is_null() {
                    restored.xray_opacity = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("background") {
                if !v.is_null() {
                    restored.background = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("sky") {
                if !v.is_null() {
                    restored.sky = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("sky_color") {
                if !v.is_null() {
                    restored.sky_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ground") {
                if !v.is_null() {
                    restored.ground = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ground_color") {
                if !v.is_null() {
                    restored.ground_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ground_transparency") {
                if !v.is_null() {
                    restored.ground_transparency = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ground_from_below") {
                if !v.is_null() {
                    restored.ground_from_below = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("watermarks") {
                if !v.is_null() {
                    restored.watermarks = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("selected_color") {
                if !v.is_null() {
                    restored.selected_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("locked_color") {
                if !v.is_null() {
                    restored.locked_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("guides_color") {
                if !v.is_null() {
                    restored.guides_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("guides_hidden") {
                if !v.is_null() {
                    restored.guides_hidden = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("model_axes") {
                if !v.is_null() {
                    restored.model_axes = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("color_by_layer") {
                if !v.is_null() {
                    restored.color_by_layer = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("hidden_geometry") {
                if !v.is_null() {
                    restored.hidden_geometry = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("hidden_objects") {
                if !v.is_null() {
                    restored.hidden_objects = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("active_section_color") {
                if !v.is_null() {
                    restored.active_section_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("inactive_section_color") {
                if !v.is_null() {
                    restored.inactive_section_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("section_cut_color") {
                if !v.is_null() {
                    restored.section_cut_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("section_cut_width") {
                if !v.is_null() {
                    restored.section_cut_width = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("section_display") {
                if !v.is_null() {
                    restored.section_display = serde_json::from_value::<u32>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("section_fill") {
                if !v.is_null() {
                    restored.section_fill = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("section_fill_color") {
                if !v.is_null() {
                    restored.section_fill_color = serde_json::from_value::<Option<[u8; 4]>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("horizon_color") {
                if !v.is_null() {
                    restored.horizon_color = serde_json::from_value::<Option<[u8; 4]>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("default_section_fill_style") {
                if !v.is_null() {
                    restored.default_section_fill_style = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ambient_occlusion") {
                if !v.is_null() {
                    restored.ambient_occlusion = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ambient_occlusion_distance") {
                if !v.is_null() {
                    restored.ambient_occlusion_distance = serde_json::from_value::<Option<f32>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ambient_occlusion_intensity") {
                if !v.is_null() {
                    restored.ambient_occlusion_intensity = serde_json::from_value::<Option<f32>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ambient_occlusion_color_enabled") {
                if !v.is_null() {
                    restored.ambient_occlusion_color_enabled = serde_json::from_value::<Option<bool>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ambient_occlusion_color") {
                if !v.is_null() {
                    restored.ambient_occlusion_color = serde_json::from_value::<Option<[u8; 4]>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("ambient_occlusion_multiplier") {
                if !v.is_null() {
                    restored.ambient_occlusion_multiplier = serde_json::from_value::<Option<f32>>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fog") {
                if !v.is_null() {
                    restored.fog = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fog_color") {
                if !v.is_null() {
                    restored.fog_color = serde_json::from_value::<[u8; 4]>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fog_uses_background") {
                if !v.is_null() {
                    restored.fog_uses_background = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fog_start_m") {
                if !v.is_null() {
                    restored.fog_start_m = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fog_end_m") {
                if !v.is_null() {
                    restored.fog_end_m = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("hide_rest_of_model") {
                if !v.is_null() {
                    restored.hide_rest_of_model = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("hide_similar_components") {
                if !v.is_null() {
                    restored.hide_similar_components = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("component_axes") {
                if !v.is_null() {
                    restored.component_axes = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fade_rest_of_model") {
                if !v.is_null() {
                    restored.fade_rest_of_model = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("fade_similar_components") {
                if !v.is_null() {
                    restored.fade_similar_components = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("background_photo") {
                if !v.is_null() {
                    restored.background_photo = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("background_photo_opacity") {
                if !v.is_null() {
                    restored.background_photo_opacity = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("foreground_photo") {
                if !v.is_null() {
                    restored.foreground_photo = serde_json::from_value::<bool>(
                        v.clone()
                    )?;
                }
            }
            if let Some(v) = object.get("foreground_photo_opacity") {
                if !v.is_null() {
                    restored.foreground_photo_opacity = serde_json::from_value::<f64>(
                        v.clone()
                    )?;
                }
            }
            native.rendering = Some(restored);
        }

        if let Some(value) = &scene.shadows {
            native.saved.0 |= SceneProperties::SHADOWS;
            native.shadows = Some(skpwright::ShadowInfo {
                time: serde_json::from_value::<u32>(value.get("time").ok_or("Missing shadow field time")?.clone())?,
                city: serde_json::from_value::<String>(value.get("city").ok_or("Missing shadow field city")?.clone())?,
                country: serde_json::from_value::<String>(value.get("country").ok_or("Missing shadow field country")?.clone())?,
                longitude: serde_json::from_value::<f64>(value.get("longitude").ok_or("Missing shadow field longitude")?.clone())?,
                latitude: serde_json::from_value::<f64>(value.get("latitude").ok_or("Missing shadow field latitude")?.clone())?,
                tz_offset_h: serde_json::from_value::<f64>(value.get("tz_offset_h").ok_or("Missing shadow field tz_offset_h")?.clone())?,
                displayed: serde_json::from_value::<bool>(value.get("displayed").ok_or("Missing shadow field displayed")?.clone())?,
                on_faces: serde_json::from_value::<bool>(value.get("on_faces").ok_or("Missing shadow field on_faces")?.clone())?,
                on_ground: serde_json::from_value::<bool>(value.get("on_ground").ok_or("Missing shadow field on_ground")?.clone())?,
                from_edges: serde_json::from_value::<bool>(value.get("from_edges").ok_or("Missing shadow field from_edges")?.clone())?,
                light: serde_json::from_value::<u32>(value.get("light").ok_or("Missing shadow field light")?.clone())?,
                dark: serde_json::from_value::<u32>(value.get("dark").ok_or("Missing shadow field dark")?.clone())?,
                use_sun_for_shading: serde_json::from_value::<bool>(value.get("use_sun_for_shading").ok_or("Missing shadow field use_sun_for_shading")?.clone())?,
                daylight_saving: serde_json::from_value::<bool>(value.get("daylight_saving").ok_or("Missing shadow field daylight_saving")?.clone())?,
                north: serde_json::from_value::<[f64; 3]>(value.get("north").ok_or("Missing shadow field north")?.clone())?,
                north_displayed: serde_json::from_value::<bool>(value.get("north_displayed").ok_or("Missing shadow field north_displayed")?.clone())?,
            });
        }

        if let Some(value) = &scene.axes {
            native.axes = Some(skpwright::Axes {
                origin_m: serde_json::from_value::<[f64; 3]>(value.get("origin_m").ok_or("Missing axes field origin_m")?.clone())?,
                x: serde_json::from_value::<[f64; 3]>(value.get("x").ok_or("Missing axes field x")?.clone())?,
                y: serde_json::from_value::<[f64; 3]>(value.get("y").ok_or("Missing axes field y")?.clone())?,
                z: serde_json::from_value::<[f64; 3]>(value.get("z").ok_or("Missing axes field z")?.clone())?,
            });
        }

        if let Some(style) = &scene.style {
            if !model.styles.iter().any(|s| &s.name == style) {
                return Err(format!(
                    "Scene '{}' references unknown style '{}'",
                    scene.name, style
                ).into());
            }
            native.style = Some(style.clone());
        }

        if let Some(names) = &scene.hidden_layers {
            native.saved.0 |= SceneProperties::VISIBLE_LAYERS;
            for name in names {
                let index = model.layers.iter().position(|layer| &layer.name == name)
                    .ok_or_else(|| format!("Scene '{}' references unknown layer '{}'", scene.name, name))?;
                if !native.hidden_layers.contains(&index) {
                    native.hidden_layers.push(index);
                }
            }
        }

        if let Some(active_sections) = &scene.active_section_planes {
            native.saved.0 |= SceneProperties::ACTIVE_SECTION_PLANES;

            for id in active_sections {
                let index = *section_ids.get(id.as_str()).ok_or_else(|| {
                    format!(
                        "Scene '{}' references unknown section plane '{}'",
                        scene.name, id
                    )
                })?;

                let pid = model.geometry[root_index_for_sections]
                    .sections[index].pid;

                native.active_section_planes.push(pid);
            }
        }

        // No fabricated per-scene styles: retain camera and section settings.
        native_scenes.push(native);
    }

    let selected = match input.selected_scene.as_deref() {
        None => None,
        Some(name) => Some(
            input.scenes.iter()
                .position(|s| s.name == name)
                .ok_or_else(|| format!("Unknown selected scene: {}", name))?
        ),
    };

    model.scenes = native_scenes;
    model.selected_scene = selected;

    let modern_bytes = || skpwright::Document::from_model(&model)?.write();
    let output_bytes = if let Some(version) = version {
        let number: u32 = version.parse().map_err(|_| "Invalid SKP version")?;
        if !matches!(number, 3..=8 | 2013..=2027) {
            return Err("Unsupported BlackFox SKP version".into());
        }

        if (2021..=2027).contains(&number) {
            if number != 2026 {
                eprintln!("COMPATIBILITY NOTE: SKP {} uses the shared revision-17 container; compatibility with that year's SketchUp application is not independently verified.", number);
            }
            modern_bytes()?
        } else {
            let release = skpwright::Release::of_name(number).ok_or("Unsupported SKP release")?;
            let (bytes, notes) = model.to_release(release)?;
            for note in notes {
                eprintln!("COMPATIBILITY NOTE: {}", note);
            }
            bytes
        }
    } else {
        modern_bytes()?
    };

    // Validate and encode first, then reserve the final name without clobbering
    // a file created while either exporter was preparing its data.
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(output)?;
    if let Err(error) = std::io::Write::write_all(&mut file, &output_bytes) {
        drop(file);
        if let Err(cleanup) = fs::remove_file(output) {
            return Err(format!("Failed writing SKP: {error}; cleanup failed: {cleanup}").into());
        }
        return Err(error.into());
    }

    println!("SUCCESS: {}", output.display());
    Ok(())
}
pub fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    // CLI arguments provided by unified dispatcher.

    if args.len() != 3 && args.len() != 4 {
        eprintln!("Usage: BlackFoxSKP export input.json output.skp [skp_version]");
        std::process::exit(2);
    }

    println!("BlackFox SKP Exporter 1.0.0");

    let input_path = Path::new(&args[1]);
    let output_path = Path::new(&args[2]);

    if input_path == output_path {
        return Err("Input and output paths must differ".into());
    }

    if output_path.exists() {
        return Err("Output already exists; refusing to overwrite".into());
    }

    let json = fs::read_to_string(input_path)?;
    let json = json.trim_start_matches('\u{feff}');
    let input: InputModel = serde_json::from_str(json)?;

    convert(input, output_path, args.get(3).map(String::as_str))
}


