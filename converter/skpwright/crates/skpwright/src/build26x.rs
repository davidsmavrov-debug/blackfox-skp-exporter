//! Writing the post-2017 features of a model (`crate::features`) into a
//! 2026 container: the hatch pattern list and its archive entries, and
//! the records that name a pattern. Called from `build26::from_model`;
//! a model with empty features goes through none of it.

use std::collections::HashSet;

use crate::build26::folder_name;
use crate::features::{base64, pattern_bytes};
use crate::model::Error;
use crate::tree26::{Body, Record};
use crate::xml26::esc;
use crate::{Crs, Document, HatchPattern, LayerFolder, Model, RenderHatch};

/// The check of `build26::validate`: every reference a feature makes
/// names something the model holds.
pub(crate) fn validate(model: &Model) -> Result<(), Error> {
    let f = &model.features;
    let patterns = f.hatch_patterns.len();
    let bad = |what: &str, at: usize| {
        Err(Error(format!(
            "{what} names hatch pattern {at}, but the model has {patterns}"
        )))
    };
    for h in &f.hatch_patterns {
        if let Some(t) = &h.texture {
            let p = t.path.trim_start_matches("./");
            if p.is_empty()
                || p.starts_with('/')
                || p.contains('\\')
                || p.split('/').any(|s| s == "..")
            {
                return Err(Error(format!(
                    "hatch pattern {:?} has the texture path {:?}, which leaves its folder",
                    h.name, t.path
                )));
            }
            if p == "hatchpattern.xml" {
                return Err(Error(format!(
                    "hatch pattern {:?} has the texture path {:?}, which is the entry of its definition",
                    h.name, t.path
                )));
            }
        }
    }
    for &(m, h) in &f.material_hatch {
        if m >= model.materials.len() {
            return Err(Error(format!("material hatch names material {m}")));
        }
        if h >= patterns {
            return bad("a material", h);
        }
    }
    let placed: usize = model.geometry.iter().map(|r| r.placed.len()).sum();
    for &(i, h) in &f.instance_hatch {
        if i >= placed {
            return Err(Error(format!("instance hatch names instance {i}")));
        }
        if h >= patterns {
            return bad("an instance", h);
        }
    }
    let layers = model.layers.len().max(1);
    let line_styles = LINE_STYLE_COUNT;
    for &(l, s) in &f.layer_line_styles {
        if l >= layers {
            return Err(Error(format!("a layer line style names layer {l}")));
        }
        if s >= line_styles {
            return Err(Error(format!(
                "layer {l} names line style {s}, but the file has {line_styles}"
            )));
        }
    }
    for &(l, _) in &f.layer_page_behaviour {
        if l >= layers {
            return Err(Error(format!("a layer page behaviour names layer {l}")));
        }
    }
    fn folders_ok(list: &[LayerFolder], layers: usize, depth: usize) -> Result<(), Error> {
        if crate::features::folders_too_deep(list, depth) {
            return Err(Error(format!(
                "layer folders are nested more than {} deep",
                crate::features::FOLDER_DEPTH
            )));
        }
        for fo in list {
            if let Some(l) = fo.layers.iter().find(|l| **l >= layers) {
                return Err(Error(format!("layer folder {:?} names layer {l}", fo.name)));
            }
            folders_ok(&fo.folders, layers, depth + 1)?;
        }
        Ok(())
    }
    folders_ok(&f.layer_folders, layers, 0)?;
    if let Some(c) = &f.crs {
        if let Some((t, _)) = c
            .unknown
            .iter()
            .find(|(t, _)| (0x7E91..=0x7E98).contains(t) || *t == 0x7E9C)
        {
            return Err(Error(format!(
                "the coordinate reference system holds the record {t:#06x} out of its place"
            )));
        }
    }
    for run in &model.geometry {
        for s in &run.sections {
            if let Some(h) = s.hatch.filter(|h| *h >= patterns) {
                return bad("a section plane", h);
            }
        }
    }
    validate_pbr_environments(model)?;
    validate_pages_misc(model)
}

