//! The 2026 settings records (SKP_FORMAT §16.9–§16.15) decoded into
//! the typed structs of [`crate::settings`].

use std::collections::HashMap;

use crate::model::Error;
use crate::read26::{entity_id, f64s, text, uint, xml_attrs, Node, Rec};
use crate::settings::*;
use crate::zip::Entries;
use crate::INCH;

fn m3(v: [f64; 3]) -> [f64; 3] {
    [v[0] / INCH, v[1] / INCH, v[2] / INCH]
}

fn f64_at(r: Option<Rec<'_>>) -> Option<f64> {
    let b = r?.data;
    (b.len() == 8).then(|| f64::from_le_bytes(b.try_into().unwrap()))
}

fn flag(r: Option<Rec<'_>>) -> bool {
    uint(r).unwrap_or(0) != 0
}

/// A packed id list (`<width:u8> <id>`, §16.2) as ids.
pub(crate) fn ids(r: Option<Rec<'_>>) -> Vec<u32> {
    ids_checked(r).0
}

/// [`ids`] and whether the list was cut short: an id wider than 4 bytes
/// (more than a persistent id holds) or running past the record ends it,
/// and the rest of the record is not read.
pub(crate) fn ids_checked(r: Option<Rec<'_>>) -> (Vec<u32>, bool) {
    let mut out = Vec::new();
    let Some(b) = r.map(|r| r.data) else {
        return (out, false);
    };
    let mut o = 0;
    while o < b.len() {
        let w = b[o] as usize;
        let v = match b.get(o + 1..o + 1 + w) {
            Some(v) if w <= 4 => v,
            _ => return (out, true),
        };
        out.push(v.iter().rev().fold(0u32, |a, &x| (a << 8) | x as u32));
        o += 1 + w;
    }
    (out, false)
}

/// A `0x34bc` camera record (§16.9).
pub(crate) fn camera(n: &Node<'_>) -> Result<Camera, Error> {
    Ok(Camera {
        eye_m: m3(f64s::<3>(n.get(0x34BD), n.at)?),
        target_m: m3(f64s::<3>(n.get(0x34BE), n.at)?),
        up: f64s::<3>(n.get(0x34BF), n.at)?,
        perspective: flag(n.get(0x34C2)),
        fov_deg: f64_at(n.get(0x34C4)).unwrap_or(0.0),
        parallel_height_m: f64_at(n.get(0x34C3)).unwrap_or(0.0) / INCH,
        two_point_perspective: flag(n.get(0x34CA)),
        description: String::new(),
        undecoded: [
            f64_at(n.get(0x34C0)).unwrap_or(1.0),
            f64_at(n.get(0x34C1)).unwrap_or(1000.0),
        ],
        aspect_ratio: f64_at(n.get(0x34C5)).unwrap_or(0.0),
        scale_2d: f64_at(n.get(0x34CB)).unwrap_or(1.0),
        offset_2d: [
            f64_at(n.get(0x34CC)).unwrap_or(0.0),
            f64_at(n.get(0x34CD)).unwrap_or(0.0),
        ],
        image_width: f64_at(n.get(0x34C9)).unwrap_or(0.0),
    })
}

/// A `0x733c` rendering-options record (§16.10).
pub(crate) fn rendering(n: &Node<'_>) -> RenderingOptions {
    let mut ro = RenderingOptions::default();
    for r in &n.kids {
        ro.set_record(r.tag, r.data);
    }
    ro
}

