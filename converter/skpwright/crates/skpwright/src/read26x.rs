//! Reading the post-2017 features of a 2026 container (`crate::features`):
//! the hatch pattern list and the places that name a pattern. Called from
//! `read26::parse_entries`; every child record this module knows is
//! listed, and one it does not know is reported, as in `read26`.

use std::collections::{HashMap, HashSet};

use crate::features::{
    pattern_lines, unbase64, Crs, Environment, EnvironmentImage, HatchPattern, HatchTexture,
    LayerFolder, PageEnvironment, PageExtras, PbrMaterial, PbrTexture, Procedure, ProcedureKind,
    ProcedureResult, RenderHatch, Snap,
};
use crate::read26::{entity_id, pool, report_unknown, section, text, uint, Node, Rec};
use crate::settings26::{ids, ids_checked};
use crate::zip::Entries;
use crate::{Diagnostic, Features, PlacedInstance};

/// The top-level hatch pattern list (`0x0211`), a list wrapper (`0x7d00`)
/// of an item list (`0x7d01`) of items (`0x7d0a`).
const HATCH_LIST: u16 = 0x0211;
const HATCH_WRAPPER: u16 = 0x7D00;
const HATCH_ITEMS: u16 = 0x7D01;
const HATCH_ITEM: u16 = 0x7D0A;
/// The item's name, twice (always the same text in the corpus; the roles
/// are candidates).
const HATCH_NAME: u16 = 0x7D15;
const HATCH_NAME_ALT: u16 = 0x7D0B;
/// The item's preview: a container holding the archive path (`0x2134`).
const HATCH_THUMBNAIL: u16 = 0x7D10;
const HATCH_THUMBNAIL_PATH: u16 = 0x2134;
const ENTITY: u16 = 0x05DC;

/// A material's hatch pattern id (child of `0x32c8`).
const MATERIAL_HATCH: u16 = 0x32CF;
/// A section plane's hatch pattern id (child of `0x445c`).
pub(crate) const SECTION_HATCH: u16 = 0x4460;
/// An instance's hatch pattern id (child of `0x1964`).
pub(crate) const INSTANCE_HATCH: u16 = 0x1969;

/// The rendering options' hatch settings: a container (`0x7388`) of one
/// record (`0x7d0a`) that mirrors the style document's item 7019.
const RENDER_HATCH: u16 = 0x7388;
const RO_COLOR: u16 = 0x7D0C;
const RO_SLOT_A: u16 = 0x7D0D;
const RO_SLOT_B: u16 = 0x7D12;
const RO_X_SCALE: u16 = 0x7D0E;
const RO_Y_SCALE: u16 = 0x7D0F;
const RO_ROTATION: u16 = 0x7D14;
const RO_PATTERN: u16 = 0x7D11;
const RO_PAT_NAME: u16 = 0x7D13;
/// The section fill colour of the rendering options.
const RO_SECTION_FILL: u16 = 0x7373;

/// The layer section (`0x01f8 > 0x3a98`): the layer list, and the folder
/// tree (`0x3a9b`) whose root record (`0x3e80`) has no id.
const LAYER_SECTION: u16 = 0x01F8;
const LAYER_LIST_WRAPPER: u16 = 0x3A98;
const LAYER_LIST: u16 = 0x3A99;
const LAYER_FOLDERS: u16 = 0x3A9B;
const LAYER: u16 = 0x3C8C;
/// A layer's page behaviour and line style (child of `0x3c8c`).
const LAYER_PAGE_BEHAVIOUR: u16 = 0x3C90;
const LAYER_LINE_STYLE: u16 = 0x3C91;
const FOLDER: u16 = 0x3E80;
const FOLDER_NAME: u16 = 0x3E81;
const FOLDER_HIDDEN: u16 = 0x3E82;
const FOLDER_FOLDERS: u16 = 0x3E83;
const FOLDER_LAYERS: u16 = 0x3E84;
const FOLDER_NEW_PAGES: u16 = 0x3E85;
use crate::features::FOLDER_DEPTH;

/// The line style list (`0x0208 > 0x4074 > 0x4075`) of `0x4076` items.
const LINE_STYLES: u16 = 0x0208;
const LINE_STYLE: u16 = 0x4076;

/// The coordinate reference system section (`0x0218`) and its record.
const CRS_SECTION: u16 = 0x0218;
const CRS: u16 = 0x7E90;
const CRS_NAME: u16 = 0x7E97;
const CRS_DESCRIPTION: u16 = 0x7E98;
const CRS_MAP_ZONE: u16 = 0x7E9C;

/// The archive entry of a pattern's definition, by the folder it is in.
pub(crate) fn xml_entry(folder: &str) -> String {
    format!("hatchpatterns/{folder}/hatchpattern.xml")
}

/// The hatch patterns of a file, and the id each is named by.
pub(crate) struct HatchTable {
    pub(crate) patterns: Vec<HatchPattern>,
    /// Persistent id -> index into `patterns`.
    pub(crate) index: HashMap<u32, usize>,
}

fn skipped(diags: &mut Vec<Diagnostic>, class: impl Into<String>, at: usize, bytes: usize) {
    diags.push(Diagnostic::Skipped {
        class: class.into(),
        at,
        bytes,
    });
}

/// The folder of the first definition (`<prefix><folder><suffix>`) that
/// carries each name, built by one pass over the archive the first time a
/// fallback needs it. Every candidate entry is inflated once, however many
/// items fall back.
#[derive(Default)]
struct NameIndex(Option<HashMap<String, String>>);

impl NameIndex {
    fn folder_of(
        &mut self,
        zip: &dyn Entries,
        prefix: &str,
        suffix: &str,
        name_in: impl Fn(&str) -> Option<String>,
        name: &str,
    ) -> Option<String> {
        let map = self.0.get_or_insert_with(|| {
            let mut map = HashMap::new();
            for (entry, _) in zip.list() {
                let Some(folder) = entry
                    .strip_prefix(prefix)
                    .and_then(|e| e.strip_suffix(suffix))
                else {
                    continue;
                };
                let Some(bytes) = zip.read(&entry).ok().flatten() else {
                    continue;
                };
                if let Some(n) = name_in(&String::from_utf8_lossy(&bytes)) {
                    map.entry(n).or_insert_with(|| folder.to_string());
                }
            }
            map
        });
        map.get(name).cloned()
    }
}

/// The name a `hatchpattern.xml` carries.
fn pattern_name_in(xml: &str) -> Option<String> {
    crate::read26::xml_attrs(xml, "<hp:hatchpattern ")?.remove("name")
}

/// The name an `environment.xml` carries.
fn environment_name_in(xml: &str) -> Option<String> {
    let i = xml.find("<env:environment ")? + "<env:environment ".len();
    let a = ordered_attrs(&xml[i..i + crate::read26::tag_end(&xml[i..])?]);
    a.into_iter().find(|(k, _)| k == "name").map(|(_, v)| v)
}

