//! The model of a stream-container file, read by the whole-file grammar
//! ([`crate::file17`], `docs/SKP_FORMAT.md` §2–§10): the file is read
//! once from its header to its last byte, every object in the place the
//! grammar gives it, and the model is built from what was read. Nothing
//! is searched for and nothing is calibrated: a material is a `CMaterial`
//! object, a scene is an element of the scene list, the root entities
//! are the model's entity list.
//!
//! The objects are collected as they are read ([`crate::ar17::Collected`]):
//! each as the [`Entity`] the mesh and the scene are built from, in the
//! store map slot the file gives it, and the records the settings are
//! read from — materials, layers, scenes, styles — as the grammar's own
//! typed records.

use std::collections::HashMap;

use crate::ar17::{Collected, Frame, S16};
use crate::carchive::{Child, Slot};
use crate::entity::{DefMeta, DimensionBody, Entity, Ftc, RadialDimensionBody, TextBody};
use crate::file17::{
    AttrVal, Axes17, Body, Camera17, Entry, File17, InstanceClass, Pre, Ptr, Rendering17, Shadow17,
    Style17, StyleVal, Watermark17, ITEM_WATERMARKS,
};
use crate::model::Error;
use crate::settings::*;
use crate::settings17::Settings17;
use crate::walk2::{Continuous, DefSpan};
use crate::{AttrValue, Attribute, Image, Material, INCH};

fn child(p: &Ptr) -> Child {
    match p.id() {
        Some(id) => Child::Obj(id as usize),
        None => Child::Null,
    }
}

/// The slot a drawing base names, as the model holds it: 0 for none, and
/// for a slot past what the model's 16 bits hold.
fn slot16(p: &Ptr) -> u16 {
    p.id().and_then(|id| u16::try_from(id).ok()).unwrap_or(0)
}

/// A drawing base's flags in the 2026 layout (§16.4).
fn flags(d: &crate::file17::Drawbase) -> u8 {
    (d.hidden != 0) as u8
        | ((d.casts_shadows != 0) as u8) << 1
        | ((d.receives_shadows != 0) as u8) << 2
        | ((d.soft != 0) as u8) << 3
        | ((d.smooth != 0) as u8) << 4
}

fn drawn(d: &crate::file17::Drawbase) -> crate::entity::Drawn {
    crate::entity::Drawn {
        material: slot16(&d.material),
        layer: slot16(&d.layer),
        hidden: d.hidden != 0,
        casts_shadows: d.casts_shadows != 0,
        receives_shadows: d.receives_shadows != 0,
    }
}