/// The references of the pages' extras, the snaps and the procedures.
fn validate_pages_misc(model: &Model) -> Result<(), Error> {
    fn count(list: &[LayerFolder]) -> usize {
        list.iter().map(|f| 1 + count(&f.folders)).sum()
    }
    let f = &model.features;
    let folders = count(&f.layer_folders);
    let defs = model.definitions.len();
    let mut seen = HashSet::new();
    for (i, p) in &f.scene_pages {
        let Some(scene) = model.scenes.get(*i) else {
            return Err(Error(format!("a page's extras name page {i}")));
        };
        if !seen.insert(*i) {
            return Err(Error(format!("page {i} has more than one set of extras")));
        }
        if let Some(k) = p.hidden_layer_folders.iter().find(|k| **k >= folders) {
            return Err(Error(format!(
                "page {i} hides layer folder {k}, but the model has {folders}"
            )));
        }
        if !p.hidden_layer_folders.is_empty()
            && !scene.saved.has(crate::SceneProperties::VISIBLE_LAYERS)
        {
            return Err(Error(format!(
                "page {i} does not save its visible layers, so it lists no hidden layer folders"
            )));
        }
        for v in [p.transition_s, p.delay_s].into_iter().flatten() {
            if !v.is_finite() {
                return Err(Error(format!("page {i} has a time that is not finite")));
            }
        }
    }
    for s in &f.snaps {
        if let Some(d) = s.definition {
            if model
                .definitions
                .get(d)
                .is_none_or(|x| x.map_index.is_none())
            {
                return Err(Error(format!(
                    "a snap names definition {d}, which the model lacks or which has no map index"
                )));
            }
        }
    }
    let mut seen = HashSet::new();
    for p in &f.procedures {
        if p.definition >= defs {
            return Err(Error(format!(
                "a procedure names definition {}",
                p.definition
            )));
        }
        if !seen.insert(p.definition) {
            return Err(Error(format!(
                "definition {} has more than one procedure",
                p.definition
            )));
        }
        for r in &p.results {
            if r.definition >= defs || r.definition == p.definition {
                return Err(Error(format!(
                    "a result of the procedure of definition {} places definition {}",
                    p.definition, r.definition
                )));
            }
        }
        if !p.results.is_empty() && model.definitions[p.definition].map_index.is_none() {
            return Err(Error(format!(
                "definition {} has no map index to hold the result of its procedure",
                p.definition
            )));
        }
    }
    Ok(())
}

/// A path that stays inside the folder of the entry that names it.
fn inside(path: &str) -> bool {
    let p = path.trim_start_matches("./");
    !(p.is_empty() || p.starts_with('/') || p.contains('\\') || p.split('/').any(|s| s == ".."))
}