/// The hatch pattern list: each item's name and thumbnail from
/// `model.dat`, the rest from the pattern's `hatchpattern.xml`. A damaged
/// item is skipped and recorded.
pub(crate) fn hatch_patterns(
    top: &Node<'_>,
    zip: &dyn Entries,
    diags: &mut Vec<Diagnostic>,
) -> HatchTable {
    let mut table = HatchTable {
        patterns: Vec::new(),
        index: HashMap::new(),
    };
    let Some(list) = pool(top, HATCH_LIST, diags) else {
        return table;
    };
    report_unknown(&list, &[HATCH_WRAPPER], "hatch list", diags);
    let Some(wrapper) = pool(&list, HATCH_WRAPPER, diags) else {
        return table;
    };
    report_unknown(&wrapper, &[HATCH_ITEMS], "hatch list", diags);
    let Some(items) = pool(&wrapper, HATCH_ITEMS, diags) else {
        return table;
    };
    report_unknown(&items, &[HATCH_ITEM], "hatch list", diags);
    let mut names = NameIndex::default();
    for r in items.all(HATCH_ITEM) {
        match item(r, zip, &mut names, diags) {
            Ok((id, pattern)) => {
                table.index.insert(id, table.patterns.len());
                table.patterns.push(pattern);
            }
            Err(_) => skipped(diags, "hatch pattern", r.off - 6, r.data.len() + 6),
        }
    }
    table
}

fn item(
    r: Rec<'_>,
    zip: &dyn Entries,
    names: &mut NameIndex,
    diags: &mut Vec<Diagnostic>,
) -> Result<(u32, HatchPattern), crate::model::Error> {
    let n = Node::of(r)?;
    report_unknown(
        &n,
        &[ENTITY, HATCH_NAME, HATCH_NAME_ALT, HATCH_THUMBNAIL],
        "hatch pattern",
        diags,
    );
    let id = entity_id(&n)?;
    let name = text(n.get(HATCH_NAME).or_else(|| n.get(HATCH_NAME_ALT)));
    if n.get(HATCH_NAME).is_some()
        && n.get(HATCH_NAME_ALT).is_some()
        && text(n.get(HATCH_NAME)) != text(n.get(HATCH_NAME_ALT))
    {
        skipped(
            diags,
            "hatch pattern names differ",
            r.off - 6,
            r.data.len() + 6,
        );
    }
    let mut pattern = HatchPattern {
        name: name.clone(),
        ..HatchPattern::default()
    };
    // The preview is named by its full archive path.
    if let Some(t) = n.node(HATCH_THUMBNAIL)? {
        report_unknown(&t, &[HATCH_THUMBNAIL_PATH], "hatch thumbnail", diags);
        let path = text(t.get(HATCH_THUMBNAIL_PATH));
        if !path.is_empty() {
            pattern.thumbnail = zip.read(&path).ok().flatten();
            if pattern.thumbnail.is_none() {
                skipped(diags, path, t.at - 6, t.len + 6);
            }
        }
    }
    // The definition: `hatchpatterns/<name>/` in the files seen, whose
    // folder name is not recorded. Candidates, in turn: the name, the
    // folder the preview sits beside (`<folder>_1`), the name as a folder
    // name is made of it, and the one definition that carries the name.
    let preview_folder = n
        .node(HATCH_THUMBNAIL)?
        .map(|t| text(t.get(HATCH_THUMBNAIL_PATH)))
        .and_then(|p| {
            p.strip_prefix("hatchpatterns/")?
                .strip_suffix("_1/thumbnail.jpg")
                .map(str::to_string)
        });
    let sanitized = crate::build26::folder_name(&name, &mut HashSet::new());
    let found = [Some(name.clone()), preview_folder, Some(sanitized)]
        .into_iter()
        .flatten()
        .find_map(|f| {
            let bytes = zip.read(&xml_entry(&f)).ok().flatten()?;
            Some((f, bytes))
        })
        .or_else(|| {
            let folder = names.folder_of(
                zip,
                "hatchpatterns/",
                "/hatchpattern.xml",
                pattern_name_in,
                &name,
            )?;
            let bytes = zip.read(&xml_entry(&folder)).ok().flatten()?;
            Some((folder, bytes))
        });
    match found {
        Some((folder, bytes)) => definition(
            &String::from_utf8_lossy(&bytes),
            &folder,
            zip,
            &mut pattern,
            diags,
            r,
        ),
        None => skipped(diags, xml_entry(&name), r.off - 6, r.data.len() + 6),
    }
    Ok((id, pattern))
}

/// The attributes of `<hp:hatchpattern …>` (in the order the writer gives
/// them: `name`, `colorRed`, `colorGreen`, `colorBlue`, `hasTexture`,
/// `xScale`, `yScale`, `rotationAngle`, `hasPat`, then `patName` and
/// `patData` when `hasPat` is 1) and its `textureName` child, if any.
fn definition(
    xml: &str,
    folder: &str,
    zip: &dyn Entries,
    pattern: &mut HatchPattern,
    diags: &mut Vec<Diagnostic>,
    r: Rec<'_>,
) {
    let Some(a) = crate::read26::xml_attrs(xml, "<hp:hatchpattern ") else {
        skipped(
            diags,
            "hatchpattern.xml without a pattern",
            r.off - 6,
            r.data.len() + 6,
        );
        return;
    };
    let num = |k: &str| a.get(k).and_then(|v| v.parse::<f64>().ok());
    let byte = |k: &str, d: u8| a.get(k).and_then(|v| v.parse::<u8>().ok()).unwrap_or(d);
    pattern.color = [
        byte("colorRed", 255),
        byte("colorGreen", 255),
        byte("colorBlue", 255),
    ];
    pattern.x_scale = num("xScale").unwrap_or(1.0);
    pattern.y_scale = num("yScale").unwrap_or(1.0);
    pattern.rotation = num("rotationAngle").unwrap_or(0.0);
    if a.get("hasPat").map(String::as_str) == Some("1") {
        pattern.pat_name = Some(a.get("patName").cloned().unwrap_or_default());
        let data = a.get("patData").map(String::as_str).unwrap_or("");
        match unbase64(data).and_then(|b| pattern_lines(&b)) {
            Some(lines) => pattern.lines = lines,
            None => skipped(
                diags,
                "hatch pattern data does not tile as lines",
                r.off - 6,
                r.data.len() + 6,
            ),
        }
    }
    if a.get("hasTexture").map(String::as_str) == Some("1") {
        if let Some(t) = crate::read26::xml_attrs(xml, "<textureName ") {
            let path = t.get("path").cloned().unwrap_or_default();
            let entry = format!("hatchpatterns/{folder}/{}", path.trim_start_matches("./"));
            let png = zip.read(&entry).ok().flatten();
            if png.is_none() {
                skipped(diags, entry, r.off - 6, r.data.len() + 6);
            }
            pattern.texture = Some(HatchTexture {
                id: t.get("id").and_then(|v| v.parse().ok()).unwrap_or(1),
                path,
                file_name: t.get("file_name").cloned().unwrap_or_default(),
                png: png.unwrap_or_default(),
            });
        }
    }
}

