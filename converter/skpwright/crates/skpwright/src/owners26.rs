//! The attribute dictionaries of the model's settings and lists (§16.8):
//! the current style, the styles, axes, shadow settings, line styles,
//! scenes and their display, shadow and axes settings, layer folders,
//! hatch patterns, and the lists themselves. Each such record holds its
//! dictionaries in the entity base `0x05dc > 0x05dd`, which leads the
//! record and has no id for the settings that are not listed entities.

use std::collections::HashMap;

use crate::read26::{attrs_of, entity_id, read_dictionaries, Node, Rec};
use crate::tree26::Record;
use crate::{Attributes, Owner};

const FOLDER: u16 = 0x3E80;
const FOLDER_FOLDERS: u16 = 0x3E83;
use crate::features::FOLDER_DEPTH;

/// The list records, each the second record of its section of the
/// model (`0x01f4`), with the owner of their own dictionaries.
const LISTS: [(Owner, [u16; 2]); 8] = [
    (Owner::Materials, [0x01F7, 0x30D4]),
    (Owner::Layers, [0x01F8, 0x3A98]),
    (Owner::Definitions, [0x01F9, 0x1770]),
    (Owner::Scenes, [0x0207, 0x6D60]),
    (Owner::Styles, [0x0206, 0x6978]),
    (Owner::LineStyles, [0x0208, 0x4074]),
    (Owner::HatchPatterns, [0x0211, 0x7D00]),
    (Owner::Environments, [0x0210, 0x7918]),
];

fn node<'a>(n: &Node<'a>, tag: u16) -> Option<Node<'a>> {
    n.node(tag).ok().flatten()
}

/// The dictionaries of the settings and lists of `top`. `scenes` is the
/// count of scenes the model read (a scene past it is dropped); `hatches`
/// maps a hatch pattern's id to its index.
pub(crate) fn read(
    top: &Node<'_>,
    scenes: usize,
    hatches: &HashMap<u32, usize>,
) -> Vec<Attributes> {
    let mut out = Vec::new();
    let mut add = |owner: Owner, rec: Option<Rec<'_>>| {
        let Some(Ok(n)) = rec.map(Node::of) else {
            return;
        };
        let d = read_dictionaries(attrs_of(&n));
        if !d.is_empty() {
            out.push(Attributes {
                owner,
                dictionaries: d,
            });
        }
    };
    for (owner, [sec, list]) in LISTS {
        add(owner, node(top, sec).and_then(|s| s.get(list)));
    }
    // The current style settings, then the styles of the list.
    if let Some(mgr) = node(top, 0x0206).and_then(|s| node(&s, 0x6978)) {
        add(
            Owner::CurrentStyle,
            node(&mgr, 0x697B).and_then(|c| c.get(0x6B6C)),
        );
        if let Some(list) = node(&mgr, 0x6979) {
            for (i, r) in list.all(0x6B6C).enumerate() {
                add(Owner::Style(i), Some(r));
            }
        }
    }
    add(
        Owner::Axes,
        node(top, 0x01FC)
            .and_then(|a| node(&a, 0x4650))
            .and_then(|a| a.get(0x07D0)),
    );
    add(
        Owner::ShadowInfo,
        node(top, 0x0204).and_then(|s| s.get(0x6590)),
    );
    if let Some(list) = node(top, 0x0208)
        .and_then(|l| node(&l, 0x4074))
        .and_then(|l| node(&l, 0x4075))
    {
        for (i, r) in list.all(0x4076).enumerate() {
            add(Owner::LineStyle(i), Some(r));
        }
    }
    if let Some(list) = node(top, 0x0207)
        .and_then(|s| node(&s, 0x6D60))
        .and_then(|l| node(&l, 0x6D61))
    {
        for (i, r) in list.all(0x7148).take(scenes).enumerate() {
            let Ok(pg) = Node::of(r) else { continue };
            add(Owner::Scene(i), pg.get(0x6F54));
            add(
                Owner::SceneRendering(i),
                node(&pg, 0x714D).and_then(|o| o.get(0x733C)),
            );
            add(
                Owner::SceneShadowInfo(i),
                node(&pg, 0x714E).and_then(|o| o.get(0x6590)),
            );
            add(
                Owner::SceneAxes(i),
                node(&pg, 0x714F)
                    .and_then(|o| node(&o, 0x4650))
                    .and_then(|a| a.get(0x07D0)),
            );
        }
    }
    if let Some(root) = node(top, 0x01F8)
        .and_then(|s| node(&s, 0x3A98))
        .and_then(|w| node(&w, 0x3A9B))
        .and_then(|w| node(&w, FOLDER))
    {
        let mut next = 0;
        folders(&root, 0, &mut next, &mut add);
    }
    if let Some(items) = node(top, 0x0211)
        .and_then(|l| node(&l, 0x7D00))
        .and_then(|l| node(&l, 0x7D01))
    {
        for r in items.all(0x7D0A) {
            let Ok(n) = Node::of(r) else { continue };
            if let Some(&i) = entity_id(&n).ok().and_then(|id| hatches.get(&id)) {
                add(Owner::HatchPattern(i), Some(r));
            }
        }
    }
    out
}

