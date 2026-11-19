//! The side files of a 2026 archive that the authoring path writes
//! (`docs/WRITING.md`): `materials/<folder>/material.xml`,
//! `styles/<name>/style.xml`, the two `meta/` thumbnail PNGs and
//! `meta/meta.dat`. Every element, attribute and constant below is the
//! one the corpus files carry (SKP_FORMAT §16.6, §16.14, §16.1).

use crate::features::PbrMaterial;
use crate::settings::{RenderingOptions, RoKind, RoValue, Watermark, RO_FIELDS};
use crate::tree26::Record;

/// XML attribute/text escaping.
pub(crate) fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 has no way to carry other control characters.
            c if c.is_control() && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// C's `%.17g`, which is how the style document prints its decimals
/// (`0.65000000000000002`, `1`, `0.80000000000000004`).
pub(crate) fn g17(v: f64) -> String {
    g_digits(v, 17)
}

/// C's `%.<n>g`.
pub(crate) fn g_digits(v: f64, n: i32) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf" } else { "inf" }.into();
    }
    // `n` significant digits in scientific form, then choose %g's style.
    let sci = format!("{:.*e}", n as usize - 1, v); // d.ddde±x
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if !(-4..n).contains(&exp) {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push_str(&format!(
            "e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        ));
        return out;
    }
    if exp < 0 {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.push_str(digits);
    } else {
        let int_len = exp as usize + 1;
        if digits.len() <= int_len {
            out.push_str(digits);
            for _ in digits.len()..int_len {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..int_len]);
            out.push('.');
            out.push_str(&digits[int_len..]);
        }
    }
    out
}