/// The features of a file: the hatch patterns and every reference to one.
/// `materials` maps a material id to its slot (index + 1); `scanned`
/// holds what the entity walk met.
pub(crate) fn features(
    top: &Node<'_>,
    zip: &dyn Entries,
    hatches: &HatchTable,
    materials: &HashMap<u32, u16>,
    layers: &HashMap<u32, u16>,
    scanned: Scanned<'_>,
    diags: &mut Vec<Diagnostic>,
) -> Features {
    let mut f = Features {
        hatch_patterns: hatches.patterns.clone(),
        ..Features::default()
    };
    if let Some(list) = section(top, 0x01F7, &[0x30D4, 0x30D5], diags) {
        for r in list.all(0x32C8) {
            let Ok(m) = Node::of(r) else { continue };
            let Some(hid) = uint(m.get(MATERIAL_HATCH)) else {
                continue;
            };
            let slot = entity_id(&m).ok().and_then(|id| materials.get(&id));
            match (slot, hatches.index.get(&hid)) {
                (Some(slot), Some(h)) => f.material_hatch.push((*slot as usize - 1, *h)),
                _ => skipped(
                    diags,
                    "material hatch reference",
                    r.off - 6,
                    r.data.len() + 6,
                ),
            }
        }
    }
    for &(i, hid) in scanned.instance_hatch {
        match hatches.index.get(&hid) {
            Some(h) => f.instance_hatch.push((i, *h)),
            None => skipped(diags, "instance hatch reference", 0, 0),
        }
    }
    f.render_hatch = render_hatch(top, diags);
    layer_features(top, layers, &mut f, diags);
    f.crs = crs(top, diags);
    pbr_materials(top, zip, materials, &mut f, diags);
    let env_index = environments(top, zip, &mut f, diags);
    page_environments(top, &env_index, &mut f, diags);
    page_extras(top, &mut f, diags);
    f.snaps = scanned.snaps;
    procedures(
        top,
        scanned.results,
        scanned.definition_links,
        &mut f,
        diags,
    );
    f
}

/// What the entity walk of `read26` hands over for the features.
pub(crate) struct Scanned<'a> {
    /// `(instance index, hatch id)` as the entity walk met them.
    pub(crate) instance_hatch: &'a [(usize, u32)],
    /// The snaps of the model and of the definitions.
    pub(crate) snaps: Vec<Snap>,
    /// `(definition index, placements in the wrapper of its result)`.
    pub(crate) results: Vec<(usize, Vec<PlacedInstance>)>,
    /// `(definition container id, index into the definitions)`, sorted.
    pub(crate) definition_links: &'a [(u32, usize)],
}

/// A layer's index in `Model::layers` from its id.
fn layer_index(layers: &HashMap<u32, u16>, id: u32) -> Option<usize> {
    layers.get(&id).map(|slot| *slot as usize - 1)
}

/// The layers' line styles and page behaviour, and the layer folders.
/// `layers` maps a layer id to its slot (index + 1).
fn layer_features(
    top: &Node<'_>,
    layers: &HashMap<u32, u16>,
    f: &mut Features,
    diags: &mut Vec<Diagnostic>,
) {
    let line_styles: HashMap<u32, usize> = section(top, LINE_STYLES, &[0x4074, 0x4075], diags)
        .map(|list| {
            list.all(LINE_STYLE)
                .filter_map(|r| entity_id(&Node::of(r).ok()?).ok())
                .enumerate()
                .map(|(i, id)| (id, i))
                .collect()
        })
        .unwrap_or_default();
    if let Some(list) = section(top, LAYER_SECTION, &[LAYER_LIST_WRAPPER, LAYER_LIST], diags) {
        for r in list.all(LAYER) {
            let Ok(l) = Node::of(r) else { continue };
            let Some(index) = entity_id(&l).ok().and_then(|id| layer_index(layers, id)) else {
                continue;
            };
            if let Some(b) = uint(l.get(LAYER_PAGE_BEHAVIOUR)).filter(|b| *b != 0) {
                f.layer_page_behaviour.push((index, b));
            }
            if let Some(sr) = l.get(LAYER_LINE_STYLE) {
                match uint(Some(sr)).and_then(|id| line_styles.get(&id)) {
                    Some(s) => f.layer_line_styles.push((index, *s)),
                    None => skipped(diags, "layer line style", sr.off - 6, sr.data.len() + 6),
                }
            }
        }
    }
    let Some(wrapper) = section(
        top,
        LAYER_SECTION,
        &[LAYER_LIST_WRAPPER, LAYER_FOLDERS],
        diags,
    ) else {
        return;
    };
    let Some(root) = wrapper.get(FOLDER) else {
        return;
    };
    let Ok(root) = Node::of(root) else {
        skipped(diags, "layer folders", root_off(&wrapper), wrapper.len);
        return;
    };
    let (folders, plain) = folder_children(&root, layers, 0, diags);
    // The root holds the folders and nothing else: a loose layer is in
    // no list, and its own flags stay clear.
    if !plain
        || !ids(root.get(FOLDER_LAYERS)).is_empty()
        || uint(root.get(FOLDER_HIDDEN)).unwrap_or(0) != 0
        || uint(root.get(FOLDER_NEW_PAGES)).unwrap_or(0) != 0
        || !text(root.get(FOLDER_NAME)).is_empty()
    {
        skipped(diags, "layer folder root", root.at - 6, root.len + 6);
    }
    f.layer_folders = folders;
}

fn root_off(n: &Node<'_>) -> usize {
    n.at.saturating_sub(6)
}

/// The folders under `n` (`0x3e83`), and whether the whole subtree read
/// without loss.
fn folder_children(
    n: &Node<'_>,
    layers: &HashMap<u32, u16>,
    depth: usize,
    diags: &mut Vec<Diagnostic>,
) -> (Vec<LayerFolder>, bool) {
    let Some(sub) = n.get(FOLDER_FOLDERS) else {
        return (Vec::new(), true);
    };
    let mut out = Vec::new();
    let mut ok = true;
    let Ok(sub) = Node::of(sub) else {
        skipped(diags, "layer folders", sub.off - 6, sub.data.len() + 6);
        return (out, false);
    };
    report_unknown(&sub, &[FOLDER], "layer folders", diags);
    for r in sub.all(FOLDER) {
        if depth >= FOLDER_DEPTH {
            skipped(
                diags,
                "layer folders nested too deep",
                r.off - 6,
                r.data.len() + 6,
            );
            ok = false;
            continue;
        }
        let Ok(fnode) = Node::of(r) else {
            skipped(diags, "layer folder", r.off - 6, r.data.len() + 6);
            ok = false;
            continue;
        };
        report_unknown(
            &fnode,
            &[
                ENTITY,
                FOLDER_NAME,
                FOLDER_HIDDEN,
                FOLDER_FOLDERS,
                FOLDER_LAYERS,
                FOLDER_NEW_PAGES,
            ],
            "layer folder",
            diags,
        );
        let mut members = Vec::new();
        let (member_ids, cut) = ids_checked(fnode.get(FOLDER_LAYERS));
        if cut {
            skipped(
                diags,
                "layer folder member list",
                r.off - 6,
                r.data.len() + 6,
            );
            ok = false;
        }
        for id in member_ids {
            match layer_index(layers, id) {
                Some(i) => members.push(i),
                None => {
                    skipped(diags, "layer folder member", r.off - 6, r.data.len() + 6);
                    ok = false;
                }
            }
        }
        let (folders, inner) = folder_children(&fnode, layers, depth + 1, diags);
        ok &= inner;
        out.push(LayerFolder {
            name: text(fnode.get(FOLDER_NAME)),
            hidden: uint(fnode.get(FOLDER_HIDDEN)).unwrap_or(0) != 0,
            hidden_on_new_pages: uint(fnode.get(FOLDER_NEW_PAGES)).unwrap_or(0) != 0,
            layers: members,
            folders,
        });
    }
    (out, ok)
}