/// A `0x6590` shadow-info record (§16.11).
pub(crate) fn shadow(n: &Node<'_>) -> Result<ShadowInfo, Error> {
    Ok(ShadowInfo {
        time: uint(n.get(0x6591)).unwrap_or(0),
        city: text(n.get(0x6593)),
        country: text(n.get(0x6594)),
        longitude: f64_at(n.get(0x6595)).unwrap_or(0.0),
        latitude: f64_at(n.get(0x6596)).unwrap_or(0.0),
        tz_offset_h: f64_at(n.get(0x6597)).unwrap_or(0.0),
        displayed: flag(n.get(0x6599)),
        on_faces: flag(n.get(0x659B)),
        on_ground: flag(n.get(0x659C)),
        from_edges: flag(n.get(0x659D)),
        light: uint(n.get(0x659E)).unwrap_or(0),
        dark: uint(n.get(0x659F)).unwrap_or(0),
        use_sun_for_shading: flag(n.get(0x65A0)),
        daylight_saving: flag(n.get(0x6592)),
        north: f64s::<3>(n.get(0x6598), n.at).unwrap_or([0.0, 1.0, 0.0]),
        north_displayed: flag(n.get(0x659A)),
    })
}

/// A style document's parts: `(item id, variant type, text)` for the
/// scalar items, the
/// watermarks of item 5001, and the strokes of item 1013 with the file
/// names of their images.
type StyleDocument = (
    Vec<(u32, u32, String)>,
    Vec<(Watermark, Option<String>)>,
    Option<(Strokes, Vec<String>)>,
);

/// The items of a `style.xml` (§16.14).
fn style_document(xml: &str) -> StyleDocument {
    let mut items = Vec::new();
    let mut watermarks = Vec::new();
    let mut strokes = None;
    let mut rest = xml;
    while let Some(i) = rest.find("<sty:item id=\"") {
        rest = &rest[i + 14..];
        let Some(q) = rest.find('"') else { break };
        let Ok(id) = rest[..q].parse::<u32>() else {
            break;
        };
        let Some(v) = rest.find("<t:variant type=\"") else {
            break;
        };
        let after = &rest[v + 17..];
        let Some(q2) = after.find('"') else { break };
        let Ok(ty) = after[..q2].parse::<u32>() else {
            break;
        };
        let Some(gt) = after.find('>') else { break };
        let body = &after[gt + 1..];
        let end = body.find("</sty:item>").unwrap_or(body.len());
        if ty == 13 && id == 5001 {
            watermarks = screen_images(&body[..end]);
        } else if ty == 13 && id == crate::file17::ITEM_STROKES {
            strokes = npr_edge(&body[..end]);
        } else if ty == 13 {
            // Another structured item: none is observed.
        } else {
            let value =
                crate::read26::unescape(body[..end.min(body.find('<').unwrap_or(end))].trim());
            items.push((id, ty, value));
        }
        rest = &body[end..];
    }
    (items, watermarks, strokes)
}