/// The folders under `n`, in pre-order, as `read26x` lists them.
fn folders(
    n: &Node<'_>,
    depth: usize,
    next: &mut usize,
    add: &mut dyn FnMut(Owner, Option<Rec<'_>>),
) {
    let Some(sub) = n.node(FOLDER_FOLDERS).ok().flatten() else {
        return;
    };
    for r in sub.all(FOLDER) {
        let Ok(f) = Node::of(r) else { continue };
        if depth >= FOLDER_DEPTH {
            continue;
        }
        add(Owner::LayerFolder(*next), Some(r));
        *next += 1;
        folders(&f, depth + 1, next, add);
    }
}

/// The record of a settings or list owner in `top`.
fn locate(top: &mut Record, owner: Owner) -> Option<&mut Record> {
    fn nth(list: &mut Record, tag: u16, i: usize) -> Option<&mut Record> {
        list.children_mut()?
            .iter_mut()
            .filter(|r| r.tag == tag)
            .nth(i)
    }
    if let Some((_, path)) = LISTS.iter().find(|(o, _)| *o == owner) {
        return top.path_mut(path);
    }
    match owner {
        Owner::CurrentStyle => top.path_mut(&[0x0206, 0x6978, 0x697B, 0x6B6C]),
        Owner::Style(i) => nth(top.path_mut(&[0x0206, 0x6978, 0x6979])?, 0x6B6C, i),
        Owner::Axes => top.path_mut(&[0x01FC, 0x4650, 0x07D0]),
        Owner::ShadowInfo => top.path_mut(&[0x0204, 0x6590]),
        Owner::LineStyle(i) => nth(top.path_mut(&[0x0208, 0x4074, 0x4075])?, 0x4076, i),
        Owner::Scene(i) => {
            nth(top.path_mut(&[0x0207, 0x6D60, 0x6D61])?, 0x7148, i)?.path_mut(&[0x6F54])
        }
        Owner::SceneRendering(i) => {
            nth(top.path_mut(&[0x0207, 0x6D60, 0x6D61])?, 0x7148, i)?.path_mut(&[0x714D, 0x733C])
        }
        Owner::SceneShadowInfo(i) => {
            nth(top.path_mut(&[0x0207, 0x6D60, 0x6D61])?, 0x7148, i)?.path_mut(&[0x714E, 0x6590])
        }
        Owner::SceneAxes(i) => nth(top.path_mut(&[0x0207, 0x6D60, 0x6D61])?, 0x7148, i)?
            .path_mut(&[0x714F, 0x4650, 0x07D0]),
        Owner::HatchPattern(i) => nth(top.path_mut(&[0x0211, 0x7D00, 0x7D01])?, 0x7D0A, i),
        Owner::LayerFolder(i) => {
            let mut next = 0;
            folder_at(
                top.path_mut(&[0x01F8, 0x3A98, 0x3A9B, FOLDER])?,
                i,
                &mut next,
            )
        }
        _ => None,
    }
}

/// The folder in pre-order position `want` under `n`.
fn folder_at<'a>(n: &'a mut Record, want: usize, next: &mut usize) -> Option<&'a mut Record> {
    let kids = n.get_mut(FOLDER_FOLDERS)?.children_mut()?;
    for r in kids.iter_mut().filter(|r| r.tag == FOLDER) {
        if *next == want {
            return Some(r);
        }
        *next += 1;
        if let Some(found) = folder_at(r, want, next) {
            return Some(found);
        }
    }
    None
}

/// Put `attrs` (a `0x05dd` record) into the entity base of `rec`: in
/// the base it has, else in a new one that leads the record (after the
/// `0x0063` word that opens the definition list, `n-attr-definitions`).
fn attach(rec: &mut Record, attrs: Record) {
    let Some(kids) = rec.children_mut() else {
        return;
    };
    match kids.iter_mut().find(|r| r.tag == 0x05DC) {
        Some(base) => {
            base.remove(0x05DD);
            base.set(attrs);
        }
        None => {
            let at = kids.iter().take_while(|r| r.tag == 0x0063).count();
            kids.insert(at, Record::kids(0x05DC, vec![attrs]))
        }
    }
}

/// Write the dictionaries of the settings and lists into `top`; `attrs`
/// gives an owner's `0x05dd` record. Returns how many owners' dictionaries
/// have no record to sit in.
pub(crate) fn write(
    model: &crate::Model,
    top: &mut Record,
    attrs: &mut dyn FnMut(Owner) -> Option<Record>,
) -> usize {
    let mut lost = 0;
    let mut owners: Vec<Owner> = Vec::new();
    for a in &model.dictionaries {
        if a.owner.is_settings() && !owners.contains(&a.owner) {
            owners.push(a.owner);
        }
    }
    for owner in owners {
        let Some(a) = attrs(owner) else { continue };
        match locate(top, owner) {
            Some(r) => attach(r, a),
            None => lost += 1,
        }
    }
    lost
}