/// The coordinate reference system; `None` while its record is empty.
fn crs(top: &Node<'_>, diags: &mut Vec<Diagnostic>) -> Option<Crs> {
    let sec = pool(top, CRS_SECTION, diags)?;
    report_unknown(&sec, &[CRS], "crs section", diags);
    let r = sec.get(CRS)?;
    if r.data.is_empty() {
        return None;
    }
    let Ok(n) = Node::of(r) else {
        skipped(diags, "crs", r.off - 6, r.data.len() + 6);
        return None;
    };
    // A record that is there but not an f64 is reported and reads as 0.
    let f64_of = |tag: u16, diags: &mut Vec<Diagnostic>| match n.get(tag) {
        None => 0.0,
        Some(r) if r.data.len() == 8 => f64::from_le_bytes(r.data.try_into().unwrap()),
        Some(r) => {
            skipped(
                diags,
                format!("crs record {tag:#06x}"),
                r.off - 6,
                r.data.len() + 6,
            );
            0.0
        }
    };
    let mut c = Crs {
        eastings: f64_of(0x7E91, diags),
        northings: f64_of(0x7E92, diags),
        height: f64_of(0x7E93, diags),
        x_axis: [f64_of(0x7E94, diags), f64_of(0x7E95, diags)],
        scale: f64_of(0x7E96, diags),
        name: text(n.get(CRS_NAME)),
        description: text(n.get(CRS_DESCRIPTION)),
        map_zone: text(n.get(CRS_MAP_ZONE)),
        geodetic_datum: text(n.get(0x7E99)),
        vertical_datum: text(n.get(0x7E9A)),
        map_projection: text(n.get(0x7E9B)),
        map_unit: text(n.get(0x7E9D)),
        map_unit_metres: n
            .get(0x7E9E)
            .and_then(|r| r.data.get(..8))
            .map(|b| f64::from_le_bytes(b.try_into().unwrap())),
        tgl_id: text(n.get(0x7E9F)),
        calibration_file_name: text(n.get(0x7EA0)),
        unknown: Vec::new(),
    };
    const KNOWN: [u16; 16] = [
        0x7E91,
        0x7E92,
        0x7E93,
        0x7E94,
        0x7E95,
        0x7E96,
        CRS_NAME,
        CRS_DESCRIPTION,
        CRS_MAP_ZONE,
        0x7E99,
        0x7E9A,
        0x7E9B,
        0x7E9D,
        0x7E9E,
        0x7E9F,
        0x7EA0,
    ];
    for k in &n.kids {
        if !KNOWN.contains(&k.tag) {
            c.unknown.push((k.tag, k.data.to_vec()));
        }
    }
    Some(c)
}

fn f32_of(r: Option<Rec<'_>>) -> Option<f32> {
    let b = r?.data;
    (b.len() == 4).then(|| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The rendering options' hatch settings; `None` while they are the
/// defaults (the fill colour then being the section fill colour).
fn render_hatch(top: &Node<'_>, diags: &mut Vec<Diagnostic>) -> Option<RenderHatch> {
    let ro = top.node(0x01FB).ok()??.node(0x733C).ok()??;
    let r = ro.get(RENDER_HATCH)?;
    let n = Node::of(r).ok()?;
    report_unknown(&n, &[HATCH_ITEM], "rendering hatch", diags);
    let inner = n.get(HATCH_ITEM)?;
    let Ok(h) = Node::of(inner) else {
        skipped(
            diags,
            "rendering hatch",
            inner.off - 6,
            inner.data.len() + 6,
        );
        return None;
    };
    report_unknown(
        &h,
        &[
            RO_COLOR,
            RO_SLOT_A,
            RO_SLOT_B,
            RO_X_SCALE,
            RO_Y_SCALE,
            RO_ROTATION,
            RO_PATTERN,
            RO_PAT_NAME,
        ],
        "rendering hatch",
        diags,
    );
    let bad = |diags: &mut Vec<Diagnostic>| {
        skipped(
            diags,
            "rendering hatch",
            inner.off - 6,
            inner.data.len() + 6,
        )
    };
    let color: [u8; 4] = match h.get(RO_COLOR).map(|c| c.data) {
        Some(c) if c.len() == 4 => [c[0], c[1], c[2], c[3]],
        _ => {
            bad(diags);
            return None;
        }
    };
    let (Some(x_scale), Some(y_scale), Some(rotation)) = (
        f32_of(h.get(RO_X_SCALE)),
        f32_of(h.get(RO_Y_SCALE)),
        f32_of(h.get(RO_ROTATION)),
    ) else {
        bad(diags);
        return None;
    };
    let lines = match h.get(RO_PATTERN) {
        None => Vec::new(),
        Some(p) => match pattern_lines(p.data) {
            Some(l) => l,
            None => {
                bad(diags);
                return None;
            }
        },
    };
    let slot = |t: u16| h.get(t).map(|r| r.data.to_vec()).unwrap_or_default();
    let found = RenderHatch {
        color,
        x_scale,
        y_scale,
        rotation,
        pat_name: text(h.get(RO_PAT_NAME)),
        lines,
        texture_slots: [slot(RO_SLOT_A), slot(RO_SLOT_B)],
    };
    // The fill colour is the section fill colour, written twice: with the
    // rest at its defaults there is nothing of the hatch to carry.
    let fill = ro
        .get(RO_SECTION_FILL)
        .map(|c| c.data)
        .filter(|c| c.len() == 4)
        .map(|c| [c[0], c[1], c[2], c[3]]);
    let plain = RenderHatch {
        color: fill.unwrap_or(RenderHatch::default().color),
        ..RenderHatch::default()
    };
    (found != plain).then_some(found)
}

// ------------------------------------------------- physically based materials

/// The attributes of `mat:material` the reader types; any other is kept
/// on [`PbrMaterial::other_material_attrs`].
const MATERIAL_ATTRS: [&str; 11] = [
    "name",
    "type",
    "workflow",
    "colorRed",
    "colorGreen",
    "colorBlue",
    "colorizeType",
    "trans",
    "useTrans",
    "pbrPromoState",
    "hasTexture",
];

/// The attributes of a start tag in file order; `tag` is the text between
/// the element name and the closing `>` (a trailing `/` is ignored).
pub(crate) fn ordered_attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = tag.trim_end_matches('/');
    while let Some(eq) = rest.find("=\"") {
        let key = rest[..eq].trim().to_string();
        let v0 = eq + 2;
        let Some(len) = rest[v0..].find('"') else {
            break;
        };
        out.push((key, crate::read26::unescape(&rest[v0..v0 + len])));
        rest = &rest[v0 + len + 1..];
    }
    out
}

/// One element of an XML fragment, as [`elements`] scans them.
pub(crate) struct Elem {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    /// The element's text; empty for a self-closing element.
    pub(crate) text: String,
    /// The element exactly as the fragment has it.
    pub(crate) raw: String,
}

/// The child elements of `s`, one level deep: a start tag with its end
/// tag, or a self-closing tag. Comments and declarations are skipped.
pub(crate) fn elements(s: &str) -> Vec<Elem> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(lt) = s[i..].find('<') {
        let start = i + lt;
        let rest = &s[start..];
        if let Some(c) = rest.strip_prefix("<!--") {
            match c.find("-->") {
                Some(e) => i = start + 4 + e + 3,
                None => break,
            }
            continue;
        }
        let Some(gt) = crate::read26::tag_end(rest) else {
            break;
        };
        if rest.starts_with("<?") || rest.starts_with("</") {
            i = start + gt + 1;
            continue;
        }
        let tag = &rest[1..gt];
        let (name, attr_text) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let name = name.trim_end_matches('/');
        if tag.ends_with('/') {
            out.push(Elem {
                name: name.to_string(),
                attrs: ordered_attrs(attr_text),
                text: String::new(),
                raw: rest[..=gt].to_string(),
            });
            i = start + gt + 1;
            continue;
        }
        let close = format!("</{name}>");
        let Some(end) = rest[gt + 1..].find(&close).map(|e| gt + 1 + e) else {
            break;
        };
        out.push(Elem {
            name: name.to_string(),
            attrs: ordered_attrs(attr_text),
            text: crate::read26::unescape(rest[gt + 1..end].trim()),
            raw: rest[..end + close.len()].to_string(),
        });
        i = start + end + close.len();
    }
    out
}