/// The `<mat:pbrMR>` block (§16.6): the constant block of a plain
/// material, or the one `p` describes. `x` and `y` are the material's own
/// scale, which `p` may override.
fn pbr(x: &str, y: &str, p: Option<&PbrMaterial>) -> String {
    let d = PbrMaterial::default();
    let p = p.unwrap_or(&d);
    let (x, y) = match p.scale {
        Some([sx, sy]) => (g17(sx), g17(sy)),
        None => (x.to_string(), y.to_string()),
    };
    let f32t = |v: f32| g_digits(v as f64, 8);
    let mut out = format!("    <mat:pbrMR xScale=\"{x}\" yScale=\"{y}\"");
    for (k, v) in &p.other_pbr_attrs {
        out.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    out.push_str(">\n");
    let mut line = |s: String| {
        out.push_str("      ");
        out.push_str(&s);
        out.push('\n');
    };
    line(format!(
        "<mat:enable_metalness>{}</mat:enable_metalness>",
        p.enable_metalness as u8
    ));
    line(format!(
        "<mat:enable_roughness>{}</mat:enable_roughness>",
        p.enable_roughness as u8
    ));
    line(format!(
        "<mat:roughness_texture_invert>{}</mat:roughness_texture_invert>",
        p.roughness_texture_invert as u8
    ));
    line(format!(
        "<mat:enable_normal>{}</mat:enable_normal>",
        p.enable_normal as u8
    ));
    line(format!(
        "<mat:enable_occlusion>{}</mat:enable_occlusion>",
        p.enable_occlusion as u8
    ));
    for (name, t) in [
        ("metallic_texture", &p.metallic),
        ("roughness_texture", &p.roughness),
        ("normal_texture", &p.normal),
        ("occlusion_texture", &p.occlusion),
    ] {
        if let Some(t) = t {
            let mut e = format!(
                "<mat:{name} id=\"{}\" path=\"{}\" file_name=\"{}\"",
                t.id,
                esc(&t.path),
                esc(&t.file_name)
            );
            for (k, v) in &t.other_attrs {
                e.push_str(&format!(" {k}=\"{}\"", esc(v)));
            }
            e.push_str("/>");
            line(e);
        }
    }
    line(format!(
        "<mat:metallicFactor>{}</mat:metallicFactor>",
        f32t(p.metallic_factor)
    ));
    line(format!(
        "<mat:roughnessFactor>{}</mat:roughnessFactor>",
        f32t(p.roughness_factor)
    ));
    line(format!(
        "<mat:normalMapStyle>{}</mat:normalMapStyle>",
        p.normal_map_style
    ));
    line(format!(
        "<mat:normalScale>{}</mat:normalScale>",
        f32t(p.normal_scale)
    ));
    line(format!(
        "<mat:occlusionStrength>{}</mat:occlusionStrength>",
        f32t(p.occlusion_strength)
    ));
    line(format!(
        "<mat:baseColorFactor>{}</mat:baseColorFactor>",
        p.base_color_factor
    ));
    for e in &p.other_elements {
        line(e.clone());
    }
    out.push_str("    </mat:pbrMR>\n");
    out
}

/// The `workflow`, `pbrPromoState` and trailing attributes of
/// `mat:material` for `p` (the plain values without it), and the `trans`
/// text to use when the material is opaque.
fn material_extras(p: Option<&PbrMaterial>) -> (u32, u32, String) {
    match p {
        Some(p) => (
            p.workflow,
            p.promo_state,
            p.other_material_attrs
                .iter()
                .map(|(k, v)| format!(" {k}=\"{}\"", esc(v)))
                .collect(),
        ),
        None => (0, 0, String::new()),
    }
}

const MATERIAL_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n\
<materialDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/material\" xmlns:mat=\"http://sketchup.google.com/schemas/sketchup/1.0/material\">\n";

/// The `trans`/`useTrans` pair for an opacity (§16.6). `trans` is the
/// transparency, one minus the opacity. Opacity 1 writes `trans="1"
/// useTrans="0"`, as every opaque corpus material does; anything else
/// writes the transparency with `useTrans="1"`.
pub(crate) fn trans_attrs(opacity: f64) -> (String, u8) {
    if opacity >= 1.0 {
        ("1".to_string(), 0)
    } else {
        (g17(1.0 - opacity.max(0.0)), 1)
    }
}

/// A solid material's document (§16.6), its opacity written as
/// [`trans_attrs`] does (`pbrPromoState` stays 0, as the converted files
/// have it).
pub(crate) fn solid_material_xml(
    name: &str,
    rgb: [u8; 3],
    opacity: f64,
    colorize_type: u32,
    pbr: Option<&PbrMaterial>,
) -> String {
    let (trans, use_trans) = trans_attrs_for(opacity, pbr);
    solid_xml(name, rgb, &trans, use_trans, colorize_type, pbr)
}

/// [`trans_attrs`], an opaque material taking the unused `trans` its
/// physically based properties carry.
fn trans_attrs_for(opacity: f64, pbr: Option<&PbrMaterial>) -> (String, u8) {
    match pbr.and_then(|p| p.trans_unused) {
        Some(t) if opacity >= 1.0 => (g17(t), 0),
        _ => trans_attrs(opacity),
    }
}

/// A layer colour's material document: opaque, written `trans="0"
/// useTrans="0"`.
pub(crate) fn layer_material_xml(name: &str, rgb: [u8; 3]) -> String {
    solid_xml(name, rgb, "0", 0, 0, None)
}

fn solid_xml(
    name: &str,
    rgb: [u8; 3],
    trans: &str,
    use_trans: u8,
    colorize_type: u32,
    p: Option<&PbrMaterial>,
) -> String {
    let (workflow, promo, extra) = material_extras(p);
    format!(
        "{MATERIAL_HEAD}  <mat:material name=\"{}\" type=\"0\" workflow=\"{workflow}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" colorizeType=\"{colorize_type}\" trans=\"{trans}\" useTrans=\"{use_trans}\" pbrPromoState=\"{promo}\" hasTexture=\"0\"{extra}>\n{}  </mat:material>\n</materialDocument>\n",
        esc(name),
        rgb[0],
        rgb[1],
        rgb[2],
        pbr("1", "1", p)
    )
}

/// A textured material's document. `texture_name` is the texture's name
/// as the model carries it (a stream-container file keeps the path the image was
/// loaded from), `file_name` the archive file name; `image_path` is the archive path of
/// the image, written folder-relative (`./<file>`) when it lives in this
/// material's folder and archive-rooted otherwise (a shared image).
/// `avg_rgba` is the texture's average colour; the material colour
/// attributes carry its RGB.
#[allow(clippy::too_many_arguments)]
pub(crate) fn textured_material_xml(
    name: &str,
    folder: &str,
    texture_name: &str,
    file_name: &str,
    image_path: &str,
    rgb: [u8; 3],
    avg_rgba: [u8; 4],
    colorized: bool,
    size_in: (f64, f64),
    opacity: f64,
    colorize_type: u32,
    p: Option<&PbrMaterial>,
) -> String {
    let kind = if colorized { 2 } else { 1 };
    let (trans, use_trans) = trans_attrs_for(opacity, p);
    let (workflow, promo, extra) = material_extras(p);
    let own = format!("materials/{folder}/");
    let path = match image_path.strip_prefix(&own) {
        Some(rest) => format!("./{rest}"),
        None => image_path.to_string(),
    };
    let (x, y) = (g17(size_in.0), g17(size_in.1));
    format!(
        "{MATERIAL_HEAD}  <mat:material name=\"{}\" type=\"{kind}\" workflow=\"{workflow}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" colorizeType=\"{colorize_type}\" trans=\"{trans}\" useTrans=\"{use_trans}\" pbrPromoState=\"{promo}\" hasTexture=\"1\"{extra}>\n\
         \x20   <mat:texture textureFilename=\"{}\" xScale=\"{x}\" yScale=\"{y}\" avgColor=\"{}\">\n\
         \x20     <mat:images>\n\
         \x20       <mat:image id=\"1\" path=\"{}\" file_name=\"{}\"/>\n\
         \x20     </mat:images>\n\
         \x20   </mat:texture>\n{}  </mat:material>\n</materialDocument>\n",
        esc(name),
        rgb[0],
        rgb[1],
        rgb[2],
        esc(texture_name),
        u32::from_le_bytes(avg_rgba),
        esc(&path),
        esc(file_name),
        pbr(&x, &y, p)
    )
}

/// The items a 2026 style document holds beyond the rendering-option
/// table, at the values every observed file gives them: (id, variant
/// type, text). Items 4007 and 8100 to 8107 are rendering-option fields
/// too (horizon colour, ambient occlusion); the entries here are the
/// values a style written without those settings takes.
pub(crate) const STYLE_CONSTANTS: [(u32, u32, &str); 8] = [
    (1016, 1, "1"),
    (4007, 4, "0"),
    (8100, 1, "0"),
    (8102, 6, "10"),
    (8103, 6, "0"),
    (8105, 1, "0"),
    (8106, 4, "-16777216"),
    (8107, 6, "1"),
];

/// A style document (§16.14, §16.10): one `<sty:item>` per setting in
/// ascending id, the watermark list as item 5001, and the items the 2026
/// document adds beyond the rendering-option table at their constant
/// values.
pub(crate) fn style_xml(
    name: &str,
    desc: &str,
    ro: &RenderingOptions,
    watermarks: &[(Watermark, String)],
    extra: &[crate::StyleItem],
    strokes: Option<&crate::Strokes>,
) -> String {
    // (id, variant type, text)
    let mut items: Vec<(u32, u32, String)> = Vec::new();
    for (field, value) in RO_FIELDS.iter().zip(ro.values()) {
        let (Some(id), Some(v)) = (field.item, value.1) else {
            continue;
        };
        // The variant type follows the field's kind (a bool setting in a
        // u32 record is item type 4), as the reader maps it back.
        let as_u32 = |v: RoValue| match v {
            RoValue::Bool(b) => b as u32,
            RoValue::U32(x) => x,
            RoValue::Rgba(c) => u32::from_le_bytes(c),
            RoValue::F64(f) => f as u32,
            RoValue::F32(f) => f as u32,
        };
        let (ty, text) = match field.kind {
            RoKind::Bool => (1, ((as_u32(v) != 0) as u8).to_string()),
            RoKind::U32 | RoKind::Rgba => (4, (as_u32(v) as i32).to_string()),
            RoKind::F64 => (
                7,
                match v {
                    RoValue::F64(f) => g17(f),
                    other => g17(as_u32(other) as f64),
                },
            ),
            // A float item prints eight significant digits (`%.8g`).
            RoKind::F32 => (
                6,
                match v {
                    RoValue::F32(f) => g_digits(f as f64, 8),
                    other => g_digits(as_u32(other) as f64, 8),
                },
            ),
        };
        let text = match (id, ro.edges_mode.filter(|_| ro.edges)) {
            (1000, Some(m)) => (m as i32).to_string(),
            _ => text,
        };
        items.push((id, ty, text));
    }
    // The items the style keeps as they are, then those with no
    // rendering-option field at the values every file holds.
    let mut have: std::collections::HashSet<u32> = items.iter().map(|i| i.0).collect();
    for x in extra {
        if !have.insert(x.id) {
            continue;
        }
        let (ty, text) = match &x.value {
            crate::StyleValue::Bool(b) => (1, (*b as u8).to_string()),
            crate::StyleValue::Int(i) => (4, i.to_string()),
            crate::StyleValue::Colour(c) => (4, (u32::from_le_bytes(*c) as i32).to_string()),
            crate::StyleValue::Double(f) => (7, g17(*f)),
            crate::StyleValue::Other { kind, text } => (*kind, esc(text)),
        };
        items.push((x.id, ty, text));
    }
    for (id, ty, text) in STYLE_CONSTANTS {
        if have.insert(id) {
            items.push((id, ty, text.to_string()));
        }
    }
    items.sort_by_key(|i| i.0);
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
    out.push_str("<styleDocument xmlns=\"http://sketchup.google.com/schemas/sketchup/1.0/style\" xmlns:sty=\"http://sketchup.google.com/schemas/sketchup/1.0/style\">\n");
    out.push_str(&format!(
        "  <sty:style xmlns:t=\"http://sketchup.google.com/schemas/1.0/types\" name=\"{}\" desc=\"{}\">\n",
        esc(name),
        esc(desc)
    ));
    let mut wrote_5001 = false;
    let mut wrote_strokes = strokes.is_none();
    for (id, ty, text) in &items {
        if *id > crate::file17::ITEM_STROKES && !wrote_strokes {
            out.push_str(&strokes_item(strokes.unwrap()));
            wrote_strokes = true;
        }
        if *id > 5001 && !wrote_5001 {
            out.push_str(&watermark_item(watermarks));
            wrote_5001 = true;
        }
        out.push_str(&format!(
            "    <sty:item id=\"{id}\">\n      <t:variant type=\"{ty}\">{text}</t:variant>\n    </sty:item>\n"
        ));
    }
    if !wrote_strokes {
        out.push_str(&strokes_item(strokes.unwrap()));
    }
    if !wrote_5001 {
        out.push_str(&watermark_item(watermarks));
    }
    out.push_str("  </sty:style>\n</styleDocument>\n");
    out
}

/// The archive file name of a sketchy-edge style's `k`-th stroke image
/// (§16.14): `Stroke_<variant>_<length>.png`, the variants of the first
/// length first.
pub(crate) fn stroke_file(st: &crate::Strokes, k: usize) -> String {
    let variants = st.variants.max(1) as usize;
    let length = st.lengths.get(k / variants).copied().unwrap_or(0);
    format!("Stroke_{}_{length}.png", k % variants)
}

/// Item 1013: a sketchy-edge style's strokes, `<nprEdge>` with its
/// lengths and images (§16.14).
fn strokes_item(st: &crate::Strokes) -> String {
    let mut s = String::new();
    s.push_str("    <sty:item id=\"1013\">\n      <t:variant type=\"13\" xmlns:n0=\"http://sketchup.google.com/schemas/sketchup/1.0/npr\">\n");
    s.push_str(&format!(
        "        <n0:nprEdge dropoutLength=\"{}\" fadeFactor=\"{}\" width=\"{}\" useHalo=\"{}\" haloLength=\"{}\" numProtos=\"{}\">\n",
        g17(st.dropout_length),
        g17(st.fade_factor),
        st.width,
        st.halo as u8,
        st.halo_length,
        st.variants
    ));
    s.push_str("          <n0:lengths>\n");
    for l in &st.lengths {
        s.push_str(&format!("            <n0:length>{l}</n0:length>\n"));
    }
    s.push_str("          </n0:lengths>\n          <n0:images>\n");
    for k in 0..st.images.len() {
        let f = stroke_file(st, k);
        s.push_str(&format!(
            "            <n0:image id=\"{}\" path=\"./{f}\" file_name=\"{f}\"/>\n",
            k + 1
        ));
    }
    s.push_str(
        "          </n0:images>\n        </n0:nprEdge>\n      </t:variant>\n    </sty:item>\n",
    );
    s
}

/// Item 5001: the watermark list, backgrounds before the `<MODEL SPACE>`
/// separator and overlays after it. Each watermark comes with its archive
/// image path.
fn watermark_item(watermarks: &[(Watermark, String)]) -> String {
    let mut s = String::new();
    s.push_str("    <sty:item id=\"5001\">\n      <t:variant type=\"13\" xmlns:n0=\"http://sketchup.google.com/schemas/sketchup/1.0/wmlist\">\n");
    s.push_str(&format!(
        "        <n0:wmlist count=\"{}\">\n          <n0:screenimages>\n",
        watermarks.len() + 1
    ));
    let image = |w: &Watermark, path: &str, image_id: usize| {
        let file = w.source_file.rsplit(|c| c == '/' || c == '\\').next().unwrap_or(&w.source_file);
        let (fitting, stretch) = fitting_of(w);
        format!(
            "            <n0:screenimage name=\"{}\" info_found=\"0\" info_filename=\"{}\" info_time=\"{}\" position=\"{}\" tiled=\"{}\" stretched=\"{}\" maintainAR=\"{}\" fittingType=\"{fitting}\" stretchType=\"{stretch}\" background=\"{}\" intensForAlpha=\"{}\" alphaScale=\"{}\" scale=\"{}\">\n\
             \x20             <n0:images>\n\
             \x20               <n0:image id=\"{image_id}\" path=\"{}\" file_name=\"{}\"/>\n\
             \x20             </n0:images>\n\
             \x20           </n0:screenimage>\n",
            esc(&w.name),
            esc(&w.source_file),
            watermark_time(w),
            w.position,
            w.tiled as u8,
            w.stretched as u8,
            w.keep_aspect_ratio as u8,
            w.background as u8,
            w.mask as u8,
            g17(w.blend),
            g17(w.scale),
            esc(path),
            esc(file)
        )
    };
    let mut image_id = 0usize;
    for (w, p) in watermarks.iter().filter(|(w, _)| w.background) {
        image_id += 1;
        s.push_str(&image(w, p, image_id));
    }
    s.push_str("            <n0:screenimage name=\"&lt;MODEL SPACE&gt;\"/>\n");
    for (w, p) in watermarks.iter().filter(|(w, _)| !w.background) {
        image_id += 1;
        s.push_str(&image(w, p, image_id));
    }
    s.push_str(
        "          </n0:screenimages>\n        </n0:wmlist>\n      </t:variant>\n    </sty:item>\n",
    );
    s
}

/// The 2026 fitting type (0 tiled, 1 stretched, 2 positioned) and stretch
/// type (1 = stretched with its aspect ratio kept) of a watermark
/// (§16.14).
pub(crate) fn fitting_of(w: &Watermark) -> (u32, u32) {
    let fitting = if w.tiled {
        0
    } else if w.stretched {
        1
    } else {
        2
    };
    (fitting, w.keep_aspect_ratio as u32)
}

/// The file time a watermark record carries: the model's, or the write
/// time when the model has none.
pub(crate) fn watermark_time(w: &Watermark) -> u32 {
    if w.time != 0 {
        w.time
    } else {
        now_secs()
    }
}

pub(crate) fn now_secs() -> u32 {
    crate::env::now_secs()
}

// ---------------------------------------------------------------- PNG

/// A small opaque PNG (`w`×`h`, one flat RGB colour): a CRC-32 and a
/// zlib-wrapped DEFLATE stream, both already here. Stands in for the
/// thumbnails SketchUp renders.
pub(crate) fn flat_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut row = vec![0u8]; // filter: none
    for _ in 0..w {
        row.extend(rgb);
    }
    let mut raw = Vec::with_capacity(row.len() * h as usize);
    for _ in 0..h {
        raw.extend_from_slice(&row);
    }
    let deflate = crate::deflate::compress(&raw, 1);
    let mut idat = vec![0x78, 0x01];
    idat.extend(deflate);
    idat.extend(adler32(&raw).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]); // 8-bit RGB, no interlace
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [
        (b"IHDR", &ihdr[..]),
        (b"IDAT", &idat[..]),
        (b"IEND", &[][..]),
    ] {
        png.extend((body.len() as u32).to_be_bytes());
        let mut chunk = kind.to_vec();
        chunk.extend_from_slice(body);
        png.extend_from_slice(&chunk);
        png.extend(crate::zip::crc32(&chunk).to_be_bytes());
    }
    png
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// The format of an image, as its first bytes tell it: the type a 2017
/// image object and a `0x2329` record carry (§8.2) and the extension of
/// a file of the format. The types are 1 JPEG, 2 BMP, 3 TIFF, 4 PNG and
/// 7 TGA, which has no signature and is known by its header's fields.
/// An image of no known format is taken for a JPEG, as before.
pub(crate) fn image_format(bytes: &[u8]) -> (u32, &'static str) {
    let tga = || {
        // No identifier, no colour map, true colour (plain or packed),
        // 24 or 32 bits a pixel.
        bytes.len() > 18
            && bytes[0] == 0
            && bytes[1] == 0
            && matches!(bytes[2], 2 | 10)
            && matches!(bytes[16], 24 | 32)
    };
    if bytes.starts_with(b"\x89PNG") {
        (4, "png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        (1, "jpg")
    } else if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        (3, "tif")
    } else if bytes.starts_with(b"BM") {
        (2, "bmp")
    } else if tga() {
        (7, "tga")
    } else {
        (1, "jpg")
    }
}