fn slot32(p: &Ptr) -> u32 {
    p.id().unwrap_or(0)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The records kept beside the entities: what the settings, the
/// materials and the attributes are read from.
pub(crate) fn kept(b: &Body) -> bool {
    matches!(
        b,
        Body::Dib(_)
            | Body::Camera(_)
            | Body::Thumbnail(_)
            | Body::AttrContainer(_)
            | Body::AttrNamed(_)
            | Body::Material(_)
            | Body::Scene(_)
            | Body::Style(_)
            | Body::Watermark(_)
            | Body::BackgroundImage(_)
            | Body::SchemaZip(_)
            | Body::Def(_)
            | Body::Layer(_)
            | Body::LayerGroup(_)
            | Body::LineStyle(_)
    )
}

/// The attribute container an object names in its preamble, or, for a
/// material, in its texture.
pub(crate) fn attributes_of(b: &Body) -> Option<u32> {
    if let Body::Material(m) = b {
        if let Some(id) = m.texture.as_ref().and_then(|t| t.attrs.id()) {
            return Some(id);
        }
    }
    b.pre()?.attrs.id()
}

/// An object as the entity the model is built from. In a release that
/// stores no persistent ids (§14) an entity's id is its store-map slot:
/// unique within the file and the same on every read of its bytes, so
/// what the model keys by id — scene visibility, annotation anchors,
/// in-place edits — holds.
pub(crate) fn entity(b: &Body, f: &Frame, release: crate::Release) -> Entity {
    let pid = |p: &Pre| if release.has_pids() { p.pid } else { f.id };
    match b {
        Body::VersionMap(v) => Entity::VersionMap {
            entries: v
                .iter()
                .filter(|(n, _)| n.text() != crate::file17::END_OF_VERSION_MAP)
                .map(|(n, s)| (n.text(), *s))
                .collect(),
        },
        Body::Dib(v) => Entity::Dib {
            subtype: v.subtype,
            at: f.marks[0] + 4,
            len: v.data.0.len(),
        },
        Body::Camera(v) => Entity::Camera {
            desc: v.description.text(),
        },
        Body::Thumbnail(v) => Entity::Thumbnail {
            camera: child(&v.camera),
            image: child(&v.image),
        },
        Body::AttrContainer(v) => Entity::AttrContainer {
            children: v.dictionaries.iter().map(child).collect(),
        },
        Body::AttrNamed(v) => Entity::AttrNamed {
            name: v.name.text(),
            entries: v.entries.len(),
        },
        Body::Material(v) => Entity::Material {
            pid: pid(&v.pre),
            name: v.name.text(),
            dib: v
                .texture
                .as_ref()
                .map(|t| child(&t.image))
                .unwrap_or(Child::Null),
        },
        Body::Layer(v) => Entity::Layer {
            pid: pid(&v.pre),
            name: v.name.text(),
            hidden: v.hidden != 0,
            rgba: v.rgba,
        },
        Body::Def(v) => Entity::ComponentDef {
            pid: pid(&v.space.pre),
            meta: Box::new(DefMeta {
                name: v.name.text(),
                desc: v.description.text(),
                source: v.source.text(),
                guid: hex(&v.guid),
                timestamp: v.timestamp,
                behaviour: Behaviour {
                    glues_to_surface: v.mid[28] != 0,
                    cuts_opening: v.mid[29] != 0,
                    glue_plane: u32::from_le_bytes([v.mid[30], v.mid[31], v.mid[32], v.mid[33]]),
                    always_faces_camera: v.mid[34] & 1 != 0,
                    shadows_face_sun: v.mid[34] & 2 != 0,
                    extra: u32::from_le_bytes([v.mid[35], v.mid[36], v.mid[37], v.mid[38]]),
                },
                flag: v.mid[0] != 0,
                point_in: [1, 9, 17]
                    .map(|o| f64::from_le_bytes(v.mid[o..o + 8].try_into().unwrap())),
            }),
            decl_index: (slot32(&v.space.active_layer) as usize).saturating_sub(1),
            count: v.space.entities.len(),
            entities: v.space.entities.iter().map(child).collect(),
        },
        Body::Vertex(v) => Entity::Vertex {
            pid: pid(&v.pre),
            x: v.p[0],
            y: v.p[1],
            z: v.p[2],
            at: f.marks[0] as u32,
        },
        Body::Edge(v) => Entity::Edge {
            pid: pid(&v.pre),
            at: f.marks[0] as u32,
            material: slot16(&v.drawbase.material),
            flags: flags(&v.drawbase),
            layer: slot16(&v.drawbase.layer),
            v0: child(&v.v0),
            v1: child(&v.v1),
            curve: child(&v.curve),
        },
        Body::EdgeUse(v) => Entity::EdgeUse {
            edge: child(&v.edge),
            dir: v.dir,
            parent: child(&v.parent),
        },
        Body::Loop(v) => Entity::Loop {
            edge_uses: v.uses.iter().map(child).collect(),
        },
        Body::Face(v) => Entity::Face {
            pid: pid(&v.pre),
            at: f.marks[0] as u32,
            front_material: slot16(&v.drawbase.material),
            back_material: slot16(&v.back_material),
            flags: flags(&v.drawbase),
            layer: slot16(&v.drawbase.layer),
            plane: Box::new(v.plane),
            loops: v.loops.iter().map(child).collect(),
            attrs: child(&v.pre.attrs),
        },
        Body::ArcCurve(v) => Entity::ArcCurve {
            pid: pid(&v.pre),
            params: Box::new(v.params),
        },
        Body::Curve(v) => Entity::Curve {
            pid: pid(&v.pre),
            members: v.members,
        },
        Body::Instance(v) if v.class == InstanceClass::Image => Entity::Image {
            pid: pid(&v.pre),
            defref: slot32(&v.definition),
            transform: Box::new(v.transform),
            hidden: v.drawbase.hidden != 0,
            layer: slot16(&v.drawbase.layer),
            flags: flags(&v.drawbase),
        },
        Body::Instance(v) => Entity::InstancePlaced {
            pid: pid(&v.pre),
            defref: slot32(&v.definition),
            transform: Box::new(v.transform),
            tf_at: f.marks[1],
            drawbase_at: f.marks[0],
            hidden: v.drawbase.hidden != 0,
            layer: slot16(&v.drawbase.layer),
            material: slot16(&v.drawbase.material),
            is_group: v.class == InstanceClass::Group,
            more: Box::new(crate::entity::InstanceMore {
                name: v.name.text(),
                guid: v.guid,
                casts_shadows: v.drawbase.casts_shadows != 0,
                receives_shadows: v.drawbase.receives_shadows != 0,
                locked: v.drawbase.locked != 0,
            }),
        },
        Body::Relationship(v) => Entity::Relationship {
            pid: pid(&v.pre),
            a: child(&v.a),
            b: child(&v.b),
            next: Child::Null,
        },
        Body::Ftc(v) => {
            let part = |lo: usize, hi: usize| -> Vec<f64> { v.k[lo..hi].to_vec() };
            Entity::FaceTextureCoords {
                pid: pid(&v.pre),
                ftc: Box::new(Ftc {
                    front: part(0, 9).try_into().unwrap_or([0.0; 9]),
                    front_extra: part(9, 12).try_into().unwrap_or([0.0; 3]),
                    back: part(12, 21).try_into().unwrap_or([0.0; 9]),
                    back_extra: part(21, 24).try_into().unwrap_or([0.0; 3]),
                    front_pins: v.front_pins.clone(),
                    back_pins: v.back_pins.clone(),
                    flags: v.flags,
                }),
            }
        }
        Body::Font(v) => Entity::Font {
            name: v.name.text(),
            bold: v.tail[0] != 0,
            italic: v.tail[1] != 0,
            size_pt: u32::from_le_bytes([v.tail[2], v.tail[3], v.tail[4], v.tail[5]]),
            model_height: v.tail[6] != 0,
            // A release before 6 holds no height: the default (§14).
            height_in: if release.font_tail_len() < 15 {
                10.0
            } else {
                f64::from_le_bytes(v.tail[7..15].try_into().unwrap())
            },
        },
        Body::RadialDimension(v) => Entity::RadialDimension {
            pid: pid(&v.pre),
            body: Box::new(RadialDimensionBody {
                drawn: drawn(&v.drawbase),
                arrow: v.arrow,
                text_override: v.text.text(),
                font: child(&v.font),
                aligned: v.aligned != 0,
                edge: child(&v.edge),
                leader: v.leader,
                diameter: v.diameter != 0,
                arc: v.arc,
            }),
        },
        Body::Polyline(v) => Entity::Polyline {
            pid: pid(&v.pre),
            points_in: v.points.clone(),
            drawn: drawn(&v.drawbase),
        },
        Body::Dimension(v) => Entity::Dimension {
            pid: pid(&v.pre),
            body: Box::new(DimensionBody {
                drawn: drawn(&v.drawbase),
                arrow: v.arrow,
                text_override: v.text.text(),
                font: child(&v.font),
                anchors: [
                    (v.start.kind, v.start.point, child(&v.start.entity)),
                    (v.end.kind, v.end.point, child(&v.end.entity)),
                ],
                paths: [
                    v.start.path.iter().map(child).collect(),
                    v.end.path.iter().map(child).collect(),
                ],
                seconds: [
                    (
                        child(&v.start.second),
                        v.start.second_path.iter().map(child).collect(),
                    ),
                    (
                        child(&v.end.second),
                        v.end.second_path.iter().map(child).collect(),
                    ),
                ],
                normal: v.normal,
                x_axis: v.x_axis,
                offset: v.offset,
                text_position: v.text_position,
                aligned: v.aligned != 0,
                variant: v.u,
                zero: v.zero,
            }),
        },
        Body::Text(v) => Entity::Text {
            pid: pid(&v.pre),
            body: Box::new(TextBody {
                drawn: drawn(&v.drawbase),
                content: v.content.text(),
                font: child(&v.font),
                screen_position: v.screen,
                anchor_kind: v.anchor.kind,
                anchor_point: v.anchor.point,
                anchor_entity: child(&v.anchor.entity),
                anchor_path: v.anchor.path.iter().map(child).collect(),
                leader_offset: v.leader_offset,
                view_direction: v.view_direction,
                leader: v.leader,
                arrow: v.arrow,
                leader_hidden: v.leader_shown == 0,
            }),
        },
        Body::SectionPlane(v) => Entity::SectionPlane {
            pid: pid(&v.pre),
            plane: v.plane,
            drawn: drawn(&v.drawbase),
            names: Box::new([text16(&v.names[0]), text16(&v.names[1])]),
        },
        Body::GuidePoint(v) => Entity::ConstructionPoint {
            pid: pid(&v.pre),
            points_in: Box::new([v.point, v.anchor]),
            drawn: drawn(&v.drawbase),
            flag: v.b,
        },
        Body::GuideLine(v) => Entity::ConstructionLine {
            pid: pid(&v.pre),
            params: Box::new(v.params),
            drawn: drawn(&v.drawbase),
            // The dash pattern is stored from 2018; a zero (a guide the
            // save left as it was, and every guide before 2018) is the
            // plain solid line (§14.5).
            stipple: match u16::from_le_bytes([v.tail[0], v.tail[1]]) {
                0 => 0xFFFF,
                m if release >= crate::Release::R2018 => m,
                _ => 0xFFFF,
            },
        },
        Body::Scene(_)
        | Body::Style(_)
        | Body::Watermark(_)
        | Body::BackgroundImage(_)
        | Body::LineStyle(_)
        | Body::LayerGroup(_)
        | Body::SchemaZip(_) => Entity::Other(b.class_name().to_string()),
    }
}

/// What the grammar read of a file, for [`crate::Model`] to be built
/// from: the structure the entities are grouped by, and everything that
/// is no entity.
pub(crate) struct Read17 {
    pub walk: Continuous,
    pub materials: Vec<Material>,
    pub material_links: Vec<(u16, usize)>,
    pub attributes: Vec<Attribute>,
    pub images: Vec<Image>,
    pub settings: Settings17,
    /// The slot of the model's active layer.
    pub active_layer: Option<usize>,
    /// The slot of the current paint material.
    pub current_material: Option<usize>,
    /// The dictionaries of every object that holds any, by the object's
    /// slot, and the model's own (its space's, then its properties).
    pub dictionaries: Vec<(usize, Vec<crate::Dictionary>)>,
    pub model_dictionaries: Vec<crate::Dictionary>,
    /// The dictionaries of the display settings record.
    pub rendering_dictionaries: Vec<crate::Dictionary>,
    /// The dictionaries of the document's settings and lists (§16.8):
    /// the lists, the styles, the axes, the shadow settings, the line
    /// styles, the scenes and their copies, and the layer folders.
    pub settings_dictionaries: Vec<(crate::Owner, Vec<crate::Dictionary>)>,
    /// The model's preview images.
    pub thumbnail: Option<Vec<u8>>,
    pub preview: Option<Vec<u8>>,
    /// Each definition's preview image and camera, by the definition's
    /// slot.
    pub definition_thumbnails: HashMap<usize, (Option<Vec<u8>>, Option<Camera>)>,
    pub classifications: Vec<crate::Classification>,
    /// The post-2017 features this release stores (2019: layer line
    /// styles, 2020: layer folders).
    pub features: crate::Features,
}

/// Read a stream-container file by the grammar. An error names what does not fit:
/// a structure that is not decoded, a file of another release, a file
/// that is damaged.
pub(crate) fn read(d: &[u8]) -> Result<Read17, Error> {
    let (f, c) = File17::collect(d)?;
    let Collected {
        map,
        mut definitions,
        file,
        records,
        attributes: owned,
        ..
    } = c;
    definitions.sort_by_key(|f| f.id);

    // A definition holds the slots from its own to the last assigned
    // while it was read, short of those of the definitions inside it.
    // The definitions are in slot order, so those inside one follow it
    // in the list and end where the first one past its last slot begins.
    let own = |outer: &Frame| -> Vec<(usize, usize)> {
        let (lo, hi) = (outer.id as usize + 1, outer.last as usize + 1);
        let mut ranges = Vec::new();
        let mut from = lo;
        let first = definitions.partition_point(|i| i.id <= outer.id);
        for inner in definitions[first..]
            .iter()
            .take_while(|i| i.id <= outer.last)
        {
            let (a, b) = (inner.id as usize, inner.last as usize + 1);
            if a < from {
                continue; // inside a definition already left out
            }
            if a > from {
                ranges.push((from, a));
            }
            from = b;
        }
        if hi > from {
            ranges.push((from, hi));
        }
        ranges
    };
    let defs: Vec<DefSpan> = definitions
        .iter()
        .map(|f| DefSpan {
            slot: f.id as usize,
            start: f.at,
            end: f.end,
            members: own(f),
        })
        .collect();
    // The model's entity list: the slots assigned inside it, short of
    // the definitions written inside (none in an observed file).
    let root = Frame {
        id: file.entities.0,
        last: file.entities.1,
        ..Default::default()
    };
    let layer_slots: Vec<usize> = f
        .space
        .layers
        .iter()
        .filter_map(|p| p.id())
        .map(|id| id as usize)
        .filter(|&s| matches!(map.get(s), Some(Slot::Object(Entity::Layer { .. }))))
        .collect();
    let base = layer_slots
        .first()
        .map(|s| s.saturating_sub(2))
        .unwrap_or(0);
    let walk = Continuous {
        base,
        layer_slots,
        defs,
        roots: f.space.entities.iter().map(child).collect(),
        root_list_at: file.marks[1],
        root_slot_floor: file.entities.0 as usize + 1,
        root_members: own(&root),
        end: d.len(),
        root_active_slot: f.space.active_section.id().map(|id| id as usize),
        bound: Vec::new(),
        map,
    };

    let ids = {
        let mut v: Vec<u32> = records.keys().copied().collect();
        v.sort_unstable();
        v
    };
    let (materials, material_links) = materials(&ids, &records);
    let attributes = attributes(&f, &ids, &records);
    let images = images(&ids, &records);
    let settings = settings(&f, &ids, &records, &walk);
    let features = features(&f, &records);
    let settings_dictionaries = settings_dictionaries(&f, &records, &settings.styles);
    Ok(Read17 {
        walk,
        features,
        materials,
        material_links,
        attributes,
        images,
        settings,
        active_layer: f.space.active_layer.id().map(|id| id as usize),
        current_material: f.space.current_material.id().map(|id| id as usize),
        dictionaries: owned
            .iter()
            .filter_map(|&(owner, container)| {
                let dicts = dictionaries_of(container, &records);
                (!dicts.is_empty()).then_some((owner as usize, dicts))
            })
            .collect(),
        settings_dictionaries,
        rendering_dictionaries: f
            .rendering
            .pre
            .attrs
            .id()
            .map(|id| dictionaries_of(id, &records))
            .unwrap_or_default(),
        model_dictionaries: [&f.space.pre.attrs, &f.properties]
            .into_iter()
            .filter_map(|p| p.id())
            .flat_map(|id| dictionaries_of(id, &records))
            .collect(),
        thumbnail: image_bytes(&f.thumbnail, &records),
        preview: image_bytes(&f.preview, &records),
        classifications: f
            .schemas
            .iter()
            .filter_map(|p| match records.get(&p.id()?)? {
                Body::SchemaZip(z) => Some(crate::Classification {
                    name: z.name.text(),
                    source_path: z.path.text(),
                    data: z.data.0.clone(),
                }),
                _ => None,
            })
            .collect(),
        definition_thumbnails: records
            .iter()
            .filter_map(|(id, b)| match b {
                Body::Def(d) => match records.get(&d.thumbnail.id()?)? {
                    Body::Thumbnail(t) => Some((
                        *id as usize,
                        (
                            image_bytes(&t.image, &records),
                            camera_of(&t.camera, &records, f.release),
                        ),
                    )),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
    })
}

/// A value of a dictionary as the model holds it.
fn value(v: &AttrVal) -> crate::Value {
    use crate::Value;
    match v {
        AttrVal::Nil => Value::Nil,
        AttrVal::I32(x) => Value::Int(*x),
        AttrVal::F32(x) => Value::Float(*x),
        AttrVal::F64(x) => Value::Double(*x),
        AttrVal::Bool(b) => Value::Bool(*b != 0),
        AttrVal::Time(t) => Value::Time(*t),
        AttrVal::Str(s) => Value::Text(s.text()),
        AttrVal::Array(items) => Value::Array(items.iter().map(value).collect()),
        AttrVal::Raw8(b) => Value::Bytes8(*b),
        AttrVal::Vec3(p) => Value::Vector(*p),
        AttrVal::Dir3(p) => Value::Direction(*p),
    }
}

/// The dictionaries of the attribute container at `container` (§10.5),
/// in order; a face's texture placement in the same container is not
/// one.
/// The dictionaries of the document's settings and lists, by owner, in
/// the order of `owners26::read` (§16.8): each such record's preamble
/// (§5.1) points at them.
fn settings_dictionaries(
    f: &File17,
    records: &HashMap<u32, Body>,
    styles: &[Style],
) -> Vec<(crate::Owner, Vec<crate::Dictionary>)> {
    use crate::Owner;
    let mut out = Vec::new();
    let mut add = |owner: Owner, pre: &Pre| {
        if let Some(id) = pre.attrs.id() {
            let d = dictionaries_of(id, records);
            if !d.is_empty() {
                out.push((owner, d));
            }
        }
    };
    let body = |p: &Ptr| p.id().and_then(|id| records.get(&id));
    add(Owner::Materials, &f.space.materials_pre);
    add(Owner::Layers, &f.space.layers_pre);
    add(Owner::Scenes, &f.scenes_pre);
    add(Owner::Styles, &f.styles_pre);
    add(Owner::LineStyles, &f.line_styles_pre);
    if let Some(Body::Style(st)) = body(&f.current_style) {
        add(Owner::CurrentStyle, &st.pre);
    }
    // A style of the list by its place in `Model::styles`, which holds
    // each identifier once.
    let mut seen = Vec::new();
    for p in &f.styles {
        let Some(Body::Style(st)) = body(p) else {
            continue;
        };
        let guid = hex(&st.guid);
        if let Some(i) = styles.iter().position(|s| s.guid == guid) {
            if !seen.contains(&i) {
                seen.push(i);
                add(Owner::Style(i), &st.pre);
            }
        }
    }
    add(Owner::Axes, &f.axes.pre);
    add(Owner::ShadowInfo, &f.shadow.pre);
    for (i, p) in f.line_styles.iter().enumerate() {
        if let Some(Body::LineStyle(l)) = body(p) {
            add(Owner::LineStyle(i), &l.pre);
        }
    }
    for (i, p) in f.scenes.iter().enumerate() {
        if let Some(Body::Scene(sc)) = body(p) {
            add(Owner::Scene(i), &sc.pre);
            add(Owner::SceneRendering(i), &sc.rendering.pre);
            add(Owner::SceneShadowInfo(i), &sc.shadow.pre);
            add(Owner::SceneAxes(i), &sc.axes.pre);
        }
    }
    // The layer folders in pre-order, as `features` lists them.
    fn folders(
        ids: &[Ptr],
        records: &HashMap<u32, Body>,
        path: &mut Vec<u32>,
        next: &mut usize,
        add: &mut dyn FnMut(Owner, &Pre),
    ) {
        for p in ids {
            let Some(id) = p.id() else { continue };
            let Some(Body::LayerGroup(g)) = records.get(&id) else {
                continue;
            };
            if path.contains(&id) || path.len() >= crate::features::FOLDER_DEPTH {
                continue;
            }
            add(Owner::LayerFolder(*next), &g.pre);
            *next += 1;
            path.push(id);
            folders(&g.children, records, path, next, add);
            path.pop();
        }
    }
    folders(
        &f.space.groups.2,
        records,
        &mut Vec::new(),
        &mut 0,
        &mut add,
    );
    out
}

fn dictionaries_of(container: u32, records: &HashMap<u32, Body>) -> Vec<crate::Dictionary> {
    let Some(Body::AttrContainer(c)) = records.get(&container) else {
        return Vec::new();
    };
    c.dictionaries
        .iter()
        .filter_map(|p| match records.get(&p.id()?)? {
            Body::AttrNamed(d) => Some(crate::Dictionary {
                name: d.name.text(),
                entries: d
                    .entries
                    .iter()
                    .map(|e| (e.key.text(), value(&e.value)))
                    .collect(),
                flag: d.tail,
            }),
            _ => None,
        })
        .collect()
}

/// The image an object pointer names, whatever its format (§8.2).
fn image_bytes(p: &Ptr, records: &HashMap<u32, Body>) -> Option<Vec<u8>> {
    match records.get(&p.id()?)? {
        Body::Dib(d) if !d.data.0.is_empty() => Some(d.data.0.clone()),
        _ => None,
    }
}

/// The opacity a material shows (§8.1): one minus the stored
/// transparency when the flag says it applies, opaque otherwise.
fn opacity(transparency: f64, flag: u8) -> f64 {
    if flag == 1 {
        ((1.0 - transparency) * 1e4).round() / 1e4
    } else {
        1.0
    }
}

/// Every material, in the order the file holds them, and the slot of
/// each.
fn materials(ids: &[u32], records: &HashMap<u32, Body>) -> (Vec<Material>, Vec<(u16, usize)>) {
    let mut out = Vec::new();
    let mut links = Vec::new();
    for id in ids {
        let Some(Body::Material(m)) = records.get(id) else {
            continue;
        };
        if let Ok(slot) = u16::try_from(*id) {
            links.push((slot, out.len()));
        }
        let name = m.name.text();
        out.push(match &m.texture {
            None => Material::Solid {
                name,
                rgba: m.rgba,
                opacity: opacity(m.opacity, m.use_opacity),
                colorize_type: m.colorize,
            },
            Some(t) => {
                let image = t.image.id();
                // An image written with the material is inside it, and
                // so after it; a shared one was written before.
                let own = image.is_some_and(|i| i > *id);
                let quality = image.and_then(|i| match records.get(&i) {
                    Some(Body::Dib(d)) if own && d.subtype == 1 && d.quality <= 100 => {
                        Some(d.quality)
                    }
                    _ => None,
                });
                let file_name = t.file_name.text();
                Material::Textured {
                    name,
                    texture: (!file_name.is_empty()).then_some(file_name),
                    applied_size_in: Some((t.size_in[0], t.size_in[1])),
                    jpeg_quality: quality,
                    image_bytes: match &t.embedded {
                        Some(d) => Some(d.data.0.clone()).filter(|b| !b.is_empty()),
                        None => image_bytes(&t.image, records),
                    },
                    avg_rgba: Some(t.color),
                    colorized: m.kind == 2,
                    texture_avg_rgba: Some(m.rgba),
                    colorize_type: m.colorize,
                    opacity: opacity(m.opacity, m.use_opacity),
                }
            }
        });
    }
    (out, links)
}

fn attribute(e: &Entry) -> Option<Attribute> {
    let value = match &e.value {
        AttrVal::I32(v) if v.unsigned_abs() < 1_000_000 => AttrValue::Int(*v),
        AttrVal::F64(v) if !v.is_nan() && v.abs() < 1e6 => AttrValue::F64(*v),
        AttrVal::Bool(b) if *b <= 1 => AttrValue::Bool(*b == 1),
        _ => return None,
    };
    Some(Attribute {
        key: e.key.text(),
        value,
    })
}

/// The numbers and the switches of the option sets and of every
/// attribute dictionary, in the order the file holds them.
fn attributes(f: &File17, ids: &[u32], records: &HashMap<u32, Body>) -> Vec<Attribute> {
    let mut out: Vec<Attribute> = f
        .option_sets
        .iter()
        .flat_map(|s| s.entries.iter())
        .filter_map(attribute)
        .collect();
    for id in ids {
        if let Some(Body::AttrNamed(d)) = records.get(id) {
            out.extend(d.entries.iter().filter_map(attribute));
        }
    }
    out
}

fn images(ids: &[u32], records: &HashMap<u32, Body>) -> Vec<Image> {
    ids.iter()
        .filter_map(|id| match records.get(id)? {
            Body::Dib(d) if d.data.0.len() >= 8 => Some(Image {
                kind: crate::xml26::image_ext(&d.data.0).to_string(),
                bytes: d.data.0.len(),
            }),
            _ => None,
        })
        .collect()
}

/// A camera. A release before 4 holds the unit view direction in place
/// of the target (§14, [`crate::Release::camera_is_object`]): the target
/// reads as the point one inch along it from the eye.
fn camera(c: &Camera17, release: crate::Release) -> Option<Camera> {
    let mut d = c.body.to_vec();
    if !release.camera_is_object() {
        let f = |d: &[u8], o: usize| f64::from_le_bytes(d[o..o + 8].try_into().unwrap());
        for k in 0..3 {
            let t = f(&d, 8 * k) + f(&d, 24 + 8 * k);
            d[24 + 8 * k..32 + 8 * k].copy_from_slice(&t.to_le_bytes());
        }
    }
    d.extend_from_slice(&c.tail);
    Camera::from_2017(&d, 0, c.body.len(), c.description.text())
}

fn camera_of(p: &Ptr, records: &HashMap<u32, Body>, release: crate::Release) -> Option<Camera> {
    match records.get(&p.id()?)? {
        Body::Camera(c) => camera(c, release),
        _ => None,
    }
}

/// The display settings of a block of `release`. The fields a release's
/// block has no byte for (§14, [`crate::Release::rendering_parts`])
/// read as the default document's.
fn rendering(r: &Rendering17, release: crate::Release) -> Option<(RenderingOptions, bool)> {
    let block = release.rendering_in(&r.fields.0, &crate::build17::RENDERING);
    let shown = block.get(27).is_some_and(|&b| b != 0);
    let mut ro = RenderingOptions::from_2017(&block, 0)?;
    // What releases after 2017 add to the block (§14.5): from 2018 the
    // section fill colour and switch, from 2020 a hidden-objects switch
    // apart from the hidden-geometry one. A block that holds the bytes a
    // new model starts with leaves the settings unset.
    if release >= crate::Release::R2018 {
        let colour: [u8; 4] = block[206..210].try_into().ok()?;
        let fill = block[210] != 0;
        if colour != [0xff, 0, 0, 0] || fill {
            ro.section_fill_color = Some(colour);
            ro.section_fill = Some(fill);
        }
    }
    if release >= crate::Release::R2019 {
        ro.dashes = Some(block[211] != 0);
    }
    if release >= crate::Release::R2020 {
        ro.hidden_objects = block[212] != 0;
    }
    Some((ro, shown))
}

fn shadow(s: &Shadow17, displayed: bool) -> ShadowInfo {
    let b = &s.tail;
    ShadowInfo {
        time: s.time,
        city: s.city.text(),
        country: s.country.text(),
        longitude: s.longitude,
        latitude: s.latitude,
        tz_offset_h: s.tz_offset,
        displayed,
        on_faces: b[1] != 0,
        on_ground: b[2] != 0,
        from_edges: b[3] != 0,
        light: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        dark: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
        use_sun_for_shading: b[12] != 0,
        daylight_saving: s.a != 0,
        north: s.north,
        north_displayed: b[0] != 0,
    }
}

fn axes(a: &Axes17) -> Axes {
    Axes {
        origin_m: a.origin.map(|x| x / INCH),
        x: a.x,
        y: a.y,
        z: a.z,
    }
}

fn watermark(w: &Watermark17, background: bool, records: &HashMap<u32, Body>) -> Watermark {
    // The record keeps the aspect-ratio byte set while the control is
    // hidden; it means something for a stretched watermark only (§10.8).
    let stretched = w.stretched != 0;
    Watermark {
        name: w.name.text(),
        source_file: w.path.text(),
        background,
        tiled: w.tiled != 0,
        stretched,
        keep_aspect_ratio: stretched && w.keep_aspect != 0,
        position: w.position,
        mask: w.mask != 0,
        blend: w.blend,
        scale: w.scale,
        image_bytes: image_bytes(&w.image, records),
        time: w.time,
    }
}

/// A style's settings and its watermarks (§10.8).
fn style(
    s: &Style17,
    records: &HashMap<u32, Body>,
) -> (
    RenderingOptions,
    Vec<Watermark>,
    Option<Strokes>,
    Vec<crate::StyleItem>,
) {
    let mut settings = RenderingOptions::default();
    let mut watermarks = Vec::new();
    let mut strokes = None;
    let mut extra = Vec::new();
    for item in &s.items {
        if let Some(st) = item.strokes.as_deref() {
            strokes = Some(Strokes {
                dropout_length: st.numbers[0],
                fade_factor: st.numbers[1],
                width: st.a,
                halo: st.b != 0,
                halo_length: st.c,
                variants: st.variants,
                lengths: st.lengths.clone(),
                images: st
                    .images
                    .iter()
                    .filter_map(|p| image_bytes(p, records))
                    .collect(),
            });
            continue;
        }
        if item.id == ITEM_WATERMARKS {
            // Backgrounds, a null for the model, then overlays.
            let mut background = true;
            for v in &item.values {
                let StyleVal::Ptr(p) = v else { continue };
                match p.id().and_then(|id| records.get(&id)) {
                    Some(Body::Watermark(w)) => watermarks.push(watermark(w, background, records)),
                    _ => background = false,
                }
            }
            continue;
        }
        // An item the rendering options do not name, kept as it is.
        if !RO_FIELDS.iter().any(|f| f.item == Some(item.id)) {
            if let [v] = item.values.as_slice() {
                let value = match v {
                    StyleVal::U8(x) => Some(crate::StyleValue::Bool(*x != 0)),
                    StyleVal::U32(x) => Some(crate::StyleValue::Int(*x as i32)),
                    StyleVal::Rgba(x) => Some(crate::StyleValue::Colour(*x)),
                    StyleVal::F64(x) => Some(crate::StyleValue::Double(*x)),
                    StyleVal::Ptr(_) => None,
                };
                if let Some(value) = value {
                    extra.push(crate::StyleItem { id: item.id, value });
                }
            }
            continue;
        }
        for v in &item.values {
            let (kind, raw): (RoKind, Vec<u8>) = match v {
                StyleVal::U8(x) => (RoKind::Bool, vec![*x]),
                StyleVal::U32(x) => (RoKind::U32, x.to_le_bytes().to_vec()),
                StyleVal::Rgba(x) => (RoKind::Rgba, x.to_vec()),
                StyleVal::F64(x) => (RoKind::F64, x.to_le_bytes().to_vec()),
                StyleVal::Ptr(_) => continue,
            };
            // A colour is a colour whatever type its value is stored as.
            let colour = RO_FIELDS
                .iter()
                .find(|f| f.item == Some(item.id))
                .is_some_and(|f| f.kind == RoKind::Rgba);
            let value = match (colour, <[u8; 4]>::try_from(raw.as_slice())) {
                (true, Ok(rgba)) => Some(RoValue::Rgba(rgba)),
                (true, Err(_)) => None,
                (false, _) => RoValue::decode(kind, &raw),
            };
            if let Some(value) = value {
                settings.set_item(item.id, value);
            }
        }
    }
    (settings, watermarks, strokes, extra)
}

/// A scene's matched photo (§10.10).
fn match_photo(p: &Ptr, records: &HashMap<u32, Body>) -> Option<MatchPhoto> {
    match records.get(&p.id()?)? {
        Body::BackgroundImage(b) => Some(MatchPhoto {
            path: b.path.text(),
            image: image_bytes(&b.image, records).unwrap_or_default(),
            width: b.width,
            height: b.height,
            time: b.time,
            scale: b.scale,
            points: b.points.clone(),
        }),
        _ => None,
    }
}

fn text16(s: &S16) -> String {
    s.text()
}

fn settings(
    f: &File17,
    ids: &[u32],
    records: &HashMap<u32, Body>,
    walk: &Continuous,
) -> Settings17 {
    let mut s = Settings17::default();
    let mut displayed = false;
    s.camera = camera_of(&f.camera, records, f.release);
    if let Some((ro, shown)) = rendering(&f.rendering, f.release) {
        displayed = shown;
        s.rendering = Some(ro);
    }
    s.shadows = Some(shadow(&f.shadow, displayed));
    // A release without the byte (§14, 6 → 7) leaves the setting unknown.
    s.anti_aliased_textures = f
        .release
        .has_view_watermarks()
        .then_some(f.anti_aliased_textures != 0);
    s.axes = Some(axes(&f.axes));

    // The saved styles are the style objects of the file short of the
    // one that holds the current settings, each listed once by its
    // identifier.
    let current = f.current_style.id();
    let mut slot_of_style: Vec<u32> = Vec::new();
    for id in ids {
        let Some(Body::Style(st)) = records.get(id) else {
            continue;
        };
        if Some(*id) == current {
            continue;
        }
        let guid = hex(&st.guid);
        if s.styles.iter().any(|x| x.guid == guid) {
            continue;
        }
        let (settings, watermarks, strokes, items) = style(st, records);
        slot_of_style.push(*id);
        s.styles.push(Style {
            name: text16(&st.name),
            description: text16(&st.description),
            guid,
            settings,
            watermarks,
            strokes,
            items,
        });
    }
    let style_index = |p: &Ptr| -> Option<usize> {
        let id = p.id()?;
        slot_of_style.iter().position(|s| *s == id).or_else(|| {
            // A style listed once stands for every object of its
            // identifier.
            let Some(Body::Style(st)) = records.get(&id) else {
                return None;
            };
            let guid = hex(&st.guid);
            s.styles.iter().position(|x| x.guid == guid)
        })
    };
    if let Some(Body::Style(cur)) = current.and_then(|id| records.get(&id)) {
        let guid = hex(&cur.guid);
        s.active_style = s
            .styles
            .iter()
            .position(|x| x.guid == guid)
            .or_else(|| style_index(&f.active_style));
        let (cur_settings, cur_watermarks, _, _) = style(cur, records);
        s.watermarks = cur_watermarks;
        // Settings the display record has no byte for (section fill) are
        // the current style's, when a later SketchUp saved them there.
        if let Some(ro) = s.rendering.as_mut() {
            for (f, (_, v)) in RO_FIELDS.iter().zip(cur_settings.values()) {
                if let (None, Some(item), Some(v)) = (f.off17, f.item, v) {
                    ro.set_item(item, v);
                }
            }
        }
    } else {
        s.active_style = style_index(&f.active_style);
    }

    for p in &f.scenes {
        let Some(Body::Scene(sc)) = p.id().and_then(|id| records.get(&id)) else {
            continue;
        };
        let flags = sc.flags;
        let has = |bit: u32| flags & bit != 0;
        let view = has(2)
            .then(|| rendering(&sc.rendering, f.release))
            .flatten();
        let shown = view.as_ref().map(|(_, shown)| *shown);
        let slots = |v: &[Ptr]| -> Vec<usize> {
            v.iter()
                .filter_map(|p| p.id())
                .map(|id| id as usize)
                .collect()
        };
        let pids = |v: &[Ptr]| -> Vec<u32> {
            slots(v)
                .iter()
                .filter_map(|&slot| match walk.map.get(slot)? {
                    Slot::Object(e) => e.pid(),
                    _ => None,
                })
                .collect()
        };
        s.scenes.push(Scene {
            name: text16(&sc.name),
            description: text16(&sc.description),
            saved: SceneProperties(flags),
            camera: if has(1) {
                camera_of(&sc.camera, records, f.release)
            } else {
                None
            },
            rendering: view.map(|(ro, _)| ro),
            style: if has(2) {
                style_index(&sc.style).map(|i| s.styles[i].name.clone())
            } else {
                None
            },
            shadows: has(4).then(|| shadow(&sc.shadow, shown.unwrap_or(displayed))),
            axes: has(8).then(|| axes(&sc.axes)),
            hidden_entities: if has(16) {
                pids(&sc.hidden_entities)
            } else {
                Vec::new()
            },
            active_section_planes: if has(64) {
                pids(&sc.section_planes)
            } else {
                Vec::new()
            },
            hidden_layers: if has(32) {
                slots(&sc.hidden_layers)
                    .iter()
                    .filter_map(|s| walk.layer_slots.iter().position(|q| q == s))
                    .collect()
            } else {
                Vec::new()
            },
            in_animation: sc.in_animation != 0,
            thumbnail: (sc.thumb_flags == [1, 1] && !sc.thumb.0.is_empty())
                .then(|| sc.thumb.0.clone()),
            match_photo: match_photo(&sc.photo, records),
        });
    }

    s.selected_scene = f
        .selected_scene
        .id()
        .and_then(|id| f.scenes.iter().position(|p| p.id() == Some(id)));

    // Fonts, texts and dimensions are entities of the map.
    crate::settings17::annotations(&walk.map, &mut s);
    let font = |p: &Ptr| -> Option<usize> { s.font_index.get(&(p.id()? as usize)).copied() };
    let u32at = |b: &[u8], o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let rgba = |b: &[u8], o: usize| [b[o], b[o + 1], b[o + 2], b[o + 3]];
    let (dd, td) = (&f.dimension_defaults, &f.text_defaults);
    s.dimension_defaults = Some(DimensionDefaults {
        font: font(&f.dimension_font),
        aligned: dd[0] != 0,
        highlight_non_associated: dd[22] != 0,
        radius_prefix: dd[27] != 0,
        hide_foreshortened: dd[28] != 0,
        hide_small: dd[37] != 0,
        arrow: u32at(dd, 14),
        text_position: u32at(dd, 54),
        color: rgba(dd, 46),
    });
    s.text_defaults = Some(TextDefaults {
        font: font(&f.text_font),
        screen_font: font(&f.screen_font),
        arrow: u32at(td, 9),
        leader: u32at(td, 0),
        leader_text_color: rgba(td, 14),
        // A release before 6 holds no screen text colour: the default.
        screen_text_color: if f.release.has_screen_text() {
            rgba(td, 18)
        } else {
            [63, 63, 63, 255]
        },
    });
    s
}

/// The layer line styles and layer folders a release 2019 or 2020 file
/// holds, in the terms of `Model::features`: layers by their index in
/// the layer list, line styles by their place in the file's style list.
fn features(f: &File17, records: &HashMap<u32, Body>) -> crate::Features {
    use crate::LayerFolder;
    let mut out = crate::Features::default();
    let layer_ids: Vec<u32> = f.space.layers.iter().filter_map(|p| p.id()).collect();
    let styles: Vec<u32> = f.line_styles.iter().filter_map(|p| p.id()).collect();
    for (i, id) in layer_ids.iter().enumerate() {
        let Some(Body::Layer(l)) = records.get(id) else {
            continue;
        };
        if l.tail != 0 {
            out.layer_page_behaviour.push((i, l.tail));
        }
        if let Some(s) = l
            .style
            .id()
            .and_then(|id| styles.iter().position(|s| *s == id))
        {
            out.layer_line_styles.push((i, s));
        }
    }
    for (k, p) in f.scenes.iter().enumerate() {
        if !f.release.scene_has_pair() {
            break;
        }
        let Some(Body::Scene(sc)) = p.id().and_then(|id| records.get(&id)) else {
            continue;
        };
        let (t, d) = (
            (sc.f[0] >= 0.0).then_some(sc.f[0]),
            (sc.f[1] >= 0.0).then_some(sc.f[1]),
        );
        if t.is_some() || d.is_some() {
            out.scene_pages.push((
                k,
                crate::PageExtras {
                    transition_s: t,
                    delay_s: d,
                    ..Default::default()
                },
            ));
        }
    }
    /// Folders nest a few levels in practice; a file that points a folder
    /// at itself or at an ancestor, or fans out past what any file holds,
    /// reads as far as `budget` allows.
    fn folder(
        id: u32,
        layer_ids: &[u32],
        records: &HashMap<u32, Body>,
        path: &mut Vec<u32>,
        budget: &mut usize,
    ) -> Option<LayerFolder> {
        let Some(Body::LayerGroup(g)) = records.get(&id) else {
            return None;
        };
        if path.contains(&id) || *budget == 0 {
            return None;
        }
        *budget -= 1;
        path.push(id);
        let out = Some(LayerFolder {
            name: g.name.text(),
            hidden: g.hidden != 0,
            hidden_on_new_pages: false,
            layers: g
                .layers
                .iter()
                .filter_map(|l| {
                    l.id()
                        .and_then(|id| layer_ids.iter().position(|x| *x == id))
                })
                .collect(),
            folders: g
                .children
                .iter()
                .filter_map(|c| {
                    c.id()
                        .and_then(|id| folder(id, layer_ids, records, path, budget))
                })
                .collect(),
        });
        path.pop();
        out
    }
    let mut budget = 10_000;
    out.layer_folders = f
        .space
        .groups
        .2
        .iter()
        .filter_map(|p| {
            p.id()
                .and_then(|id| folder(id, &layer_ids, records, &mut Vec::new(), &mut budget))
        })
        .collect();
    out
}

#[cfg(test)]
mod tests {
    use crate::{header, Diagnostic, Model};
    use std::path::PathBuf;

    /// The 2017 files to compare the readers on: the corpus, and the
    /// local directories `SKPWRIGHT_WRITE_MODELS` names (searched two
    /// levels deep).
    fn files() -> Vec<PathBuf> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let mut dirs = vec![root.join("2017"), root.join("third-party")];
        if let Some(extra) = std::env::var_os("SKPWRIGHT_WRITE_MODELS") {
            for d in std::env::split_paths(&extra) {
                if let Ok(rd) = std::fs::read_dir(&d) {
                    dirs.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
                }
                dirs.push(d);
            }
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
        out
    }

    /// The parts of a model, each as text.
    fn parts(m: &Model) -> Vec<(&'static str, String)> {
        let diagnostics: Vec<&Diagnostic> = m
            .diagnostics
            .iter()
            .filter(|d| !matches!(d, Diagnostic::PadSlotBound { .. } | Diagnostic::WholeFile))
            .collect();
        vec![
            ("version", format!("{:?}", m.version)),
            ("model_guid", format!("{:?}", m.model_guid)),
            ("definitions", format!("{:#?}", m.definitions)),
            ("instances", format!("{:#?}", m.instances)),
            ("geometry", format!("{:#?}", m.geometry)),
            ("materials", {
                // One line a material, the image by its length.
                let mut m = m.materials.clone();
                let mut lens = Vec::new();
                for x in m.iter_mut() {
                    if let crate::Material::Textured { image_bytes, .. } = x {
                        lens.push(image_bytes.take().map(|b| b.len()));
                    } else {
                        lens.push(None);
                    }
                }
                m.iter()
                    .zip(lens)
                    .map(|(x, l)| format!("{x:?} image {l:?}\n"))
                    .collect()
            }),
            ("layers", format!("{:#?}", m.layers)),
            ("scenes", format!("{:#?}", m.scenes)),
            ("guides", format!("{:#?}", m.guides)),
            ("attributes", format!("{:#?}", m.attributes)),
            ("images", format!("{:#?}", m.images)),
            ("camera", format!("{:#?}", m.camera)),
            ("rendering", format!("{:#?}", m.rendering)),
            ("shadows", format!("{:#?}", m.shadows)),
            ("units", format!("{:#?}", m.units)),
            ("styles", short(format!("{:#?}", m.styles))),
            ("active_style", format!("{:?}", m.active_style)),
            ("watermarks", short(format!("{:#?}", m.watermarks))),
            ("fonts", format!("{:#?}", m.fonts)),
            ("texts", format!("{:#?}", m.texts)),
            ("dimensions", format!("{:#?}", m.dimensions)),
            ("axes", format!("{:?}", m.axes)),
            ("text_defaults", format!("{:?}", m.text_defaults)),
            ("dimension_defaults", format!("{:?}", m.dimension_defaults)),
            ("anti_aliased", format!("{:?}", m.anti_aliased_textures)),
            ("animation", format!("{:?}", m.animation)),
            ("geo_located", format!("{:?}", m.geo_located)),
            ("definition_links", format!("{:?}", m.definition_links)),
            ("layer_links", format!("{:?}", m.layer_links)),
            ("material_links", format!("{:?}", m.material_links)),
            ("diagnostics", format!("{diagnostics:#?}")),
        ]
    }

    /// A debug text with every byte of an image left out.
    fn short(text: String) -> String {
        text.lines()
            .filter(|l| {
                let t = l.trim().trim_end_matches(',');
                t.is_empty() || t.parse::<u8>().is_err()
            })
            .map(|l| format!("{l}\n"))
            .collect()
    }

    /// Where two texts first differ, with the lines around.
    fn first_difference(a: &str, b: &str) -> String {
        let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
        let at = la
            .iter()
            .zip(&lb)
            .position(|(x, y)| x != y)
            .unwrap_or(la.len().min(lb.len()));
        let show = |l: &[&str]| -> String {
            l[at.saturating_sub(4)..(at + 8).min(l.len())]
                .iter()
                .map(|x| format!("      {x}\n"))
                .collect()
        };
        format!(
            "    line {at} of {}/{}\n    walk:\n{}    grammar:\n{}",
            la.len(),
            lb.len(),
            show(&la),
            show(&lb)
        )
    }

    /// On every corpus file the grammar reads what the walk reads, and
    /// more: the fonts of the annotation defaults, which the walk ends
    /// before; the end of a definition whose thumbnail is a JPEG, which
    /// the walk puts before the image's quality (§8.2); the previews,
    /// the dictionaries and the active layer, which the walk never read.
    #[test]
    fn the_grammar_reads_what_the_walk_reads() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        let mut read = 0;
        for dir in ["2017", "third-party"] {
            for e in std::fs::read_dir(root.join(dir)).unwrap().flatten() {
                let path = e.path();
                if path.extension().and_then(|e| e.to_str()) != Some("skp") {
                    continue;
                }
                let d = std::fs::read(&path).unwrap();
                let Some(hdr) = header::parse_header(&d) else {
                    continue;
                };
                if !hdr.version.starts_with("{17.") {
                    continue;
                }
                let name = path.display();
                let mut g =
                    Model::parse_grammar(&d, &hdr).unwrap_or_else(|e| panic!("{name}: {e}"));
                let w = Model::parse_walk(&d, hdr).unwrap();
                assert!(
                    g.definitions.iter().any(|d| d.thumbnail_camera.is_some())
                        || g.definitions.is_empty(),
                    "{name}: previews"
                );
                for def in &mut g.definitions {
                    def.thumbnail = None;
                    def.thumbnail_camera = None;
                }
                for s in &mut g.scenes {
                    s.thumbnail = None;
                }
                g.thumbnail = None;
                g.preview = None;
                g.dictionaries.clear();
                g.active_layer = None;
                g.current_material = None;
                // The walk reads no style items beyond the settings.
                for s in &mut g.styles {
                    s.items.clear();
                }
                for ((part, a), (_, b)) in parts(&w).into_iter().zip(parts(&g)) {
                    let (a, b) = match part {
                        "text_defaults" => continue,
                        "geometry" => {
                            let ends = |t: String| -> String {
                                t.lines()
                                    .filter(|l| !l.trim_start().starts_with("end: "))
                                    .map(|l| format!("{l}\n"))
                                    .collect()
                            };
                            (ends(a), ends(b))
                        }
                        _ => (a, b),
                    };
                    assert!(a == b, "{name}: {part}\n{}", first_difference(&a, &b));
                }
                let (a, b) = (w.text_defaults.unwrap(), g.text_defaults.unwrap());
                assert_eq!(
                    (a.arrow, a.leader_text_color, a.screen_text_color, a.leader),
                    (b.arrow, b.leader_text_color, b.screen_text_color, b.leader),
                    "{name}"
                );
                assert!(b.font.is_some() && b.screen_font.is_some(), "{name}");
                read += 1;
            }
        }
        assert!(read >= 70, "{read} files");
    }

    /// Survey: what a file loses when it is read into a model and the
    /// model is written as a 2017 file. Both files are held as records
    /// and every field is counted by the record it belongs to and its
    /// value; what the source holds more of than the written file is
    /// listed by field, with an example value.
    #[test]
    #[ignore]
    fn survey_what_a_round_trip_loses() {
        use crate::file17::{describe, File17};
        fn fields(text: &str) -> std::collections::HashMap<String, i64> {
            let mut out: std::collections::HashMap<String, i64> = Default::default();
            // The records a line lies in, by their indentation.
            let mut open: Vec<(usize, String)> = Vec::new();
            for line in text.lines() {
                let indent = line.len() - line.trim_start().len();
                let t = line.trim().trim_end_matches(',');
                while open.last().is_some_and(|(i, _)| *i >= indent) {
                    open.pop();
                }
                if let Some(name) = t.strip_suffix(" {").or_else(|| t.strip_suffix("(")) {
                    let name = name.rsplit(": ").next().unwrap_or(name);
                    open.push((indent, name.to_string()));
                    continue;
                }
                let Some((field, value)) = t.split_once(": ") else {
                    continue;
                };
                if field == "id" || value.starts_with("Ref") || field == "pid" || field == "guid" {
                    continue;
                }
                let owner = open
                    .iter()
                    .rev()
                    .map(|(_, n)| n.as_str())
                    .find(|n| n.ends_with("17") || n.chars().next().is_some_and(char::is_uppercase))
                    .unwrap_or("File17");
                *out.entry(format!("{owner}.{field} = {value}")).or_default() += 1;
            }
            out
        }
        let only = std::env::var("SKPWRIGHT_READ17_PART").ok();
        let mut lost: std::collections::BTreeMap<String, (i64, String, String)> =
            Default::default();
        let mut read = 0;
        for path in files() {
            let Ok(d) = std::fs::read(&path) else {
                continue;
            };
            if d.len() > 30_000_000 {
                continue;
            }
            let Some(hdr) = header::parse_header(&d) else {
                continue;
            };
            if !hdr.version.starts_with("{17.") {
                continue;
            }
            let (Ok(source), Ok(model)) = (File17::read(&d), Model::parse(&d)) else {
                continue;
            };
            let Ok(built) = crate::build17::from_model_for(&model, crate::Release::R2017) else {
                continue;
            };
            read += 1;
            let (a, b) = (fields(&describe(&source)), fields(&describe(&built.file)));
            if let Some(part) = &only {
                // Both sides of one field, file by file.
                let side = |m: &std::collections::HashMap<String, i64>,
                            o: &std::collections::HashMap<String, i64>| {
                    let mut v: Vec<String> = m
                        .iter()
                        .filter(|(k, n)| {
                            k.starts_with(part.as_str()) && **n > o.get(*k).copied().unwrap_or(0)
                        })
                        .map(|(k, n)| {
                            format!(
                                "{}x {}",
                                n - o.get(k).copied().unwrap_or(0),
                                k[part.len()..].chars().take(90).collect::<String>()
                            )
                        })
                        .collect();
                    v.sort();
                    v.truncate(6);
                    v
                };
                let (x, y) = (side(&a, &b), side(&b, &a));
                if !x.is_empty() || !y.is_empty() {
                    eprintln!(
                        "{:?}\n  source {x:?}\n  written {y:?}",
                        path.file_name().unwrap()
                    );
                }
            }
            for (key, n) in a {
                let more = n - b.get(&key).copied().unwrap_or(0);
                if more > 0 {
                    let (field, value) = key.split_once(" = ").unwrap();
                    let e =
                        lost.entry(field.to_string())
                            .or_insert((0, String::new(), String::new()));
                    e.0 += more;
                    if e.1.is_empty() {
                        e.1 = value.chars().take(60).collect();
                        e.2 = path.file_name().unwrap().to_string_lossy().into_owned();
                    }
                }
            }
        }
        eprintln!("{read} files");
        for (field, (n, value, file)) in lost {
            eprintln!("{n:>9}  {field}  e.g. {value}  ({file})");
        }
    }

    /// Survey: which classes of object carry attribute dictionaries, and
    /// the dictionary names and value types they hold.
    #[test]
    #[ignore]
    fn survey_the_attributes() {
        use crate::file17::{AttrVal, Body, File17};
        let mut owners: std::collections::BTreeMap<String, usize> = Default::default();
        let mut names: std::collections::BTreeMap<(String, String), usize> = Default::default();
        let mut types: std::collections::BTreeMap<String, usize> = Default::default();
        let mut nested = 0;
        fn kind(v: &AttrVal, types: &mut std::collections::BTreeMap<String, usize>) {
            let k = match v {
                AttrVal::Nil => "nil",
                AttrVal::I32(_) => "i32",
                AttrVal::F32(_) => "f32",
                AttrVal::F64(_) => "f64",
                AttrVal::Bool(_) => "bool",
                AttrVal::Str(_) => "str",
                AttrVal::Array(items) => {
                    for i in items {
                        kind(i, types);
                    }
                    "array"
                }
                AttrVal::Raw8(_) => "raw8",
                AttrVal::Vec3(_) => "vec3",
                AttrVal::Time(_) => "time",
                AttrVal::Dir3(_) => "dir3",
            };
            *types.entry(k.to_string()).or_default() += 1;
        }
        for path in files() {
            let Ok(d) = std::fs::read(&path) else {
                continue;
            };
            if d.len() > 60_000_000 {
                continue;
            }
            let Ok((f, c)) = File17::collect(&d) else {
                continue;
            };
            let file_container = f.space.pre.attrs.id();
            for (owner, container) in &c.attributes {
                let class = match c.map.get(*owner as usize) {
                    Some(crate::carchive::Slot::Object(e)) => e.class_name().to_string(),
                    _ => "?".into(),
                };
                *owners.entry(class.clone()).or_default() += 1;
                let Some(Body::AttrContainer(_)) = c.records.get(container) else {
                    continue;
                };
                // The dictionaries of this container.
                for (id, b) in &c.records {
                    let Body::AttrNamed(dict) = b else { continue };
                    let _ = id;
                    let _ = dict;
                }
            }
            let _ = file_container;
            for b in c.records.values() {
                let Body::AttrNamed(dict) = b else { continue };
                if dict.pre.attrs.id().is_some() {
                    nested += 1;
                }
                let owner = "dictionary";
                *names
                    .entry((owner.to_string(), dict.name.text()))
                    .or_default() += 1;
                for e in &dict.entries {
                    kind(&e.value, &mut types);
                }
            }
        }
        eprintln!("owners: {owners:?}\ntypes: {types:?}\nnested dictionaries: {nested}");
        let mut v: Vec<_> = names.into_iter().collect();
        v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        for ((_, name), n) in v.iter().take(40) {
            eprintln!("{n:>7} {name:?}");
        }
    }

    /// Survey: the image subtypes of every file, with the first bytes of
    /// the images stored under each, and who holds such an image.
    #[test]
    #[ignore]
    fn survey_the_images() {
        use crate::file17::{Body, File17};
        let mut seen: std::collections::BTreeMap<(u32, String), (usize, String)> =
            Default::default();
        for path in files() {
            let Ok(d) = std::fs::read(&path) else {
                continue;
            };
            let Ok((_, c)) = File17::collect(&d) else {
                continue;
            };
            for (id, b) in &c.records {
                let Body::Dib(dib) = b else { continue };
                let magic: String = dib
                    .data
                    .0
                    .iter()
                    .take(4)
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let holder = c
                    .records
                    .iter()
                    .find_map(|(_, r)| match r {
                        Body::Material(m)
                            if m.texture.as_ref().and_then(|t| t.image.id()) == Some(*id) =>
                        {
                            Some(format!("material {:?}", m.name.text()))
                        }
                        Body::Watermark(w) if w.image.id() == Some(*id) => Some("watermark".into()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "other".into());
                let e = seen
                    .entry((dib.subtype, magic))
                    .or_insert((0, String::new()));
                e.0 += 1;
                if e.1.is_empty() || (e.1.starts_with("other") && !holder.starts_with("other")) {
                    e.1 = format!("{holder} in {:?}", path.file_name().unwrap());
                }
            }
        }
        for ((subtype, magic), (n, example)) in seen {
            eprintln!("subtype {subtype} magic {magic}: {n}, e.g. {example}");
        }
    }

    /// Survey: on every file both readers read, which parts of the model
    /// differ. Prints; `SKPWRIGHT_READ17_PART` names one part to show.
    #[test]
    #[ignore]
    fn survey_the_two_readers() {
        let only = std::env::var("SKPWRIGHT_READ17_PART").ok();
        let mut counts: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
        let (mut both, mut grammar_only, mut walk_only, mut neither) = (0, 0, 0, Vec::new());
        for path in files() {
            let Ok(d) = std::fs::read(&path) else {
                continue;
            };
            let Some(hdr) = header::parse_header(&d) else {
                continue;
            };
            if !hdr.version.starts_with("{17.") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let g = Model::parse_grammar(&d, &hdr).ok();
            let w = Model::parse_walk(&d, hdr).ok().filter(|m| {
                m.diagnostics
                    .iter()
                    .any(|d| matches!(d, Diagnostic::ContinuousWalk { .. }))
            });
            let (g, w) = match (g, w) {
                (Some(g), Some(w)) => (g, w),
                (Some(_), None) => {
                    grammar_only += 1;
                    continue;
                }
                (None, Some(_)) => {
                    walk_only += 1;
                    eprintln!("WALK ONLY {name}: {:?}", super::read(&d).err().map(|e| e.0));
                    continue;
                }
                (None, None) => {
                    neither.push(name);
                    continue;
                }
            };
            both += 1;
            for ((part, a), (_, b)) in parts(&w).into_iter().zip(parts(&g)) {
                if a != b {
                    let seen = counts.entry(part).or_default();
                    if only.as_deref() == Some(part) && seen.len() < 12 {
                        eprintln!("{part} differs in {name}\n{}", first_difference(&a, &b));
                    }
                    seen.push(name.clone());
                }
            }
        }
        eprintln!(
            "both {both}, grammar only {grammar_only}, walk only {walk_only}, neither {neither:?}"
        );
        for (part, names) in &counts {
            eprintln!(
                "{part}: {} files, e.g. {:?}",
                names.len(),
                &names[..names.len().min(4)]
            );
        }
    }
}