/// The slot of each texture element of `mat:pbrMR`, in the order the file
/// has them.
const PBR_TEXTURES: [&str; 4] = [
    "mat:metallic_texture",
    "mat:roughness_texture",
    "mat:normal_texture",
    "mat:occlusion_texture",
];

/// The physically based properties of every material that has any
/// (`Features::material_pbr`).
fn pbr_materials(
    top: &Node<'_>,
    zip: &dyn Entries,
    materials: &HashMap<u32, u16>,
    f: &mut Features,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(list) = section(top, 0x01F7, &[0x30D4, 0x30D5], diags) else {
        return;
    };
    for r in list.all(0x32C8) {
        let Ok(m) = Node::of(r) else { continue };
        let Some(slot) = entity_id(&m).ok().and_then(|id| materials.get(&id)) else {
            continue;
        };
        let folder = text(m.get(0x32CC));
        let Some(bytes) = zip
            .read(&format!("materials/{folder}/material.xml"))
            .ok()
            .flatten()
        else {
            continue;
        };
        let p = pbr_of(
            &String::from_utf8_lossy(&bytes),
            &folder,
            zip,
            diags,
            (r.off - 6, r.data.len() + 6),
        );
        // The unused `trans` alone is no physically based property: a
        // material of an older file can hold it.
        let plain = PbrMaterial {
            trans_unused: p.trans_unused,
            ..PbrMaterial::default()
        };
        if p != plain {
            f.material_pbr.push((*slot as usize - 1, p));
        }
    }
}

/// The physically based properties in a `material.xml`.
fn pbr_of(
    xml: &str,
    folder: &str,
    zip: &dyn Entries,
    diags: &mut Vec<Diagnostic>,
    at: (usize, usize),
) -> PbrMaterial {
    let mut p = PbrMaterial::default();
    let bad = |diags: &mut Vec<Diagnostic>, what: &str| {
        skipped(diags, format!("material.xml {what}"), at.0, at.1)
    };
    if let Some(i) = xml.find("<mat:material ") {
        let tag = &xml[i + "<mat:material ".len()..];
        let tag = &tag[..crate::read26::tag_end(tag).unwrap_or(tag.len())];
        let attrs = ordered_attrs(tag);
        let get = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        let uint = |k: &str, d: u32, diags: &mut Vec<Diagnostic>| match get(k) {
            None => d,
            Some(v) => v.parse().unwrap_or_else(|_| {
                bad(diags, k);
                d
            }),
        };
        p.workflow = uint("workflow", 0, diags);
        p.promo_state = uint("pbrPromoState", 0, diags);
        if get("useTrans") != Some("1") {
            p.trans_unused = get("trans")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|t| *t != 1.0);
        }
        p.other_material_attrs = attrs
            .iter()
            .filter(|(k, _)| !MATERIAL_ATTRS.contains(&k.as_str()))
            .cloned()
            .collect();
    }
    // The scale a material gives the block: 1 for a solid colour, the
    // applied size of its texture otherwise.
    let own_scale = || {
        let t = xml.find("<mat:texture ").map(|i| {
            let t = &xml[i + "<mat:texture ".len()..];
            ordered_attrs(&t[..crate::read26::tag_end(t).unwrap_or(t.len())])
        });
        let num = |t: &[(String, String)], k: &str| {
            t.iter()
                .find(|(a, _)| a == k)
                .and_then(|(_, v)| v.parse::<f64>().ok())
        };
        match t {
            Some(t) => (
                num(&t, "xScale").unwrap_or(1.0),
                num(&t, "yScale").unwrap_or(1.0),
            ),
            None => (1.0, 1.0),
        }
    };
    let Some(open) = xml.find("<mat:pbrMR") else {
        return p;
    };
    let after = &xml[open + "<mat:pbrMR".len()..];
    let Some(gt) = crate::read26::tag_end(after) else {
        return p;
    };
    let attrs = ordered_attrs(&after[..gt]);
    let inner = &after[gt + 1..];
    let inner = &inner[..inner.find("</mat:pbrMR>").unwrap_or(inner.len())];
    let scale = |k: &str| {
        attrs
            .iter()
            .find(|(a, _)| a == k)
            .and_then(|(_, v)| v.parse::<f64>().ok())
    };
    let own = own_scale();
    let (x, y) = (
        scale("xScale").unwrap_or(own.0),
        scale("yScale").unwrap_or(own.1),
    );
    if (x, y) != own {
        p.scale = Some([x, y]);
    }
    p.other_pbr_attrs = attrs
        .into_iter()
        .filter(|(k, _)| k != "xScale" && k != "yScale")
        .collect();
    let mut seen: HashSet<String> = HashSet::new();
    for e in elements(inner) {
        let first = seen.insert(e.name.clone());
        let flag = |diags: &mut Vec<Diagnostic>| match e.text.as_str() {
            "0" => false,
            "1" => true,
            _ => {
                bad(diags, &e.name);
                false
            }
        };
        let float = |diags: &mut Vec<Diagnostic>| {
            e.text.parse::<f32>().unwrap_or_else(|_| {
                bad(diags, &e.name);
                1.0
            })
        };
        let uint = |diags: &mut Vec<Diagnostic>, d: u32| {
            e.text.parse::<u32>().unwrap_or_else(|_| {
                bad(diags, &e.name);
                d
            })
        };
        match e.name.as_str() {
            _ if !first => {}
            "mat:enable_metalness" => p.enable_metalness = flag(diags),
            "mat:enable_roughness" => p.enable_roughness = flag(diags),
            "mat:enable_normal" => p.enable_normal = flag(diags),
            "mat:enable_occlusion" => p.enable_occlusion = flag(diags),
            "mat:roughness_texture_invert" => p.roughness_texture_invert = flag(diags),
            "mat:metallicFactor" => p.metallic_factor = float(diags),
            "mat:roughnessFactor" => p.roughness_factor = float(diags),
            "mat:normalScale" => p.normal_scale = float(diags),
            "mat:occlusionStrength" => p.occlusion_strength = float(diags),
            "mat:normalMapStyle" => p.normal_map_style = uint(diags, 1),
            "mat:baseColorFactor" => p.base_color_factor = uint(diags, 0xFFFF_FFFF),
            n if PBR_TEXTURES.contains(&n) => {
                let a = |k: &str| {
                    e.attrs
                        .iter()
                        .find(|(a, _)| a == k)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default()
                };
                let path = a("path");
                let entry = match path.strip_prefix("./") {
                    Some(rel) => format!("materials/{folder}/{rel}"),
                    None => path.clone(),
                };
                let png = zip.read(&entry).ok().flatten();
                if png.is_none() {
                    skipped(diags, entry, at.0, at.1);
                }
                let tex = PbrTexture {
                    id: a("id").parse().unwrap_or(0),
                    path,
                    file_name: a("file_name"),
                    png: png.unwrap_or_default(),
                    other_attrs: e
                        .attrs
                        .iter()
                        .filter(|(k, _)| !["id", "path", "file_name"].contains(&k.as_str()))
                        .cloned()
                        .collect(),
                };
                match n {
                    "mat:metallic_texture" => p.metallic = Some(tex),
                    "mat:roughness_texture" => p.roughness = Some(tex),
                    "mat:normal_texture" => p.normal = Some(tex),
                    _ => p.occlusion = Some(tex),
                }
            }
            _ => p.other_elements.push(e.raw),
        }
    }
    p
}