pub(crate) fn image_type(bytes: &[u8]) -> u32 {
    image_format(bytes).0
}

pub(crate) fn image_ext(bytes: &[u8]) -> &'static str {
    image_format(bytes).1
}

// ---------------------------------------------------------------- meta.dat

/// `meta/meta.dat` (§16.1, §16.27): the version fields, a fresh model
/// GUID, the two thumbnail image records, the constant flags, the display
/// unit name, a temporary path of the observed shape, and the save
/// history with the write time.
///
/// `revision` 18 selects the family of SketchUp 2027 (`27.0.267`, the
/// writer line of the desktop client), 17 that of the web app and
/// SketchUp 2026 (`26.2.0`). An empty `history` writes the history of a
/// model converted from a stream-container file and saved once; otherwise
/// the history is carried and this save is recorded as SketchUp does: the
/// last entry's time is updated when it names the writing application,
/// else an entry is appended.
pub(crate) fn meta_dat(
    guid: &[u8; 16],
    unit_name: &str,
    temp_name: &str,
    when: u32,
    revision: crate::Revision,
    history: &[crate::SaveEntry],
) -> Vec<u8> {
    let desktop = revision.desktop();
    let image = |tag: u16, path: &str| {
        Record::kids(
            tag,
            vec![Record::kids(
                0x2328,
                vec![Record::u32(0x2329, 4), Record::string(0x232A, path)],
            )],
        )
    };
    let entry = |app: &str, time: u32, build: u64| crate::SaveEntry {
        app: app.to_string(),
        time,
        build,
    };
    let this_save = if desktop {
        entry(
            "SketchUp Client (MacQt) 27.0.267",
            when,
            0xb46a_5c8d_b7a4_1a4f,
        )
    } else {
        entry("SketchUp Web", when, 0x06ac60d8)
    };
    // Known difference: SketchUp rewrites the carried entries' `0xcd` on a
    // save by another application (§16.27, `rev-resave-box.skp`); the
    // writer carries them unchanged.
    let mut entries = history.to_vec();
    if entries.is_empty() {
        let pre = if desktop {
            0x235e_c0de_4405_8c27
        } else {
            0x6bef0ae6
        };
        entries.push(entry("Pre SU 2021", when, pre));
        entries.push(this_save);
    } else {
        match entries.last_mut() {
            Some(last) if last.app == this_save.app => last.time = when,
            _ => entries.push(this_save),
        }
    }
    let top = Record::kids(
        0x0064,
        vec![
            Record::string(0x0075, if desktop { "27.0.267" } else { "26.2.0" }),
            Record::u16(0x0076, 26),
            Record::u16(0x0077, if desktop { 3 } else { 1 }),
            Record::u16(0x0073, 1),
            Record::u16(0x0074, revision.number()),
            Record::leaf(0x0066, guid.to_vec()),
            image(0x0067, "meta/model_thumbnail.png"),
            image(0x0068, "meta/preview_thumbnail.png"),
            Record::u8(0x0069, 1),
            Record::empty(0x006A),
            Record::empty(0x006B),
            Record::empty(0x006C),
            Record::empty(0x006E),
            Record::u8(0x0071, 0),
            Record::u8(0x0079, 0),
            Record::u8(0x0072, 0),
            Record::string(0x006D, unit_name),
            Record::u8(0x0070, 1),
            Record::string(0x006F, &format!("/tmp/{temp_name}.skp")),
            Record::kids(
                0x0078,
                vec![Record::kids(
                    0x00C8,
                    vec![Record::kids(
                        0x00C9,
                        entries
                            .iter()
                            .map(|e| {
                                Record::kids(
                                    0x00CA,
                                    vec![
                                        Record::string(0x00CB, &e.app),
                                        Record::u32(0x00CC, e.time),
                                        Record::leaf(0x00CD, e.build.to_le_bytes().to_vec()),
                                    ],
                                )
                            })
                            .collect(),
                    )],
                )],
            ),
        ],
    );
    top.to_bytes()
}