/// Item 1013 of a sketchy-edge style, `<nprEdge>` (§16.14): the stroke
/// settings, the lengths, and the image file names in their order (the
/// variants of the first length first).
fn npr_edge(xml: &str) -> Option<(Strokes, Vec<String>)> {
    let a = xml_attrs(xml, ":nprEdge ")?;
    let f = |k: &str| {
        a.get(k)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let u = |k: &str| {
        a.get(k)
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    let mut lengths = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(":length>") {
        let body = &rest[i + 8..];
        let Some(end) = body.find('<') else { break };
        if let Ok(n) = body[..end].trim().parse::<u32>() {
            lengths.push(n);
        }
        rest = &body[end..];
    }
    let mut files = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(":image ") {
        let Some(im) = xml_attrs(&rest[i..], ":image ") else {
            break;
        };
        if let Some(name) = im.get("file_name") {
            files.push(name.clone());
        }
        rest = &rest[i + 7..];
    }
    Some((
        Strokes {
            dropout_length: f("dropoutLength"),
            fade_factor: f("fadeFactor"),
            width: u("width"),
            halo: u("useHalo") != 0,
            halo_length: u("haloLength"),
            variants: u("numProtos"),
            lengths,
            images: Vec::new(),
        },
        files,
    ))
}

/// The `<screenimage>` entries of a watermark list, in order, without the
/// `<MODEL SPACE>` separator (§16.14).
fn screen_images(xml: &str) -> Vec<(Watermark, Option<String>)> {
    let mut out = Vec::new();
    let mut rest = xml;
    let mut background = true;
    while let Some(i) = rest.find(":screenimage ") {
        let Some(a) = xml_attrs(&rest[i..], ":screenimage ") else {
            break;
        };
        let name = a.get("name").cloned().unwrap_or_default();
        if name == "<MODEL SPACE>" {
            background = false;
        } else {
            let b = |k: &str| a.get(k).map(|v| v.trim() != "0").unwrap_or(false);
            let f = |k: &str| {
                a.get(k)
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            // The image entry: `<n0:image path="watermarks/<file>">`,
            // looked for inside this entry only — up to the next
            // screenimage tag — and not at all when the tag is
            // self-closing.
            let entry = &rest[i..];
            let entry_end = entry[13..]
                .find(":screenimage ")
                .map(|j| j + 13)
                .unwrap_or(entry.len());
            let self_closing = entry
                .find('>')
                .map(|e| entry[..e].ends_with('/'))
                .unwrap_or(true);
            let image_path = if self_closing {
                None
            } else {
                xml_attrs(&entry[..entry_end], ":image ").and_then(|im| im.get("path").cloned())
            };
            out.push((
                Watermark {
                    name,
                    source_file: a.get("info_filename").cloned().unwrap_or_default(),
                    background,
                    tiled: b("tiled"),
                    stretched: b("stretched"),
                    keep_aspect_ratio: b("maintainAR"),
                    position: a
                        .get("position")
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0),
                    mask: b("intensForAlpha"),
                    blend: f("alphaScale"),
                    scale: f("scale"),
                    image_bytes: None,
                    time: a
                        .get("info_time")
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0),
                },
                image_path,
            ));
        }
        rest = &rest[i + 13..];
    }
    out
}

/// A style's settings from its `styles/<name>/style.xml`.
fn style_settings(
    zip: &dyn Entries,
    name: &str,
) -> (
    RenderingOptions,
    Vec<Watermark>,
    Vec<crate::StyleItem>,
    Option<Strokes>,
) {
    let mut ro = RenderingOptions::default();
    let mut extra = Vec::new();
    let Ok(Some(xml)) = zip.read(&format!("styles/{name}/style.xml")) else {
        return (ro, Vec::new(), extra, None);
    };
    let xml = String::from_utf8_lossy(&xml);
    let (items, watermarks, strokes) = style_document(&xml);
    // The stroke images sit beside the document.
    let strokes = strokes.map(|(mut st, files)| {
        st.images = files
            .iter()
            .filter_map(|f| zip.read(&format!("styles/{name}/{f}")).ok().flatten())
            .collect();
        st
    });
    let watermarks: Vec<Watermark> = watermarks
        .into_iter()
        .map(|(mut w, path)| {
            w.image_bytes = path.and_then(|p| zip.read(&p).ok().flatten());
            w
        })
        .collect();
    for (id, ty, value) in items {
        // An item the rendering options do not name, kept as it is.
        if !RO_FIELDS.iter().any(|f| f.item == Some(id)) {
            let v = match ty {
                1 => Some(crate::StyleValue::Bool(value.trim() != "0")),
                4 => value.trim().parse::<i32>().ok().map(crate::StyleValue::Int),
                7 => value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(crate::StyleValue::Double),
                _ => Some(crate::StyleValue::Other {
                    kind: ty,
                    text: value.clone(),
                }),
            };
            if let Some(value) = v {
                extra.push(crate::StyleItem { id, value });
            }
            continue;
        }
        let kind = match ty {
            1 => RoKind::Bool,
            4 => RoKind::U32,
            7 => RoKind::F64,
            6 => RoKind::F32,
            _ => continue,
        };
        if let Some(v) = RoValue::parse_item(kind, &value) {
            // colours travel as signed integers under type 4
            let v = match (v, RO_FIELDS.iter().find(|f| f.item == Some(id))) {
                (RoValue::U32(x), Some(f)) if f.kind == RoKind::Rgba => {
                    RoValue::Rgba(x.to_le_bytes())
                }
                _ => v,
            };
            ro.set_item(id, v);
        }
    }
    (ro, watermarks, extra, strokes)
}

/// The `desc` attribute of a style document's `<sty:style>`, unescaped.
fn document_description(zip: &dyn Entries, name: &str) -> Option<String> {
    let xml = zip.read(&format!("styles/{name}/style.xml")).ok()??;
    let xml = String::from_utf8_lossy(&xml);
    let at = xml.find("<sty:style")?;
    let tag = &xml[at..at + xml[at..].find('>')?];
    let i = tag.find(" desc=\"")? + 7;
    let j = tag[i..].find('"')?;
    Some(
        tag[i..i + j]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&"),
    )
}

/// The styles of `0x0206` (§16.14): the saved styles, the active style's
/// index, and the id → name map scenes refer to.
#[derive(Default)]
pub(crate) struct Styles26 {
    pub styles: Vec<Style>,
    pub active: Option<usize>,
    pub names: HashMap<u32, String>,
    /// The current settings' watermarks (`0x697b`, the `<name>_1` document).
    pub current_watermarks: Vec<Watermark>,
    /// The current-settings record when it is not the active style's
    /// plain copy.
    pub current: Option<crate::CurrentStyle>,
}

pub(crate) fn styles(top: &Node<'_>, zip: &dyn Entries) -> Result<Styles26, Error> {
    let mut out = Vec::new();
    let mut by_id = HashMap::new();
    let mut active = None;
    let mut current_watermarks = Vec::new();
    let mut current_rec: Option<(String, String)> = None;
    let Some(sec) = top.node(0x0206)? else {
        return Ok(Styles26 {
            styles: out,
            active,
            names: by_id,
            current_watermarks,
            current: None,
        });
    };
    let mgr = sec.need(0x6978)?;
    let active_id = uint(mgr.get(0x697A));
    if let Some(cur) = mgr.node(0x697B)? {
        if let Some(rec) = cur.node(0x6B6C)? {
            current_watermarks = style_settings(zip, &text(rec.get(0x6B6F))).1;
            let name = text(rec.get(0x6B6F));
            let description =
                document_description(zip, &name).unwrap_or_else(|| text(rec.get(0x6B6E)));
            current_rec = Some((name, description));
        }
    }
    let modified = uint(mgr.get(0x697C)).unwrap_or(0) != 0;
    if let Some(list) = mgr.node(0x6979)? {
        for st in list.nodes(0x6B6C)? {
            let id = entity_id(&st)?;
            let name = text(st.get(0x6B6F));
            let (settings, watermarks, items, strokes) = style_settings(zip, &name);
            by_id.insert(id, name.clone());
            if Some(id) == active_id {
                active = Some(out.len());
            }
            out.push(Style {
                name,
                description: text(st.get(0x6B6E)),
                guid: st
                    .get(0x6B6D)
                    .map(|g| g.data.iter().map(|b| format!("{b:02x}")).collect())
                    .unwrap_or_default(),
                settings,
                watermarks,
                strokes,
                items,
            });
        }
    }
    let current = match (current_rec, active.and_then(|a| out.get(a))) {
        (Some((name, description)), Some(a)) => {
            let own = document_description(zip, &a.name).unwrap_or_else(|| a.description.clone());
            let plain = name == format!("{}_1", a.name) && description == own;
            (!plain || modified).then_some(crate::CurrentStyle {
                name,
                description,
                modified,
            })
        }
        _ => None,
    };
    Ok(Styles26 {
        styles: out,
        active,
        names: by_id,
        current_watermarks,
        current,
    })
}

pub(crate) fn axes(n: &Node<'_>) -> Result<Axes, Error> {
    Ok(Axes {
        origin_m: m3(f64s::<3>(n.get(0x4651), n.at)?),
        x: f64s::<3>(n.get(0x4652), n.at)?,
        y: f64s::<3>(n.get(0x4653), n.at)?,
        z: f64s::<3>(n.get(0x4654), n.at)?,
    })
}

/// The scenes of `0x0207` (§16.15).
pub(crate) fn scenes(
    top: &Node<'_>,
    style_names: &HashMap<u32, String>,
    layer_index: &HashMap<u32, usize>,
    zip: &dyn crate::zip::Entries,
) -> Result<(Vec<Scene>, Option<usize>), Error> {
    let mut out = Vec::new();
    let Some(sec) = top.node(0x0207)? else {
        return Ok((out, None));
    };
    let photos = match_photos(top, zip)?;
    let list = sec.need(0x6D60)?;
    // The selected scene, by the id of its record (§16.10).
    let selected = uint(list.get(0x6D62));
    let mut scene_ids = Vec::new();
    for pg in list.need(0x6D61)?.nodes(0x7148)? {
        scene_ids.push(entity_id(&pg.need(0x6F54)?).ok());
        let base = pg.need(0x6F54)?;
        let saved = SceneProperties(uint(pg.get(0x7149)).unwrap_or(0));
        let camera = match pg.node(0x714A)? {
            Some(c) => Some(camera(&c.need(0x34BC)?)?),
            None => None,
        };
        let rendering = match pg.node(0x714D)? {
            Some(r) => Some(self::rendering(&r.need(0x733C)?)),
            None => None,
        };
        let shadows = match pg.node(0x714E)? {
            Some(s) => Some(shadow(&s.need(0x6590)?)?),
            None => None,
        };
        let axes = match pg.node(0x714F)? {
            Some(a) => Some(axes(&a.need(0x4650)?)?),
            None => None,
        };
        out.push(Scene {
            name: text(base.get(0x6F55)),
            description: text(base.get(0x6F56)),
            saved,
            camera,
            rendering,
            style: uint(pg.get(0x714C)).and_then(|id| style_names.get(&id).cloned()),
            shadows,
            axes,
            hidden_entities: ids(pg.get(0x714B)),
            active_section_planes: ids(pg.get(0x7151)),
            hidden_layers: ids(pg.get(0x7150))
                .iter()
                .filter_map(|id| layer_index.get(id).copied())
                .collect(),
            in_animation: flag(pg.get(0x7152)),
            // The preview: an entry the record names (§16.10).
            thumbnail: pg
                .node(0x7158)?
                .map(|t| text(t.get(0x2134)))
                .filter(|p| !p.is_empty())
                .and_then(|p| zip.read(&p).ok().flatten()),
            match_photo: uint(pg.get(0x7156)).and_then(|id| photos.get(&id).cloned()),
        });
    }
    let selected = selected.and_then(|id| scene_ids.iter().position(|x| *x == Some(id)));
    Ok((out, selected))
}

/// The matched photos of `0x0201` (§16.15) by their ids: each image's
/// source path, its archive entry, size and time, and the Match Photo
/// scale and points.
fn match_photos(
    top: &Node<'_>,
    zip: &dyn crate::zip::Entries,
) -> Result<HashMap<u32, MatchPhoto>, Error> {
    let mut out = HashMap::new();
    let Some(list) = top.node(0x0201)? else {
        return Ok(out);
    };
    for p in list.nodes(0x2904)? {
        let im = p.need(0x2905)?.need(0x2AF8)?;
        let entry = match im.node(0x2AFB)? {
            Some(r) => r.node(0x2328)?.map(|x| text(x.get(0x232A))),
            None => None,
        };
        let points = p
            .get(0x2908)
            .map(|r| {
                r.data
                    .as_chunks::<24>()
                    .0
                    .iter()
                    .map(|c| {
                        let f =
                            |k: usize| f64::from_le_bytes(c[k * 8..k * 8 + 8].try_into().unwrap());
                        [f(0), f(1), f(2)]
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.insert(
            entity_id(&p)?,
            MatchPhoto {
                path: text(im.get(0x2AF9)),
                image: entry
                    .filter(|e| !e.is_empty())
                    .and_then(|e| zip.read(&e).ok().flatten())
                    .unwrap_or_default(),
                width: uint(im.get(0x2AFC)).unwrap_or(0),
                height: uint(im.get(0x2AFD)).unwrap_or(0),
                time: uint(im.get(0x2AFF)).unwrap_or(0),
                scale: f64_at(p.get(0x2907)).unwrap_or(1.0),
                points,
            },
        );
    }
    Ok(out)
}

/// The fonts of `0x01fd` (§16.13) and their id → index map.
pub(crate) fn fonts(top: &Node<'_>) -> Result<(Vec<Font>, HashMap<u32, usize>), Error> {
    let mut out = Vec::new();
    let mut by_id = HashMap::new();
    let Some(sec) = top.node(0x01FD)? else {
        return Ok((out, by_id));
    };
    for f in sec.need(0x4E20)?.need(0x4E21)?.nodes(0x5014)? {
        by_id.insert(entity_id(&f)?, out.len());
        out.push(Font {
            family: text(f.get(0x5015)),
            bold: flag(f.get(0x5016)),
            italic: flag(f.get(0x5017)),
            size_pt: uint(f.get(0x5018)).unwrap_or(0),
            model_height: flag(f.get(0x5019)),
            height_m: f64_at(f.get(0x501A)).map_or(Font::DEFAULT_HEIGHT_M, |h| h / INCH),
        });
    }
    Ok((out, by_id))
}

fn anchor(n: Option<Node<'_>>, at: usize) -> Result<Anchor, Error> {
    let Some(a) = n else {
        return Ok(Anchor::default());
    };
    let a = a.need(0x5208)?;
    let kind = uint(a.get(0x5209)).unwrap_or(0);
    let p = f64s::<3>(a.get(0x520A), at)?;
    // `0x520b > 0x53fc` names the anchored entity (§16.13), and
    // `0x520c` a second one the same way (kind 9).
    let target = |tag: u16| -> Result<(Option<u32>, Vec<u32>), Error> {
        Ok(match a.node(tag)? {
            Some(e) => match e.node(0x53FC)? {
                Some(t) => (uint(t.get(0x53FD)), ids(t.get(0x53FE))),
                None => (None, Vec::new()),
            },
            None => (None, Vec::new()),
        })
    };
    let (entity, path) = target(0x520B)?;
    let (second, second_path) = target(0x520C)?;
    Ok(Anchor {
        kind,
        point_m: if kind == 5 {
            [p[0], p[1] / INCH, p[2] / INCH]
        } else {
            m3(p)
        },
        entity,
        path,
        second,
        second_path,
    })
}

/// The texts of an entity container (§16.13).
pub(crate) fn texts(
    container: &Node<'_>,
    fonts: &HashMap<u32, usize>,
    def_index: Option<usize>,
    slots: &crate::read26::Slots,
) -> Result<Vec<(Text, Vec<crate::Dictionary>)>, Error> {
    let mut out = Vec::new();
    let Some(list) = container.node(0x1398)? else {
        return Ok(out);
    };
    for t in list.nodes(0x55F0)? {
        let leader = match uint(t.get(0x55FD)).unwrap_or(0) {
            1 => Leader::ViewBased,
            2 => Leader::Pushpin,
            _ => Leader::None,
        };
        let off = f64s::<3>(t.get(0x55F5), t.at)?;
        let (layer, material, hidden) = slots.drawn(&t)?;
        let (casts_shadows, receives_shadows) = slots.shadows(&t)?;
        let dicts = crate::read26::read_dictionaries(crate::read26::attrs_of_drawing(&t));
        out.push((
            Text {
                layer,
                material,
                hidden,
                casts_shadows,
                receives_shadows,
                def_index,
                pid: entity_id(&t.need(0x07D0)?)?,
                content: text(t.get(0x55F1)),
                placeholder: text(t.get(0x55FF)),
                screen_position: [
                    f64_at(t.get(0x55F2)).unwrap_or(0.0),
                    f64_at(t.get(0x55F3)).unwrap_or(0.0),
                ],
                anchor: anchor(t.node(0x55F4)?, t.at)?,
                leader_offset: if leader == Leader::Pushpin {
                    m3(off)
                } else {
                    off
                },
                leader,
                view_direction: f64s::<3>(t.get(0x55F7), t.at).unwrap_or([0.0, 0.0, -1.0]),
                arrow: uint(t.get(0x55FA)).unwrap_or(0),
                font: uint(t.get(0x55F9)).and_then(|id| fonts.get(&id).copied()),
                leader_hidden: uint(t.get(0x55FC)) == Some(0),
            },
            dicts,
        ));
    }
    Ok(out)
}

/// The linear dimensions of an entity container (§16.13).
pub(crate) fn dimensions(
    container: &Node<'_>,
    fonts: &HashMap<u32, usize>,
    def_index: Option<usize>,
    slots: &crate::read26::Slots,
) -> Result<Vec<(Dimension, Vec<crate::Dictionary>)>, Error> {
    let mut out = Vec::new();
    let Some(list) = container.node(0x1399)? else {
        return Ok(out);
    };
    for d in list.nodes(0x5BCC)? {
        let base = d.need(0x59D8)?;
        let (layer, material, hidden) = slots.drawn(&base)?;
        let (casts_shadows, receives_shadows) = slots.shadows(&base)?;
        let dicts = crate::read26::read_dictionaries(crate::read26::attrs_of_drawing(&base));
        out.push((
            Dimension {
                layer,
                material,
                hidden,
                casts_shadows,
                receives_shadows,
                arrow: uint(base.get(0x59DC)).unwrap_or(0),
                def_index,
                pid: entity_id(&base.need(0x07D0)?)?,
                text_override: text(base.get(0x59D9)),
                start: anchor(d.node(0x5BCD)?, d.at)?,
                end: anchor(d.node(0x5BCE)?, d.at)?,
                normal: f64s::<3>(d.get(0x5BCF), d.at)?,
                x_axis: f64s::<3>(d.get(0x5BD0), d.at)?,
                offset_m: f64_at(d.get(0x5BD2)).unwrap_or(0.0) / INCH,
                text_position: uint(d.get(0x5BD4)).unwrap_or(0),
                variant: uint(d.get(0x5BD1)).unwrap_or(1),
                undecoded: f64_at(d.get(0x5BD3)).unwrap_or(0.0),
                aligned: flag(base.get(0x59DB)),
                font: uint(base.get(0x59DA)).and_then(|id| fonts.get(&id).copied()),
            },
            dicts,
        ));
    }
    Ok(out)
}

/// The radial dimensions of an entity container (§16.13 `0x139a`).
pub(crate) fn radial_dimensions(
    container: &Node<'_>,
    fonts: &HashMap<u32, usize>,
    def_index: Option<usize>,
    slots: &crate::read26::Slots,
) -> Result<Vec<(RadialDimension, Vec<crate::Dictionary>)>, Error> {
    let mut out = Vec::new();
    let Some(list) = container.node(0x139A)? else {
        return Ok(out);
    };
    for d in list.nodes(0x5DC0)? {
        let base = d.need(0x59D8)?;
        let (layer, material, hidden) = slots.drawn(&base)?;
        let (casts_shadows, receives_shadows) = slots.shadows(&base)?;
        let dicts = crate::read26::read_dictionaries(crate::read26::attrs_of_drawing(&base));
        out.push((
            RadialDimension {
                layer,
                material,
                hidden,
                casts_shadows,
                receives_shadows,
                arrow: uint(base.get(0x59DC)).unwrap_or(0),
                def_index,
                pid: entity_id(&base.need(0x07D0)?)?,
                text_override: text(base.get(0x59D9)),
                aligned: flag(base.get(0x59DB)),
                font: uint(base.get(0x59DA)).and_then(|id| fonts.get(&id).copied()),
                edge: uint(d.get(0x5DC1)),
                // No 2026 dimension without an edge has been observed.
                arc: None,
                leader: [
                    f64_at(d.get(0x5DC3)).unwrap_or(0.0),
                    f64_at(d.get(0x5DC4)).unwrap_or(0.0),
                ],
                diameter: flag(d.get(0x5DC5)),
            },
            dicts,
        ));
    }
    Ok(out)
}

/// A definition's `0x1b58` behaviour (§16.5).
pub(crate) fn behaviour(def: &Node<'_>) -> Result<Behaviour, Error> {
    let Some(b) = def.node(0x1B58)? else {
        return Ok(Behaviour::default());
    };
    Ok(Behaviour {
        glues_to_surface: flag(b.get(0x1B5B)),
        glue_plane: uint(b.get(0x1B59)).unwrap_or(0),
        cuts_opening: flag(b.get(0x1B5C)),
        always_faces_camera: flag(b.get(0x1B5D)),
        shadows_face_sun: flag(b.get(0x1B5E)),
        extra: uint(b.get(0x1B5A)).unwrap_or(0),
    })
}

/// Defaults for new texts, `0x01fe > 0x57e4` (§16.13).
pub(crate) fn text_defaults(
    top: &Node<'_>,
    fonts: &HashMap<u32, usize>,
) -> Result<Option<TextDefaults>, Error> {
    let Some(sec) = top.node(0x01FE)? else {
        return Ok(None);
    };
    let Some(r) = sec.node(0x57E4)? else {
        return Ok(None);
    };
    let rgba = |t: u16| -> [u8; 4] {
        r.get(t)
            .and_then(|q| q.data.try_into().ok())
            .unwrap_or([0, 0, 0, 255])
    };
    Ok(Some(TextDefaults {
        font: uint(r.get(0x57E5)).and_then(|id| fonts.get(&id).copied()),
        screen_font: uint(r.get(0x57E6)).and_then(|id| fonts.get(&id).copied()),
        arrow: uint(r.get(0x57E7)).unwrap_or(0),
        leader: uint(r.get(0x57EA)).unwrap_or(2),
        leader_text_color: rgba(0x57EC),
        screen_text_color: rgba(0x57ED),
    }))
}

/// Defaults for new dimensions, `0x01ff > 0x5fb4` (§16.13).
pub(crate) fn dimension_defaults(
    top: &Node<'_>,
    fonts: &HashMap<u32, usize>,
) -> Result<Option<DimensionDefaults>, Error> {
    let Some(sec) = top.node(0x01FF)? else {
        return Ok(None);
    };
    let Some(r) = sec.node(0x5FB4)? else {
        return Ok(None);
    };
    Ok(Some(DimensionDefaults {
        font: uint(r.get(0x5FB5)).and_then(|id| fonts.get(&id).copied()),
        aligned: flag(r.get(0x5FB6)),
        arrow: uint(r.get(0x5FBB)).unwrap_or(0),
        text_position: uint(r.get(0x5FC6)).unwrap_or(0),
        highlight_non_associated: flag(r.get(0x5FBD)),
        radius_prefix: r.get(0x5FBF).is_none_or(|q| q.data.first() != Some(&0)),
        hide_foreshortened: flag(r.get(0x5FC0)),
        hide_small: flag(r.get(0x5FC2)),
        color: r
            .get(0x5FC4)
            .and_then(|q| q.data.try_into().ok())
            .unwrap_or([0, 0, 0, 255]),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_image_paths_stay_with_their_own_entry() {
        let xml = concat!(
            "<n0:screenimage name=\"A\" info_filename=\"a.png\" tiled=\"0\"/>",
            "<n0:screenimage name=\"B\" info_filename=\"b.png\" tiled=\"1\">",
            "<n0:images><n0:image path=\"watermarks/b.png\"/></n0:images>",
            "</n0:screenimage>",
            "<n0:screenimage name=\"C\" info_filename=\"c.png\"><n0:images/></n0:screenimage>",
        );
        let out = screen_images(xml);
        let paths: Vec<(String, Option<String>)> =
            out.into_iter().map(|(w, p)| (w.name, p)).collect();
        assert_eq!(
            paths,
            vec![
                ("A".to_string(), None),
                ("B".to_string(), Some("watermarks/b.png".to_string())),
                ("C".to_string(), None),
            ]
        );
    }
}