// ------------------------------------------------------------- environments

/// The environment section (`0x0210`): a collection (`0x7918`) of the
/// list (`0x7919`) of environments (`0x7b0c`) and the current
/// environment's id (`0x791a`).
const ENV_SECTION: u16 = 0x0210;
const ENV_COLLECTION: u16 = 0x7918;
const ENV_LIST: u16 = 0x7919;
const ENV_CURRENT: u16 = 0x791A;
const ENV_ITEM: u16 = 0x7B0C;
const ENV_NAME: u16 = 0x7B0D;
const ENV_THUMBNAIL: u16 = 0x7B0E;

/// A page's environment id and the two flag bytes of `0x715b`.
const SCENE_ENV_ID: u16 = 0x715A;
const SCENE_ENV_FLAGS: u16 = 0x715B;
const SCENE_FLAG_A: u16 = 0x7B0F;
const SCENE_FLAG_B: u16 = 0x7B10;
/// The scene's saved-properties word (`0x7149`) and its environment bit.
const SCENE_SAVED: u16 = 0x7149;
const SCENE_SAVED_ENVIRONMENT: u32 = 0x200;

/// The archive entry of an environment's settings, by its folder.
pub(crate) fn environment_entry(folder: &str) -> String {
    format!("environments/{folder}/environment.xml")
}

/// The environments and the current one; returns each environment's id as
/// an index into `Features::environments`.
fn environments(
    top: &Node<'_>,
    zip: &dyn Entries,
    f: &mut Features,
    diags: &mut Vec<Diagnostic>,
) -> HashMap<u32, usize> {
    let mut index = HashMap::new();
    let Some(coll) = section(top, ENV_SECTION, &[ENV_COLLECTION], diags) else {
        return index;
    };
    report_unknown(&coll, &[ENV_LIST, ENV_CURRENT], "environments", diags);
    if let Some(list) = pool(&coll, ENV_LIST, diags) {
        report_unknown(&list, &[ENV_ITEM], "environment list", diags);
        let mut names = NameIndex::default();
        for r in list.all(ENV_ITEM) {
            match environment(r, zip, &mut names, diags) {
                Ok((id, e)) => {
                    index.insert(id, f.environments.len());
                    f.environments.push(e);
                }
                Err(_) => skipped(diags, "environment", r.off - 6, r.data.len() + 6),
            }
        }
    }
    if let Some(cur) = coll.get(ENV_CURRENT) {
        match uint(Some(cur)).and_then(|id| index.get(&id)) {
            Some(i) => f.current_environment = Some(*i),
            None => skipped(
                diags,
                "current environment",
                cur.off - 6,
                cur.data.len() + 6,
            ),
        }
    }
    index
}

fn environment(
    r: Rec<'_>,
    zip: &dyn Entries,
    names: &mut NameIndex,
    diags: &mut Vec<Diagnostic>,
) -> Result<(u32, Environment), crate::model::Error> {
    let n = Node::of(r)?;
    report_unknown(&n, &[ENTITY, ENV_NAME, ENV_THUMBNAIL], "environment", diags);
    let id = entity_id(&n)?;
    let name = text(n.get(ENV_NAME));
    let mut e = Environment {
        name: name.clone(),
        ..Environment::default()
    };
    // The folder is the one the preview sits in; without a preview, the
    // name, then the one definition that carries the name.
    let mut preview = None;
    if let Some(t) = n.node(ENV_THUMBNAIL)? {
        report_unknown(&t, &[HATCH_THUMBNAIL_PATH], "environment thumbnail", diags);
        let path = text(t.get(HATCH_THUMBNAIL_PATH));
        if !path.is_empty() {
            e.thumbnail = zip.read(&path).ok().flatten();
            if e.thumbnail.is_none() {
                skipped(diags, path.clone(), t.at - 6, t.len + 6);
            }
            preview = path
                .strip_prefix("environments/")
                .and_then(|p| p.strip_suffix("/thumbnail.jpg"))
                .map(str::to_string);
        }
    }
    let sanitized = crate::build26::folder_name(&name, &mut HashSet::new());
    let found = [preview, Some(name.clone()), Some(sanitized)]
        .into_iter()
        .flatten()
        .find_map(|f| {
            let bytes = zip.read(&environment_entry(&f)).ok().flatten()?;
            Some((f, bytes))
        })
        .or_else(|| {
            let folder = names.folder_of(
                zip,
                "environments/",
                "/environment.xml",
                environment_name_in,
                &name,
            )?;
            let bytes = zip.read(&environment_entry(&folder)).ok().flatten()?;
            Some((folder, bytes))
        });
    let Some((folder, bytes)) = found else {
        skipped(diags, environment_entry(&name), r.off - 6, r.data.len() + 6);
        return Ok((id, e));
    };
    let xml = String::from_utf8_lossy(&bytes);
    let bad = |diags: &mut Vec<Diagnostic>, what: &str| {
        skipped(
            diags,
            format!("environment.xml {what}"),
            r.off - 6,
            r.data.len() + 6,
        )
    };
    let Some(i) = xml.find("<env:environment ") else {
        bad(diags, "without an environment");
        return Ok((id, e));
    };
    let tag = &xml[i + "<env:environment ".len()..];
    let tag = &tag[..crate::read26::tag_end(tag).unwrap_or(tag.len())];
    for (k, v) in ordered_attrs(tag) {
        let flag = |diags: &mut Vec<Diagnostic>| match v.as_str() {
            "0" => false,
            "1" => true,
            _ => {
                bad(diags, &k);
                false
            }
        };
        let float = |diags: &mut Vec<Diagnostic>| {
            v.parse::<f32>().unwrap_or_else(|_| {
                bad(diags, &k);
                0.0
            })
        };
        match k.as_str() {
            "name" => e.name = v.clone(),
            "description" => e.description = v.clone(),
            "ibl_enabled" => e.ibl_enabled = flag(diags),
            "rotation_angle" => e.rotation_angle = float(diags),
            "skybox_enabled" => e.skybox_enabled = flag(diags),
            "skybox_gain" => e.skybox_gain = float(diags),
            "reflection_gain" => e.reflection_gain = float(diags),
            "shadow_light_enabled" => e.shadow_light_enabled = flag(diags),
            "shadow_light_positionX" => e.shadow_light_position[0] = float(diags),
            "shadow_light_positionY" => e.shadow_light_position[1] = float(diags),
            "ibl_filename" => e.ibl_filename = v.clone(),
            _ => e.other_attrs.push((k.clone(), v.clone())),
        }
    }
    if let Some(a) = crate::read26::xml_attrs(&xml, "<ibl_image ") {
        e.image = EnvironmentImage {
            id: a.get("id").and_then(|v| v.parse().ok()).unwrap_or(1),
            path: a.get("path").cloned().unwrap_or_default(),
            file_name: a.get("file_name").cloned().unwrap_or_default(),
        };
        let entry = match e.image.path.strip_prefix("./") {
            Some(rel) => format!("environments/{folder}/{rel}"),
            None => e.image.path.clone(),
        };
        match zip.read(&entry).ok().flatten() {
            Some(b) => e.hdr = b,
            None => skipped(diags, entry, r.off - 6, r.data.len() + 6),
        }
    }
    Ok((id, e))
}