fn validate_pbr_environments(model: &Model) -> Result<(), Error> {
    let f = &model.features;
    let mut seen = HashSet::new();
    for (m, p) in &f.material_pbr {
        if *m >= model.materials.len() {
            return Err(Error(format!(
                "physically based properties name material {m}"
            )));
        }
        if !seen.insert(*m) {
            return Err(Error(format!(
                "material {m} has more than one set of physically based properties"
            )));
        }
        let own = match &model.materials[*m] {
            crate::Material::Textured {
                texture: Some(t), ..
            } => t
                .rsplit(['/', '\\'])
                .next()
                .filter(|b| !b.is_empty())
                .map(|b| b.replace([':', '*', '?', '"', '<', '>', '|'], "_")),
            _ => None,
        };
        let mut paths: Vec<(&str, &[u8])> = Vec::new();
        for t in [&p.metallic, &p.roughness, &p.normal, &p.occlusion]
            .into_iter()
            .flatten()
        {
            if !inside(&t.path) {
                return Err(Error(format!(
                    "material {m} has the texture path {:?}, which leaves its folder",
                    t.path
                )));
            }
            let rel = t.path.trim_start_matches("./");
            if rel == "material.xml" || (!t.png.is_empty() && own.as_deref() == Some(rel)) {
                return Err(Error(format!(
                    "material {m} has the texture path {:?}, which is another entry of its folder",
                    t.path
                )));
            }
            if let Some((_, other)) = paths.iter().find(|(r, _)| *r == rel) {
                if !t.png.is_empty() && !other.is_empty() && *other != t.png.as_slice() {
                    return Err(Error(format!(
                        "material {m} has two different images at the texture path {:?}",
                        t.path
                    )));
                }
            }
            paths.push((rel, &t.png));
        }
        for (k, _) in p.other_material_attrs.iter().chain(&p.other_pbr_attrs) {
            if !is_xml_name(k) {
                return Err(Error(format!(
                    "material {m} has the attribute name {k:?}, which is not an XML name"
                )));
            }
        }
        for t in [&p.metallic, &p.roughness, &p.normal, &p.occlusion]
            .into_iter()
            .flatten()
        {
            if let Some((k, _)) = t
                .other_attrs
                .iter()
                .find(|(k, _)| !is_xml_name(k) || matches!(k.as_str(), "id" | "path" | "file_name"))
            {
                return Err(Error(format!(
                    "material {m} has the texture attribute name {k:?}, which is not an XML name"
                )));
            }
        }
    }
    let envs = f.environments.len();
    for e in &f.environments {
        if !inside(&e.image.path) {
            return Err(Error(format!(
                "environment {:?} has the image path {:?}, which leaves its folder",
                e.name, e.image.path
            )));
        }
        if matches!(
            e.image.path.trim_start_matches("./"),
            "environment.xml" | "thumbnail.jpg"
        ) {
            return Err(Error(format!(
                "environment {:?} has the image path {:?}, which is another entry of its folder",
                e.name, e.image.path
            )));
        }
        if let Some((k, _)) = e.other_attrs.iter().find(|(k, _)| !is_xml_name(k)) {
            return Err(Error(format!(
                "environment {:?} has the attribute name {k:?}, which is not an XML name",
                e.name
            )));
        }
    }
    if f.current_environment.is_some_and(|c| c >= envs) {
        return Err(Error(format!(
            "the current environment is out of range, the model has {envs}"
        )));
    }
    let mut seen = HashSet::new();
    for (i, p) in &f.scene_environment {
        if *i >= model.scenes.len() {
            return Err(Error(format!("a page environment names page {i}")));
        }
        if !seen.insert(*i) {
            return Err(Error(format!(
                "page {i} has more than one environment setting"
            )));
        }
        if p.environment.is_some_and(|e| e >= envs) {
            return Err(Error(format!(
                "page {i} names an environment, but the model has {envs}"
            )));
        }
        if !p.saved && (p.environment.is_some() || p.flag_a || p.flag_b) {
            return Err(Error(format!(
                "page {i} does not save the environment property, so it holds no environment or flags"
            )));
        }
    }
    Ok(())
}