/// An environment's `environment.xml`: the attributes in the order
/// SketchUp writes them, then the `ibl_image` element.
pub(crate) fn environment_xml(e: &crate::Environment) -> String {
    let f32t = |v: f32| g_digits(v as f64, 8);
    let mut a = format!(
        "name=\"{}\" description=\"{}\" ibl_enabled=\"{}\" rotation_angle=\"{}\" skybox_enabled=\"{}\" skybox_gain=\"{}\" reflection_gain=\"{}\" shadow_light_enabled=\"{}\" shadow_light_positionX=\"{}\" shadow_light_positionY=\"{}\" ibl_filename=\"{}\"",
        esc(&e.name),
        esc(&e.description),
        e.ibl_enabled as u8,
        f32t(e.rotation_angle),
        e.skybox_enabled as u8,
        f32t(e.skybox_gain),
        f32t(e.reflection_gain),
        e.shadow_light_enabled as u8,
        f32t(e.shadow_light_position[0]),
        f32t(e.shadow_light_position[1]),
        esc(&e.ibl_filename),
    );
    for (k, v) in &e.other_attrs {
        a.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<environmentDocument xmlns=\"http://sketchup.com/schemas/sketchup/1.0/environment\" xmlns:env=\"http://sketchup.com/schemas/sketchup/1.0/environment\">\n  <env:environment {a}>\n    <ibl_image id=\"{}\" path=\"{}\" file_name=\"{}\"/>\n  </env:environment>\n</environmentDocument>\n",
        e.image.id,
        esc(&e.image.path),
        esc(&e.image.file_name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g17_matches_c() {
        assert_eq!(g17(0.65), "0.65000000000000002");
        assert_eq!(g17(1.0), "1");
        assert_eq!(g17(0.8), "0.80000000000000004");
        assert_eq!(g17(0.1845), "0.1845");
        assert_eq!(g17(120.0), "120");
        assert_eq!(g17(0.0), "0");
        assert_eq!(g17(-7.0), "-7");
        assert_eq!(g17(1e-5), "1.0000000000000001e-05");
        assert_eq!(g17(f64::NAN), "nan");
        assert_eq!(g17(f64::INFINITY), "inf");
        assert_eq!(g17(f64::NEG_INFINITY), "-inf");
        assert_eq!(g_digits(0.8f32 as f64, 8), "0.80000001");
        assert_eq!(g_digits(2.5, 8), "2.5");
        assert_eq!(g_digits(10.0, 8), "10");
    }

    #[test]
    fn esc_keeps_xml_well_formed() {
        assert_eq!(
            esc("a<b>&\"c\"'d'"),
            "a&lt;b&gt;&amp;&quot;c&quot;&apos;d&apos;"
        );
        // Control characters XML 1.0 cannot carry are dropped; the three
        // whitespace controls stay.
        assert_eq!(esc("x\u{1}y\u{7f}\tz\n"), "xy\tz\n");
    }

    #[test]
    fn png_is_well_formed() {
        let p = flat_png(4, 3, [200, 200, 200]);
        assert!(p.starts_with(b"\x89PNG"));
        assert!(p.ends_with(&[0xAE, 0x42, 0x60, 0x82])); // IEND crc
                                                         // The IDAT inflates back to the raw scanlines.
        let idat_at = p.windows(4).position(|w| w == b"IDAT").unwrap();
        let len = u32::from_be_bytes(p[idat_at - 4..idat_at].try_into().unwrap()) as usize;
        let z = &p[idat_at + 4..idat_at + 4 + len];
        let mut out = vec![0u8; 4 * 3 * 3 + 3];
        let n = miniz_oxide::inflate::decompress_slice_iter_to_slice(
            &mut out,
            std::iter::once(&z[2..z.len() - 4]),
            false,
            true,
        )
        .unwrap();
        assert_eq!(n, 3 * (1 + 12));
    }

    #[test]
    fn meta_dat_parses_as_records() {
        let m = meta_dat(&[7u8; 16], "Inches", "x", 1, crate::Revision::R17, &[]);
        let mut notes = Vec::new();
        let r = Record::parse_model_dat(&m, &mut notes).unwrap();
        assert_eq!(r.tag, 0x64);
    }
}