/// The pages' environment settings, for the pages that differ from the
/// default [`PageEnvironment`].
fn page_environments(
    top: &Node<'_>,
    env_index: &HashMap<u32, usize>,
    f: &mut Features,
    diags: &mut Vec<Diagnostic>,
) {
    let Some(list) = section(top, 0x0207, &[0x6D60, 0x6D61], diags) else {
        return;
    };
    for (k, r) in list.all(0x7148).enumerate() {
        let Ok(pg) = Node::of(r) else { continue };
        let saved = uint(pg.get(SCENE_SAVED)).unwrap_or(0) & SCENE_SAVED_ENVIRONMENT != 0;
        let mut pe = PageEnvironment {
            saved,
            ..PageEnvironment::default()
        };
        if let Some(id) = pg.get(SCENE_ENV_ID) {
            match uint(Some(id)).and_then(|i| env_index.get(&i)) {
                Some(i) => pe.environment = Some(*i),
                None => skipped(diags, "page environment", id.off - 6, id.data.len() + 6),
            }
        }
        let flag = |r: Option<Rec<'_>>| r.is_some_and(|r| r.data.first().is_some_and(|b| *b != 0));
        match pg.node(SCENE_ENV_FLAGS) {
            Ok(Some(fl)) => {
                pe.flag_a = flag(fl.get(SCENE_FLAG_A));
                pe.flag_b = flag(fl.get(SCENE_FLAG_B));
            }
            Ok(None) => {}
            Err(_) => skipped(diags, "page environment flags", r.off - 6, r.data.len() + 6),
        }
        // A record out of step with the saved bit has no place in the
        // model.
        if saved != pg.get(SCENE_ENV_FLAGS).is_some() {
            skipped(
                diags,
                "page environment flags out of step with the saved properties",
                r.off - 6,
                r.data.len() + 6,
            );
        }
        if pe != PageEnvironment::default() {
            f.scene_environment.push((k, pe));
        }
    }
}

// ------------------------------------------------------- pages, procedures

/// The pre-order list of the layer folders' ids: each folder before its
/// subfolders, the folders [`layer_features`] keeps and no others.
fn folder_ids(n: &Node<'_>, depth: usize, out: &mut Vec<Option<u32>>) {
    let Some(Ok(sub)) = n.get(FOLDER_FOLDERS).map(Node::of) else {
        return;
    };
    for r in sub.all(FOLDER) {
        if depth >= FOLDER_DEPTH {
            continue;
        }
        let Ok(fnode) = Node::of(r) else { continue };
        out.push(entity_id(&fnode).ok());
        folder_ids(&fnode, depth + 1, out);
    }
}

fn f64_of(r: Option<Rec<'_>>) -> Option<f64> {
    let b = r?.data;
    (b.len() == 8).then(|| f64::from_le_bytes(b.try_into().unwrap()))
}

/// The scene records' layer folders, display names and times.
fn page_extras(top: &Node<'_>, f: &mut Features, diags: &mut Vec<Diagnostic>) {
    let Some(list) = section(top, 0x0207, &[0x6D60, 0x6D61], diags) else {
        return;
    };
    let mut folders = Vec::new();
    if let Some(root) = section(
        top,
        LAYER_SECTION,
        &[LAYER_LIST_WRAPPER, LAYER_FOLDERS],
        diags,
    )
    .and_then(|w| w.node(FOLDER).ok().flatten())
    {
        folder_ids(&root, 0, &mut folders);
    }
    // The first folder with each id, looked up once for every page.
    let mut folder_at: HashMap<u32, usize> = HashMap::new();
    for (i, id) in folders.iter().enumerate() {
        if let Some(id) = id {
            folder_at.entry(*id).or_insert(i);
        }
    }
    for (k, r) in list.all(0x7148).enumerate() {
        let Ok(pg) = Node::of(r) else { continue };
        let mut extra = PageExtras::default();
        if let Some(rec) = pg.get(0x7159) {
            let (hidden, cut) = ids_checked(Some(rec));
            if cut {
                skipped(
                    diags,
                    "page layer folder list",
                    rec.off - 6,
                    rec.data.len() + 6,
                );
            }
            for id in hidden {
                match folder_at.get(&id) {
                    Some(&i) => extra.hidden_layer_folders.push(i),
                    None => skipped(diags, "page layer folder", rec.off - 6, rec.data.len() + 6),
                }
            }
        }
        let name = pg
            .node(0x6F54)
            .ok()
            .flatten()
            .map(|b| text(b.get(0x6F55)))
            .unwrap_or_default();
        let shown = pg.get(0x7153).map(|r| text(Some(r)));
        let in_animation = uint(pg.get(0x7152)).unwrap_or(1) != 0;
        let derived = if in_animation {
            name
        } else {
            format!("({name})")
        };
        extra.display_name = shown.filter(|s| *s != derived);
        let own = |tag: u16| f64_of(pg.get(tag)).filter(|v| *v != -1.0);
        extra.transition_s = own(0x7154);
        extra.delay_s = own(0x7155);
        if extra != PageExtras::default() {
            f.scene_pages.push((k, extra));
        }
    }
}