/// Whether `k` can be written as an attribute name.
fn is_xml_name(k: &str) -> bool {
    let mut c = k.chars();
    c.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && c.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

/// The images of a material's physically based slots, as entries beside
/// its `material.xml`.
pub(crate) fn set_pbr_images(p: &crate::PbrMaterial, folder: &str, doc: &mut Document) {
    for t in [&p.metallic, &p.roughness, &p.normal, &p.occlusion]
        .into_iter()
        .flatten()
    {
        if !t.png.is_empty() {
            let rel = t.path.trim_start_matches("./");
            doc.set_entry(&format!("materials/{folder}/{rel}"), t.png.clone());
        }
    }
}

/// The environment list and the current environment in the environment
/// section, the environments' archive entries, and the pages' environment
/// ids and flags in the scene records. Nothing happens for a model
/// without environments or page settings.
pub(crate) fn apply_environments(
    model: &Model,
    top: &mut Record,
    doc: &mut Document,
    fresh: &mut dyn FnMut() -> u64,
) {
    let f = &model.features;
    let mut env_ids = Vec::with_capacity(f.environments.len());
    if !f.environments.is_empty() {
        let mut taken = HashSet::new();
        let mut items = Vec::with_capacity(f.environments.len());
        for e in &f.environments {
            let id = fresh();
            env_ids.push(id);
            let folder = folder_name(&e.name, &mut taken);
            doc.set_entry(
                &crate::read26x::environment_entry(&folder),
                crate::xml26::environment_xml(e).into_bytes(),
            );
            if !e.hdr.is_empty() {
                let rel = e.image.path.trim_start_matches("./");
                doc.set_entry(&format!("environments/{folder}/{rel}"), e.hdr.clone());
            }
            let mut item = vec![
                Record::kids(0x05DC, vec![Record::narrow(0x05DE, id)]),
                Record::string(0x7B0D, &e.name),
            ];
            if let Some(jpg) = &e.thumbnail {
                let path = format!("environments/{folder}/thumbnail.jpg");
                doc.set_entry(&path, jpg.clone());
                item.push(Record::kids(0x7B0E, vec![Record::string(0x2134, &path)]));
            }
            items.push(Record::kids(0x7B0C, item));
        }
        if let Some(coll) = top.path_mut(&[0x0210, 0x7918]) {
            coll.set(Record::kids(0x7919, items));
            if let Some(&id) = f.current_environment.and_then(|c| env_ids.get(c)) {
                coll.set(Record::narrow(0x791A, id));
            }
        }
    }
    if f.scene_environment.is_empty() {
        return;
    }
    let Some(Body::Kids(scenes)) = top.path_mut(&[0x0207, 0x6D60, 0x6D61]).map(|r| &mut r.body)
    else {
        return;
    };
    let mut scenes: Vec<&mut Record> = scenes.iter_mut().filter(|r| r.tag == 0x7148).collect();
    for (i, p) in &f.scene_environment {
        let Some(scene) = scenes.get_mut(*i) else {
            continue;
        };
        let Some(kids) = scene.children_mut() else {
            continue;
        };
        if let Some(flags) = kids.iter_mut().find(|r| r.tag == 0x715B) {
            flags.set(Record::u8(0x7B0F, p.flag_a as u8));
            flags.set(Record::u8(0x7B10, p.flag_b as u8));
        }
        if let Some(&id) = p.environment.and_then(|e| env_ids.get(e)) {
            let at = kids
                .iter()
                .position(|r| r.tag == 0x715B)
                .unwrap_or(kids.len());
            kids.insert(at, Record::narrow(0x715A, id));
        }
    }
}

/// The layer folders, display names and times a page holds beyond the
/// typed fields (`Features::scene_pages`). `folder_ids` is the id of each
/// layer folder in pre-order.
pub(crate) fn apply_page_extras(model: &Model, top: &mut Record, folder_ids: &[u64]) {
    let f = &model.features;
    if f.scene_pages.is_empty() {
        return;
    }
    let Some(Body::Kids(scenes)) = top.path_mut(&[0x0207, 0x6D60, 0x6D61]).map(|r| &mut r.body)
    else {
        return;
    };
    let mut scenes: Vec<&mut Record> = scenes.iter_mut().filter(|r| r.tag == 0x7148).collect();
    for (i, p) in &f.scene_pages {
        let Some(scene) = scenes.get_mut(*i) else {
            continue;
        };
        if !p.hidden_layer_folders.is_empty() {
            let hidden: Vec<u64> = p
                .hidden_layer_folders
                .iter()
                .filter_map(|&k| folder_ids.get(k).copied())
                .collect();
            scene.set(Record::id_list(0x7159, &hidden));
        }
        if let Some(name) = &p.display_name {
            scene.set(Record::string(0x7153, name));
        }
        if let Some(v) = p.transition_s {
            scene.set(Record::f64(0x7154, v));
        }
        if let Some(v) = p.delay_s {
            scene.set(Record::f64(0x7155, v));
        }
    }
}

/// The procedure registry (`0x0215`): one `0x7d82` per kind.
pub(crate) fn procedure_registry(kinds: &[crate::ProcedureKind]) -> Record {
    Record::kids(
        0x0215,
        kinds
            .iter()
            .map(|k| {
                Record::kids(
                    0x7D82,
                    vec![
                        Record::string(0x7D83, &k.name),
                        Record::string(0x7D84, &k.description),
                    ],
                )
            })
            .collect(),
    )
}

/// The `hatchpattern.xml` of `h`: the one element with its attributes in
/// the order SketchUp writes them (`name`, the colour, `hasTexture`, the
/// scales, `rotationAngle`, `hasPat`, then `patName` and `patData` with
/// a pattern), and a `textureName` child with a texture.
pub(crate) fn pattern_xml(h: &HatchPattern) -> String {
    let mut a = format!(
        "name=\"{}\" colorRed=\"{}\" colorGreen=\"{}\" colorBlue=\"{}\" hasTexture=\"{}\" xScale=\"{}\" yScale=\"{}\" rotationAngle=\"{}\" hasPat=\"{}\"",
        esc(&h.name),
        h.color[0],
        h.color[1],
        h.color[2],
        h.texture.is_some() as u8,
        h.x_scale,
        h.y_scale,
        h.rotation,
        h.pat_name.is_some() as u8,
    );
    if let Some(p) = &h.pat_name {
        a.push_str(&format!(
            " patName=\"{}\" patData=\"{}\"",
            esc(p),
            base64(&pattern_bytes(&h.lines))
        ));
    }
    let element = match &h.texture {
        Some(t) => format!(
            "  <hp:hatchpattern {a}>\n    <textureName id=\"{}\" path=\"{}\" file_name=\"{}\"/>\n  </hp:hatchpattern>\n",
            t.id,
            esc(&t.path),
            esc(&t.file_name)
        ),
        None => format!("  <hp:hatchpattern {a}/>\n"),
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<hatchpatternDocument xmlns=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern\" xmlns:hp=\"http://sketchup.com/schemas/sketchup/1.0/hatchpattern\">\n{element}</hatchpatternDocument>\n"
    )
}

/// The top-level hatch list (`0x0211`) for the model's patterns, `ids`
/// being the id of each, and their archive entries: per pattern the
/// definition (and its texture image) in `hatchpatterns/<folder>/` and the
/// preview in `hatchpatterns/<folder>_1/`. `None` without patterns,
/// unless the list itself holds dictionaries (`n-attr-hatchpatterns`).
///
/// The definition is written at the zlib level of any other `.xml` entry
/// (flags `0x6`), not at the maximum level SketchUp uses for it: the
/// encoder reproduces levels 0 and 1 only, and every level is valid
/// DEFLATE. A pattern without a preview gets no thumbnail record.
pub(crate) fn hatch_list(model: &Model, ids: &[u64], doc: &mut Document) -> Option<Record> {
    let patterns = &model.features.hatch_patterns;
    if patterns.is_empty()
        && !model
            .dictionaries
            .iter()
            .any(|a| a.owner == crate::Owner::HatchPatterns)
    {
        return None;
    }
    let mut taken = HashSet::new();
    let mut items = Vec::with_capacity(patterns.len());
    for (h, &id) in patterns.iter().zip(ids) {
        let folder = folder_name(&h.name, &mut taken);
        let preview = folder_name(&format!("{folder}_1"), &mut taken);
        if let Some(t) = &h.texture {
            if !t.png.is_empty() {
                let rel = t.path.trim_start_matches("./");
                doc.set_entry(&format!("hatchpatterns/{folder}/{rel}"), t.png.clone());
            }
        }
        doc.set_entry(
            &crate::read26x::xml_entry(&folder),
            pattern_xml(h).into_bytes(),
        );
        let mut item = vec![
            Record::kids(0x05DC, vec![Record::narrow(0x05DE, id)]),
            Record::string(0x7D15, &h.name),
            Record::string(0x7D0B, &h.name),
        ];
        if let Some(jpg) = &h.thumbnail {
            let path = format!("hatchpatterns/{preview}/thumbnail.jpg");
            doc.set_entry(&path, jpg.clone());
            item.push(Record::kids(0x7D10, vec![Record::string(0x2134, &path)]));
        }
        items.push(Record::kids(0x7D0A, item));
    }
    Some(Record::kids(
        0x0211,
        vec![Record::kids(0x7D00, vec![Record::kids(0x7D01, items)])],
    ))
}

/// Insert the hatch list into the top-level record, after the line
/// styles (`0x0208`) and so before the materials, where SketchUp has it.
pub(crate) fn insert_hatch_list(top: &mut Record, list: Record) {
    if let Body::Kids(kids) = &mut top.body {
        let at = kids
            .iter()
            .position(|r| r.tag == 0x0208)
            .map_or(kids.len(), |p| p + 1);
        kids.insert(at, list);
    }
}

/// The `0x7388` record of the rendering options for `h`, placed after the
/// section fill colour (`0x7373`) in `options` (`0x733c`).
pub(crate) fn insert_render_hatch(options: &mut Record, h: &RenderHatch) {
    let mut fields = vec![
        Record::leaf(0x7D0C, h.color.to_vec()),
        Record::leaf(0x7D0D, h.texture_slots[0].clone()),
        Record::leaf(0x7D12, h.texture_slots[1].clone()),
        Record::f32(0x7D0E, h.x_scale),
        Record::f32(0x7D0F, h.y_scale),
        Record::f32(0x7D14, h.rotation),
    ];
    if !h.lines.is_empty() {
        fields.push(Record::leaf(0x7D11, pattern_bytes(&h.lines)));
    }
    fields.push(Record::string(0x7D13, &h.pat_name));
    let rec = Record::kids(0x7388, vec![Record::kids(0x7D0A, fields)]);
    if let Body::Kids(kids) = &mut options.body {
        let at = kids
            .iter()
            .position(|r| r.tag == 0x7373)
            .map_or(kids.len(), |p| p + 1);
        kids.insert(at, rec);
    }
}

/// The number of stock line styles a written file holds
/// (`build26::line_styles`).
pub(crate) const LINE_STYLE_COUNT: usize = 12;

/// The layer features, applied to the assembled `model.dat`: the line
/// style and page behaviour of the layer records, and the layer folders.
/// `layer_ids` holds the id of each written layer and `fresh` allocates a
/// new id. The line style records must already be in `top`.
pub(crate) fn apply_layer_features(
    model: &Model,
    top: &mut Record,
    layer_ids: &[u64],
    fresh: &mut dyn FnMut() -> u64,
) -> Vec<u64> {
    let f = &model.features;
    let mut folder_ids = Vec::new();
    if !f.layer_line_styles.is_empty() || !f.layer_page_behaviour.is_empty() {
        let styles: Vec<u64> = top
            .path_mut(&[0x0208, 0x4074, 0x4075])
            .and_then(|l| match &l.body {
                Body::Kids(k) => Some(k),
                _ => None,
            })
            .map(|k| {
                k.iter()
                    .filter(|r| r.tag == 0x4076)
                    .filter_map(|r| match &r.body {
                        Body::Kids(k) => k.iter().find(|r| r.tag == 0x05DC),
                        _ => None,
                    })
                    .filter_map(|e| match &e.body {
                        Body::Kids(k) => k.iter().find(|r| r.tag == 0x05DE)?.uint(),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if let Some(Body::Kids(layers)) =
            top.path_mut(&[0x01F8, 0x3A98, 0x3A99]).map(|r| &mut r.body)
        {
            let mut layers: Vec<&mut Record> =
                layers.iter_mut().filter(|r| r.tag == 0x3C8C).collect();
            for &(l, b) in &f.layer_page_behaviour {
                if let Some(r) = layers.get_mut(l) {
                    r.set(Record::u32(0x3C90, b));
                }
            }
            for &(l, s) in &f.layer_line_styles {
                if let (Some(r), Some(&id)) = (layers.get_mut(l), styles.get(s)) {
                    r.set(Record::narrow(0x3C91, id));
                }
            }
        }
    }
    if !f.layer_folders.is_empty() {
        let list = folder_records(&f.layer_folders, layer_ids, fresh, &mut folder_ids);
        if let Some(root) = top.path_mut(&[0x01F8, 0x3A98, 0x3A9B, 0x3E80]) {
            root.set(Record::kids(0x3E83, list));
        }
    }
    folder_ids
}

/// The folders' records; `ids` collects each folder's id in pre-order.
fn folder_records(
    folders: &[LayerFolder],
    layer_ids: &[u64],
    fresh: &mut dyn FnMut() -> u64,
    ids: &mut Vec<u64>,
) -> Vec<Record> {
    folders
        .iter()
        .map(|fo| {
            let id = fresh();
            ids.push(id);
            let members: Vec<u64> = fo
                .layers
                .iter()
                .filter_map(|&l| layer_ids.get(l).copied())
                .collect();
            Record::kids(
                0x3E80,
                vec![
                    Record::kids(0x05DC, vec![Record::narrow(0x05DE, id)]),
                    Record::string(0x3E81, &fo.name),
                    Record::kids(0x3E83, folder_records(&fo.folders, layer_ids, fresh, ids)),
                    Record::id_list(0x3E84, &members),
                    Record::u8(0x3E82, fo.hidden as u8),
                    Record::u8(0x3E85, fo.hidden_on_new_pages as u8),
                ],
            )
        })
        .collect()
}

/// The coordinate reference system section (`0x0218`): its record in the
/// order the file has the fields, the records of unknown tags among them
/// by tag.
pub(crate) fn crs_section(c: &Crs) -> Record {
    let mut fields: Vec<(u16, Vec<u8>)> = vec![
        (0x7E91, c.eastings.to_le_bytes().to_vec()),
        (0x7E92, c.northings.to_le_bytes().to_vec()),
        (0x7E93, c.height.to_le_bytes().to_vec()),
        (0x7E94, c.x_axis[0].to_le_bytes().to_vec()),
        (0x7E95, c.x_axis[1].to_le_bytes().to_vec()),
        (0x7E96, c.scale.to_le_bytes().to_vec()),
        (0x7E97, c.name.as_bytes().to_vec()),
        (0x7E98, c.description.as_bytes().to_vec()),
        (0x7E9C, c.map_zone.as_bytes().to_vec()),
    ];
    for (t, s) in [
        (0x7E99, &c.geodetic_datum),
        (0x7E9A, &c.vertical_datum),
        (0x7E9B, &c.map_projection),
        (0x7E9D, &c.map_unit),
        (0x7E9F, &c.tgl_id),
        (0x7EA0, &c.calibration_file_name),
    ] {
        if !s.is_empty() {
            fields.push((t, s.as_bytes().to_vec()));
        }
    }
    if let Some(m) = c.map_unit_metres {
        fields.push((0x7E9E, m.to_le_bytes().to_vec()));
    }
    for (t, b) in &c.unknown {
        fields.push((*t, b.clone()));
    }
    fields.sort_by_key(|(t, _)| *t);
    Record::kids(
        0x0218,
        vec![Record::kids(
            0x7E90,
            fields
                .into_iter()
                .map(|(t, b)| Record::leaf(t, b))
                .collect(),
        )],
    )
}

/// A copy of `model` without what revision 17 (§16.27) does not hold,
/// and a note for each kind of thing left out; `None` when the model
/// holds nothing of the kind. Every feature but the current-settings
/// style record is of revision 18 (`Feature::since`), so the copy keeps
/// that record (without its modified flag) and nothing else of
/// `Model::features`; section plane hatches, the default section fill
/// style and the dictionaries of hatch patterns and layer folders go with
/// them.
pub(crate) fn strip_to_r17(model: &Model) -> Option<(Model, Vec<String>)> {
    use crate::{Feature, Owner, Revision};
    let newer: Vec<Feature> = model
        .features_used()
        .into_iter()
        .filter(|f| f.since() > Revision::R17)
        .collect();
    let renderings = || {
        model
            .rendering
            .iter()
            .chain(model.scenes.iter().filter_map(|s| s.rendering.as_ref()))
    };
    // `Some(false)` is the value every SketchUp 2027 file holds; a
    // revision-17 file has no such record, so it goes without a note.
    let fill_style = renderings().any(|r| r.default_section_fill_style.is_some());
    if newer.is_empty() && !fill_style {
        return None;
    }
    fn folders(list: &[LayerFolder]) -> usize {
        list.iter().map(|f| 1 + folders(&f.folders)).sum()
    }
    let f = &model.features;
    let count = |x: Feature| -> usize {
        match x {
            Feature::HatchPatterns => f.hatch_patterns.len(),
            Feature::MaterialHatch => f.material_hatch.len(),
            Feature::InstanceHatch => f.instance_hatch.len(),
            Feature::SectionHatch => model
                .geometry
                .iter()
                .flat_map(|r| &r.sections)
                .filter(|s| s.hatch.is_some())
                .count(),
            Feature::LayerFolders => folders(&f.layer_folders),
            Feature::LayerLineStyles => f.layer_line_styles.len(),
            Feature::LayerPageBehaviour => f.layer_page_behaviour.len(),
            Feature::PbrMaterials => f.material_pbr.len(),
            Feature::Environments => f.environments.len().max(1),
            Feature::PageEnvironments => f.scene_environment.len(),
            Feature::PageExtras => f.scene_pages.len(),
            Feature::Snaps => f.snaps.len(),
            Feature::Procedures => f.procedures.len(),
            Feature::ProcedureKinds => f.procedure_kinds.len(),
            Feature::DefaultSectionFillStyle => renderings()
                .filter(|r| r.default_section_fill_style == Some(true))
                .count(),
            Feature::RenderHatch
            | Feature::ModifiedStyle
            | Feature::Crs
            | Feature::CurrentStyle => 1,
        }
    };
    let mut notes: Vec<String> = newer
        .iter()
        .map(|&x| {
            format!(
                "revision 17 does not hold {}; {} left out",
                x.describe(),
                count(x)
            )
        })
        .collect();
    let feature_owned = |o: Owner| matches!(o, Owner::HatchPattern(_) | Owner::LayerFolder(_));
    let dictionaries = model
        .dictionaries
        .iter()
        .filter(|a| feature_owned(a.owner))
        .count();
    if dictionaries > 0 {
        notes.push(format!(
            "revision 17 does not hold the dictionaries of hatch patterns and layer folders; {dictionaries} sets left out"
        ));
    }
    let mut m = model.clone();
    let current_style = m.features.current_style.take().map(|mut c| {
        c.modified = false;
        c
    });
    m.features = crate::Features {
        current_style,
        ..Default::default()
    };
    for s in m.geometry.iter_mut().flat_map(|r| r.sections.iter_mut()) {
        s.hatch = None;
    }
    for r in m
        .rendering
        .iter_mut()
        .chain(m.scenes.iter_mut().filter_map(|s| s.rendering.as_mut()))
    {
        r.default_section_fill_style = None;
    }
    m.dictionaries.retain(|a| !feature_owned(a.owner));
    Some((m, notes))
}