/// The procedure records of the definitions, the placements of their
/// results, and the registry.
fn procedures(
    top: &Node<'_>,
    mut results: Vec<(usize, Vec<PlacedInstance>)>,
    links: &[(u32, usize)],
    f: &mut Features,
    diags: &mut Vec<Diagnostic>,
) {
    let index_of = |id: u32| {
        links
            .binary_search_by_key(&id, |l| l.0)
            .ok()
            .map(|i| links[i].1)
    };
    if let Some(sec) = pool(top, 0x01F9, diags) {
        for lrec in sec.all(0x1770) {
            let Some(list) = pool(&Node::of_lenient(lrec).0, 0x1771, diags) else {
                continue;
            };
            for drec in list.all(0x157C) {
                let (d, _) = Node::of_lenient(drec);
                let Some(rec) = d.get(PROCEDURE) else {
                    continue;
                };
                let Some(def) = d
                    .get(0x1388)
                    .and_then(|e| Node::of_lenient(e).0.into())
                    .and_then(|e| crate::read26::container_id(&e))
                    .and_then(index_of)
                else {
                    continue;
                };
                match procedure(rec, def, &mut results, links, diags) {
                    Some(p) => f.procedures.push(p),
                    None => skipped(diags, "procedure", rec.off - 6, rec.data.len() + 6),
                }
            }
        }
    }
    // A wrapper in a definition that names no procedure has no place in
    // the model.
    for (_, r) in results {
        if !r.is_empty() {
            skipped(diags, "procedure result without a procedure", 0, 0);
        }
    }
    if let Some(reg) = pool(top, REGISTRY, diags) {
        report_unknown(&reg, &[REGISTRY_ITEM], "procedure registry", diags);
        for item in reg.all(REGISTRY_ITEM) {
            let Ok(n) = Node::of(item) else {
                skipped(diags, "procedure kind", item.off - 6, item.data.len() + 6);
                continue;
            };
            report_unknown(
                &n,
                &[REGISTRY_NAME, REGISTRY_DESCRIPTION],
                "procedure kind",
                diags,
            );
            // The empty item every file holds names nothing.
            if n.kids.is_empty() {
                continue;
            }
            f.procedure_kinds.push(ProcedureKind {
                name: text(n.get(REGISTRY_NAME)),
                description: text(n.get(REGISTRY_DESCRIPTION)),
            });
        }
    }
}

fn procedure(
    rec: Rec<'_>,
    definition: usize,
    results: &mut [(usize, Vec<PlacedInstance>)],
    links: &[(u32, usize)],
    diags: &mut Vec<Diagnostic>,
) -> Option<Procedure> {
    let n = Node::of(rec).ok()?;
    report_unknown(&n, &[PROCEDURE_RECORD], "procedure", diags);
    let r = n.get(PROCEDURE_RECORD)?;
    let r = Node::of(r).ok()?;
    report_unknown(
        &r,
        &[
            PROCEDURE_NAME,
            PROCEDURE_VERSION,
            PROCEDURE_FLAGS,
            PROCEDURE_PARAMETERS,
        ],
        "procedure",
        diags,
    );
    let mut placed = Vec::new();
    for (d, list) in results.iter_mut() {
        if *d == definition {
            placed.append(list);
        }
    }
    let mut out = Vec::new();
    for p in placed {
        let Ok(i) = links.binary_search_by_key(&p.defref, |l| l.0) else {
            skipped(diags, "procedure result definition", 0, 0);
            continue;
        };
        out.push(ProcedureResult {
            pid: p.pid,
            definition: links[i].1,
            transform: p.transform,
            is_group: p.is_group,
            material: p.material,
            layer: p.layer,
            hidden: p.hidden,
            casts_shadows: p.casts_shadows,
            receives_shadows: p.receives_shadows,
            locked: p.locked,
            guid: p.guid,
        });
    }
    Some(Procedure {
        definition,
        name: text(r.get(PROCEDURE_NAME)),
        version: uint(r.get(PROCEDURE_VERSION)).unwrap_or(0),
        flags: uint(r.get(PROCEDURE_FLAGS)).unwrap_or(0),
        parameters: text(r.get(PROCEDURE_PARAMETERS)),
        results: out,
    })
}

/// A definition's procedure (`0x1588 > 0x7ec2`).
const PROCEDURE: u16 = 0x1588;
const PROCEDURE_RECORD: u16 = 0x7EC2;
const PROCEDURE_NAME: u16 = 0x7EC3;
const PROCEDURE_VERSION: u16 = 0x7EC4;
const PROCEDURE_FLAGS: u16 = 0x7EC5;
const PROCEDURE_PARAMETERS: u16 = 0x7EC6;
/// The registry of procedure kinds (`0x0215`).
const REGISTRY: u16 = 0x0215;
const REGISTRY_ITEM: u16 = 0x7D82;
const REGISTRY_NAME: u16 = 0x7D83;
const REGISTRY_DESCRIPTION: u16 = 0x7D84;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_close_tag_inside_an_attribute_value_is_not_the_close_tag() {
        let e = elements(r#"<a b="</a>">x</a><c/>"#);
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
        assert_eq!(e[0].text, "x");
        assert_eq!(e[0].attrs, [("b".to_string(), "</a>".to_string())]);
    }

    #[test]
    fn a_gt_inside_an_attribute_value_does_not_end_the_tag() {
        let e =
            elements(r#"<m:t id="1" path="a>b" file_name="x"/><m:on>1</m:on><m:q a='>'>2</m:q>"#);
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["m:t", "m:on", "m:q"]);
        assert_eq!(e[0].attrs[1], ("path".to_string(), "a>b".to_string()));
        assert_eq!(e[1].text, "1");
        assert_eq!(e[2].text, "2");
    }

    #[test]
    fn hostile_fragments_never_panic() {
        let cases = [
            "<",
            "<a",
            "<a b=\"",
            "<a b=\"</a>",
            "<a>",
            "</a>x</a>",
            "<a b=\"</a>\">",
            "<a>\u{e9}</a",
            "<!--",
            "<!-- x --",
            "<?x",
            "<a b='>",
            "<a/",
            "<a b=\"x\"</a>",
            "<\u{e9}>\u{e9}</\u{e9}>",
            "<a><a></a>",
        ];
        for c in cases {
            let _ = elements(c);
            let _ = ordered_attrs(c);
            let _ = crate::read26::xml_attrs(c, "<a");
            let _ = crate::read26::tag_end(c);
        }
        // Every prefix of a document that mixes the hazards.
        let doc = r#"<x a="</x>" b='>'><y c="1>2"/>t</y></x>"#;
        for i in (0..=doc.len()).filter(|i| doc.is_char_boundary(*i)) {
            let _ = elements(&doc[..i]);
            let _ = elements(&doc[i..]);
        }
    }

    struct Counting {
        entries: Vec<(String, String)>,
        reads: std::cell::Cell<usize>,
    }

    impl Entries for Counting {
        fn read(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
            self.reads.set(self.reads.get() + 1);
            Ok(self
                .entries
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| b.clone().into_bytes()))
        }
        fn list(&self) -> Vec<(String, usize)> {
            self.entries
                .iter()
                .map(|(n, b)| (n.clone(), b.len()))
                .collect()
        }
    }

    #[test]
    fn the_name_fallback_reads_each_definition_once() {
        let n = 200;
        let zip = Counting {
            entries: (0..n)
                .map(|i| {
                    (
                        format!("hatchpatterns/f{i}/hatchpattern.xml"),
                        format!("<hp:hatchpattern name=\"p{i}\"/>"),
                    )
                })
                .collect(),
            reads: Default::default(),
        };
        let mut idx = NameIndex::default();
        for i in 0..n {
            let folder = idx.folder_of(
                &zip,
                "hatchpatterns/",
                "/hatchpattern.xml",
                pattern_name_in,
                &format!("p{i}"),
            );
            assert_eq!(folder, Some(format!("f{i}")));
        }
        assert_eq!(
            idx.folder_of(
                &zip,
                "hatchpatterns/",
                "/hatchpattern.xml",
                pattern_name_in,
                "none"
            ),
            None
        );
        assert_eq!(zip.reads.get(), n);
    }
}
