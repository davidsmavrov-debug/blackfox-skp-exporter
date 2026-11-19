//! The releases of the `CArchive` container (SKP_FORMAT §14): 3 to 8 and
//! 2013 to 2020. One grammar ([`crate::file17`]) serves every release it
//! covers; what a release changes is a field the grammar reads or skips,
//! asked of the release by name. A layout never compares releases: it
//! asks for the field (`has_scenes_pre`), and each predicate below is one
//! measured difference of §14.

/// A release of the `CArchive` container, by the major number of the
/// version string its files hold (`{16.0.1}` is 16, `{8.0.1}` is 8).
/// Releases order by that number.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Release(u8);

/// The classes of release 17.3 with their schemas (§3.3, Appendix A), in
/// the order every file lists them: alphabetical, the order of every
/// release. The end marker is not listed.
const BASE: &[(&str, u32)] = &[
    ("CArcCurve", 3),
    ("CAttribute", 0),
    ("CAttributeContainer", 0),
    ("CAttributeNamed", 1),
    ("CBackgroundImage", 10),
    ("CCamera", 5),
    ("CComponent", 11),
    ("CComponentBehavior", 5),
    ("CComponentDefinition", 10),
    ("CComponentInstance", 5),
    ("CConstructionGeometry", 0),
    ("CConstructionLine", 1),
    ("CConstructionPoint", 0),
    ("CCurve", 4),
    ("CDefinitionList", 0),
    ("CDib", 3),
    ("CDimension", 1),
    ("CDimensionLinear", 6),
    ("CDimensionRadial", 2),
    ("CDimensionStyle", 4),
    ("CDrawingElement", 9),
    ("CEdge", 2),
    ("CEdgeUse", 1),
    ("CEntity", 5),
    ("CFace", 3),
    ("CFaceTextureCoords", 4),
    ("CFontManager", 0),
    ("CGroup", 1),
    ("CImage", 1),
    ("CLayer", 2),
    ("CLayerManager", 4),
    ("CLoop", 1),
    ("CMaterial", 12),
    ("CMaterialManager", 4),
    ("CPageList", 1),
    ("CPolyline3d", 0),
    ("CRelationship", 0),
    ("CRelationshipMap", 0),
    ("CRenderingOptions", 36),
    ("CSchemaFile", 1),
    ("CSchemaFilterFile", 0),
    ("CSchemaZipFile", 1),
    ("CSectionPlane", 2),
    ("CShadowInfo", 7),
    ("CSkFont", 1),
    ("CSketchCS", 0),
    ("CSketchUpModel", 26),
    ("CSketchUpPage", 1),
    ("CSkpStyle", 1),
    ("CSkpStyleManager", 2),
    ("CText", 9),
    ("CTextStyle", 5),
    ("CTexture", 6),
    ("CThumbnail", 1),
    ("CVertex", 0),
    ("CViewPage", 12),
    ("CWatermark", 1),
    ("CWatermarkManager", 2),
];

/// A class at a schema, or `None` for a class a release does not list.
type Change = (&'static str, Option<u32>);

/// How each release's version map differs from the next newer one's
/// (§14), newest first. Applied cumulatively downward from [`BASE`].
const DELTAS: &[(Release, &[Change])] = &[
    (
        Release::R2016,
        &[("CEntity", Some(3)), ("CSketchUpModel", Some(25))],
    ),
    (Release::R2015, &[("CSketchUpModel", Some(24))]),
    (Release::R2014, &[("CArcCurve", Some(2))]),
    (
        Release::R2013,
        &[
            ("CComponentInstance", Some(4)),
            ("CSchemaFile", None),
            ("CSchemaFilterFile", None),
            ("CSchemaZipFile", None),
            ("CSketchUpModel", Some(22)),
        ],
    ),
    (Release::R8, &[("CArcCurve", Some(1))]),
    (
        Release::R7,
        &[
            ("CBackgroundImage", Some(9)),
            ("CRenderingOptions", Some(35)),
            ("CViewPage", Some(11)),
        ],
    ),
    (
        Release::R6,
        &[
            ("CAttributeNamed", Some(0)),
            ("CBackgroundImage", Some(1)),
            ("CComponentBehavior", Some(4)),
            ("CRenderingOptions", Some(32)),
            ("CSketchUpModel", Some(18)),
        ],
    ),
    (
        Release::R5,
        &[
            ("CBackgroundImage", None),
            ("CCamera", Some(4)),
            ("CDib", Some(2)),
            ("CDimension", Some(0)),
            ("CDimensionLinear", Some(4)),
            ("CRenderingOptions", Some(28)),
            ("CSkFont", Some(0)),
            ("CSketchUpModel", Some(12)),
            ("CSkpStyle", None),
            ("CSkpStyleManager", None),
            ("CTextStyle", Some(4)),
            ("CTexture", Some(4)),
            ("CViewPage", Some(9)),
            ("CWatermarkManager", None),
        ],
    ),
    (
        Release::R4,
        &[
            ("CComponentInstance", Some(3)),
            ("CDrawingElement", Some(8)),
            ("CRenderingOptions", Some(25)),
        ],
    ),
    (
        Release::R3,
        &[
            ("CAttribute", None),
            ("CAttributeContainer", None),
            ("CAttributeNamed", None),
            ("CCamera", None),
            ("CComponentBehavior", Some(3)),
            ("CConstructionLine", Some(0)),
            ("CCurve", Some(3)),
            ("CEntity", Some(2)),
            ("CFaceTextureCoords", None),
            ("CLayer", Some(1)),
            ("CPageList", None),
            ("CRenderingOptions", Some(22)),
            ("CSketchUpModel", Some(10)),
            ("CThumbnail", Some(0)),
            ("CViewPage", Some(6)),
        ],
    ),
];

/// How each release after 2017 changes the version map of the release
/// before it (§14), oldest first. Applied cumulatively upward from
/// [`BASE`].
const UPS: &[(Release, &[Change])] = &[
    (
        Release::R2018,
        &[
            ("CComponentDefinition", Some(11)),
            ("CComponentInstance", Some(6)),
            ("CRenderingOptions", Some(37)),
            ("CSectionPlane", Some(3)),
            ("CSketchUpModel", Some(28)),
        ],
    ),
    (
        Release::R2019,
        &[
            ("CCustomLineStyle", Some(2)),
            ("CLayer", Some(3)),
            ("CLineStyleManager", Some(1)),
            ("CRenderingOptions", Some(38)),
            ("CSketchUpModel", Some(29)),
        ],
    ),
    (
        Release::R2020,
        &[
            ("CCustomLineStyle", Some(3)),
            ("CLayerGroup", Some(2)),
            ("CLayerManager", Some(7)),
            ("CRenderingOptions", Some(39)),
            ("CSkpStyle", Some(2)),
            ("CViewPage", Some(13)),
        ],
    ),
];

/// The releases whose every layout the grammar holds.
const GRAMMAR: &[Release] = &[
    Release::R3,
    Release::R4,
    Release::R5,
    Release::R6,
    Release::R7,
    Release::R8,
    Release::R2013,
    Release::R2014,
    Release::R2015,
    Release::R2016,
    Release::R2017,
    Release::R2018,
    Release::R2019,
    Release::R2020,
];

/// The display-settings parts of the release numbered `n`
/// ([`Release::rendering_parts`]).
fn rendering_parts_of(n: u8) -> &'static [(usize, usize)] {
    match n {
        ..=3 => &[(0, 45), (56, 87), (121, 125), (129, 174)],
        4 => &[(0, 45), (56, 87), (121, 174)],
        5 => &[(0, 87), (121, 178)],
        6 => &[(0, 179)],
        7 => &[(0, 187)],
        8..=17 => &[(0, 206)],
        // Later releases insert bytes into the 2017 block: 2018 five at
        // +160 (the section fill colour and switch), 2019 one at +7 (the
        // dashes switch, after the jitter one), 2020
        // one at +57 (the hidden-objects switch, after the hidden-geometry
        // one; §14). They are the bytes from 206 on of the extended block
        // ([`RENDERING_EXTRA`]).
        18 => &[(0, 160), (206, 211), (160, 206)],
        19 => &[(0, 7), (211, 212), (7, 160), (206, 211), (160, 206)],
        _ => &[
            (0, 7),
            (211, 212),
            (7, 57),
            (212, 213),
            (57, 160),
            (206, 211),
            (160, 206),
        ],
    }
}

/// The block `full` with the bytes releases after 2017 add to it,
/// those of [`RENDERING_EXTRA`] where it holds none.
fn extended(full: &[u8]) -> Vec<u8> {
    let mut out = full.to_vec();
    if out.len() < 206 {
        out.resize(206, 0);
    }
    let have = out.len() - 206;
    out.extend_from_slice(&RENDERING_EXTRA[have.min(RENDERING_EXTRA.len())..]);
    out
}

/// The bytes that releases after 2017 add to the display-settings block,
/// in the order the releases added them, as the bytes from 206 on of the
/// extended block `rendering_parts` addresses: the defaults of a new
/// model (`ff 00 00 00 00`, `01`, `00`).
pub(crate) const RENDERING_EXTRA: [u8; 7] = [0xff, 0, 0, 0, 0, 0x01, 0x00];

macro_rules! rules {
    ($($rule:ident $name:ident,)*) => {
        /// A layout rule of [`Release`], one per predicate below: the
        /// unit the mutation test (`docs/DEVELOPMENT.md`, feature
        /// `mutation`) makes wrong at one release at a time.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum Rule {
            $(#[doc = concat!("[`Release::", stringify!($name), "`]")] $rule,)*
        }

        #[cfg(feature = "mutation")]
        impl Rule {
            /// Every rule, in the order of the predicates.
            pub const ALL: &'static [Rule] = &[$(Rule::$rule),*];

            /// The name of the rule's predicate.
            pub fn name(self) -> &'static str {
                match self {
                    $(Rule::$rule => stringify!($name),)*
                }
            }
        }
    };
}

rules! {
    HasPids has_pids,
    RootWords root_words,
    RunLen run_len,
    TextureHasByte texture_has_byte,
    InstanceHasGuid instance_has_guid,
    HasSchemaList has_schema_list,
    RenderingParts rendering_parts,
    RenderingTailLen rendering_tail_len,
    SceneHasThumbnail scene_has_thumbnail,
    PhotoTailLen photo_tail_len,
    ThumbnailRunLen thumbnail_run_len,
    HasOptionSets has_option_sets,
    OptionSetsAfterAxes option_sets_after_axes,
    OptionsTailLen options_tail_len,
    AttributeHasTail attribute_has_tail,
    DefinitionHasExtra definition_has_extra,
    HasViewWatermarks has_view_watermarks,
    PhotoHasSize photo_has_size,
    CameraTailLen camera_tail_len,
    FontTailLen font_tail_len,
    HasScreenText has_screen_text,
    HasStyles has_styles,
    TextureImageObject texture_image_object,
    DibHasQuality dib_has_quality,
    DimensionHasFont dimension_has_font,
    DimensionHasTextPosition dimension_has_text_position,
    SceneHasPhoto scene_has_photo,
    DrawbaseHasLocked drawbase_has_locked,
    InstanceHasName instance_has_name,
    CameraIsObject camera_is_object,
    HasAttributes has_attributes,
    SpaceRunLen space_run_len,
    LayerHasWords layer_has_words,
    CurveHasEnds curve_has_ends,
    DefinitionHasBits definition_has_bits,
    HasSelectedScene has_selected_scene,
    DimensionDefaultsRange dimension_defaults_range,
    SceneHasPair scene_has_pair,
    GuideHasTail guide_has_tail,
    StoresDynamicAttributes stores_dynamic_attributes,
    CameraStoresAspect camera_stores_aspect,
    KeepsOwnVersion keeps_own_version,
    HasTailCount has_tail_count,
    SectionHasNames section_has_names,
    LayerHasStyle layer_has_style,
    HasLineStyles has_line_styles,
    SpaceHasGroups space_has_groups,
    LineStyleHasSecondId line_style_has_second_id,
}

/// A rule's answer at a release, as the predicates return it.
trait Answer: Sized {
    /// The answer of `rule` at `at`: the answer itself, except under the
    /// `mutation` feature when that cell is the one mutated
    /// (`mutation::set_mutation`), where it is a wrong one.
    fn mutated(self, rule: Rule, at: Release) -> Self;
}

#[cfg(not(feature = "mutation"))]
impl<T> Answer for T {
    #[inline(always)]
    fn mutated(self, _: Rule, _: Release) -> T {
        self
    }
}

/// A presence the other way round.
#[cfg(feature = "mutation")]
impl Answer for bool {
    fn mutated(self, rule: Rule, at: Release) -> bool {
        self ^ mutation::consult(rule, at)
    }
}

/// A length one byte longer.
#[cfg(feature = "mutation")]
impl Answer for usize {
    fn mutated(self, rule: Rule, at: Release) -> usize {
        self + usize::from(mutation::consult(rule, at))
    }
}

/// A range one byte longer.
#[cfg(feature = "mutation")]
impl Answer for (usize, usize) {
    fn mutated(self, rule: Rule, at: Release) -> (usize, usize) {
        (self.0, self.1 + usize::from(mutation::consult(rule, at)))
    }
}

/// The display-settings parts of a neighbouring release: release 5's
/// from release 6 on, release 6's before.
#[cfg(feature = "mutation")]
impl Answer for &'static [(usize, usize)] {
    fn mutated(self, rule: Rule, at: Release) -> Self {
        if !mutation::consult(rule, at) {
            self
        } else if at >= Release::R6 {
            rendering_parts_of(5)
        } else {
            rendering_parts_of(6)
        }
    }
}

/// The mutation test's hooks (`docs/DEVELOPMENT.md`): one rule made
/// wrong at one release, and the record of which rules were asked at
/// which releases. The state is the process's, not a thread's: a test
/// that sets a mutation runs alone.
#[cfg(feature = "mutation")]
pub mod mutation {
    use std::collections::BTreeSet;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use super::Release;
    pub use super::Rule;

    static MUTATED: Mutex<Option<(Rule, Release)>> = Mutex::new(None);
    static CONSULTED: Mutex<BTreeSet<(Rule, Release)>> = Mutex::new(BTreeSet::new());

    fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Make `rule` answer wrongly when asked at `release`, every other
    /// cell answering rightly; `None` makes every rule right again.
    pub fn set_mutation(cell: Option<(Rule, Release)>) {
        *lock(&MUTATED) = cell;
    }

    /// Every (rule, release) asked since [`reset_consulted`], in order.
    pub fn consulted() -> Vec<(Rule, Release)> {
        lock(&CONSULTED).iter().copied().collect()
    }

    /// Forget the rules asked so far.
    pub fn reset_consulted() {
        lock(&CONSULTED).clear();
    }

    /// Note that `rule` was asked at `at`; whether it is the mutated cell.
    pub(super) fn consult(rule: Rule, at: Release) -> bool {
        lock(&CONSULTED).insert((rule, at));
        *lock(&MUTATED) == Some((rule, at))
    }
}

impl Release {
    pub const R3: Release = Release(3);
    pub const R4: Release = Release(4);
    pub const R5: Release = Release(5);
    pub const R6: Release = Release(6);
    pub const R7: Release = Release(7);
    pub const R8: Release = Release(8);
    pub const R2013: Release = Release(13);
    pub const R2014: Release = Release(14);
    pub const R2015: Release = Release(15);
    pub const R2016: Release = Release(16);
    pub const R2017: Release = Release(17);
    pub const R2018: Release = Release(18);
    pub const R2019: Release = Release(19);
    pub const R2020: Release = Release(20);

    /// Every release of the container, oldest first. Releases 9 to 12
    /// are not offered as save targets, and no file of them is observed (§14).
    pub const ALL: [Release; 14] = [
        Release::R3,
        Release::R4,
        Release::R5,
        Release::R6,
        Release::R7,
        Release::R8,
        Release::R2013,
        Release::R2014,
        Release::R2015,
        Release::R2016,
        Release::R2017,
        Release::R2018,
        Release::R2019,
        Release::R2020,
    ];

    /// The release of a header's version string (`{16.0.1}` → 2016);
    /// `None` for a version that is no release of this container.
    pub fn of_version(s: &str) -> Option<Release> {
        let major = s.strip_prefix('{')?.split('.').next()?.parse::<u8>().ok()?;
        Release::ALL.into_iter().find(|r| r.0 == major)
    }

    /// The release a name picks out: its year (2013 to 2020) or the
    /// number its version string goes by (3 to 8, 13 to 20); `None` for
    /// any other, including years SketchUp has no release for.
    pub fn of_name(n: u32) -> Option<Release> {
        let number = match n {
            2013..=2020 => n - 2000,
            n => n,
        };
        Release::ALL.into_iter().find(|r| u32::from(r.0) == number)
    }

    /// Builds the release of a version number without checking it is
    /// one the container has: for tests of the refusal of a release
    /// the grammar does not hold.
    #[doc(hidden)]
    pub fn unlisted(number: u8) -> Release {
        Release(number)
    }

    /// The major number of the release's version string.
    pub fn number(self) -> u8 {
        self.0
    }

    /// The version string a file of the release is written with: the
    /// one SketchUp Make 2017's save-copy writes for it.
    pub fn version_string(self) -> &'static str {
        match self.0 {
            3 => "{3.0.1}",
            4 => "{4.0.1}",
            5 => "{5.0.1}",
            6 => "{6.0.1}",
            7 => "{7.0.1}",
            8 => "{8.0.1}",
            13 => "{13.0.1}",
            14 => "{14.0.1}",
            15 => "{15.0.1}",
            16 => "{16.0.1}",
            18 => "{18.0.1}",
            19 => "{19.0.1}",
            20 => "{20.0.1}",
            _ => "{17.3.116}",
        }
    }

    /// Whether the grammar holds every layout of the release: a file of
    /// a release it does not hold is refused by its release, never read
    /// as damaged.
    pub fn grammar(self) -> bool {
        GRAMMAR.contains(&self)
    }

    /// The releases the grammar holds, for a message: `17`, `16 and 17`,
    /// `15, 16 and 17`.
    pub(crate) fn grammar_releases() -> String {
        let n: Vec<String> = GRAMMAR.iter().map(|r| r.0.to_string()).collect();
        match n.split_last() {
            Some((last, [])) => format!("release {last}"),
            Some((last, rest)) => format!("releases {} and {last}", rest.join(", ")),
            None => "no release".into(),
        }
    }

    // ---- layouts (§14), one predicate per measured difference

    /// Every entity preamble and the `CLayer` second id hold the
    /// presence-masked persistent id (§5.1); before 2017 the preamble
    /// is the attribute-container pointer alone (§14, 2016).
    pub fn has_pids(self) -> bool {
        (self >= Release::R2017).mutated(Rule::HasPids, self)
    }

    /// The u32 words after the version map (§4): `2, 1200` in releases
    /// 3 to 5, then `6` up to 2016, then the next persistent id and `0`
    /// in 2017 (§14).
    pub fn root_words(self) -> usize {
        match self.0 {
            3..=5 => 2,
            6..=16 => 3,
            _ => 5,
        }
        .mutated(Rule::RootWords, self)
    }

    /// The length in this release of a constant run that is `len17`
    /// bytes long in 2017. Each such run ends in a zero byte that
    /// releases before 2017 do not hold (§14, 2016). The runs: the 15
    /// bytes after the document thumbnail, the 3 zero bytes after a
    /// definition's point, the 70 zero bytes after the axes, the 61
    /// bytes of dimension defaults, the 3 bytes before the font list and
    /// the u32 before the anti-aliased-textures byte. (The list
    /// preambles a 2017 file holds where earlier ones hold a byte less
    /// are entity preambles, [`Release::has_pids`].)
    pub fn run_len(self, len17: usize) -> usize {
        if self >= Release::R2017 {
            len17
        } else {
            len17 - 1
        }
        .mutated(Rule::RunLen, self)
    }

    /// The byte after a texture's attribute-container pointer (§8.1), 0
    /// in every 2017 file; absent before 2017 (§14, 2016).
    pub fn texture_has_byte(self) -> bool {
        (self >= Release::R2017).mutated(Rule::TextureHasByte, self)
    }

    /// The 16-byte identifier after the name of every component
    /// instance, group and image (§7.2), equal to the 2017 save's of the
    /// same session; absent in 2013 and before, where the next record
    /// follows the name (§14, 2013 and 2014).
    pub fn instance_has_guid(self) -> bool {
        (self >= Release::R2014).mutated(Rule::InstanceHasGuid, self)
    }

    /// The classification-schema list after the anti-aliased-textures
    /// byte, the last thing in the file (§4); absent in 2013 and before,
    /// whose files end at that byte (§14, 2013 and 2014).
    pub fn has_schema_list(self) -> bool {
        (self >= Release::R2014).mutated(Rule::HasSchemaList, self)
    }

    /// The parts of the 2017 display-settings block (§10.7, 206 bytes)
    /// that a release's block holds, as 2017 offsets, in file order.
    ///
    /// - From release 8, all of it.
    /// - Release 7 ends it after +186: the back-edges byte (+187) and
    ///   the match-photo settings (+188 to +205) are absent (§14, 7 → 8:
    ///   `corpus/8/box.skp` holds 19 more bytes `00 01 <f64 1.0> 01 <f64
    ///   0.8>` from 0x2215, at the end of the document's block, which
    ///   starts at 0x215a, and of every scene's; all 92 pairs).
    /// - Release 6 ends it after +178, before the x-ray opacity
    ///   (`corpus/6/box.skp`: the block from 0xf49 to 0xffb).
    /// - Releases 5 and before hold +0 to +86 and +121 to +177: no fog
    ///   (+87 to +108), no axes and guides switches (+109 to +120) and
    ///   no watermark switch (+178) (`corpus/5/box.skp`: the block from
    ///   0xe98, the fade of similar components `00 00 00 00 00 00 e0 3f`
    ///   at +77 followed by the two hide bytes and the sky colour `c6 cd
    ///   d0 ff` at 0xeef, the locked colour `ff 00 00 ff` ending it at
    ///   0xf27).
    /// - Release 4 holds +0 to +44, +56 to +86 and +121 to +173: no
    ///   depth cue or endpoints (+45 to +55) and no locked colour (+174
    ///   to +177) either (§14, 4 → 5: `corpus/5/box.skp` holds `00 04 00
    ///   00 00 00 07 00 00 00 00` at 0xec5, the depth-cue and endpoint
    ///   switches and widths, and `ff 00 00 ff` at 0xf24 that
    ///   `corpus/4/box.skp` lacks; **candidate** for the first span's
    ///   place by a byte either way, its edges being zero).
    /// - Release 3 holds +0 to +44, +56 to +86, +121 to +124 and +129
    ///   to +173: the 4 bytes between the sky and ground colours are
    ///   absent too (§14, 3 → 4: `corpus/3/box.skp` block from 0xda4, the
    ///   sky colour `c6 cd d0 ff` followed by the ground colour `d2 d0 b9
    ///   ff`; release 4 holds `00 00 00 00` between them).
    pub fn rendering_parts(self) -> &'static [(usize, usize)] {
        rendering_parts_of(self.0).mutated(Rule::RenderingParts, self)
    }

    /// The length of the release's display-settings block (§10.7).
    pub fn rendering_len(self) -> usize {
        self.rendering_parts().iter().map(|(a, b)| b - a).sum()
    }

    /// The length of the bytes between the document's display settings
    /// and the model (§4): `00 00 00 00 01 00 00 00` from release 6, the
    /// first 4 of them before (§14, 5 → 6: `corpus/5/box.skp` 0xf28 to
    /// 0xf2b, then the model's attribute container).
    pub fn rendering_tail_len(self) -> usize {
        (if self >= Release::R6 { 8 } else { 4 }).mutated(Rule::RenderingTailLen, self)
    }

    /// The release's block of the 2017 block `full`.
    pub(crate) fn rendering_of(self, full: &[u8]) -> Vec<u8> {
        let full = extended(full);
        self.rendering_parts()
            .iter()
            .flat_map(|&(a, b)| full.get(a..b).unwrap_or(&[]).iter().copied())
            .collect()
    }

    /// The 2017 block of the release's block `held`, the bytes it does
    /// not hold taken from `full` (the bytes after 2017's, from
    /// [`RENDERING_EXTRA`] unless `full` holds them).
    pub(crate) fn rendering_in(self, held: &[u8], full: &[u8]) -> Vec<u8> {
        let mut out = extended(full);
        let mut at = 0;
        for &(a, b) in self.rendering_parts() {
            let n = (b - a).min(held.len().saturating_sub(at));
            out[a..a + n].copy_from_slice(&held[at..at + n]);
            at += b - a;
        }
        out
    }

    /// A scene ends with its thumbnail flags and, when they are `01 01`,
    /// the thumbnail (§10.10); before release 8 a scene ends with its
    /// matched-photo pointer (§14, 7 → 8: `corpus/8/match-photo.skp` gains
    /// `00 00` before its selected-scene pointer at 0x11996,
    /// `corpus/8/annotation-order.skp` `01 01 01 <len> <PNG>` per scene,
    /// at 0x110bda and 0x112679).
    pub fn scene_has_thumbnail(self) -> bool {
        (self >= Release::R8).mutated(Rule::SceneHasThumbnail, self)
    }

    /// The length of the zero run that ends a matched photo (§10.10): 36
    /// bytes from release 8, 32 before (§14, 7 → 8: `corpus/7/match-photo.skp`
    /// gains 4 zero bytes in the `CBackgroundImage`, 0x1197d of the
    /// release 7 file, where the run is 40 bytes (46 in release 8, with
    /// the last z and the thumbnail flags); **candidate** for the place within the zero run,
    /// which a diff cannot fix).
    pub fn photo_tail_len(self) -> usize {
        (if self >= Release::R8 { 36 } else { 32 }).mutated(Rule::PhotoTailLen, self)
    }

    /// The length of the constant run after the document thumbnail (§4):
    /// 15 bytes in 2017, 14 from release 7 to 2016, 10 before (§14, 6 →
    /// 7: `01 00 00 00` and six zero bytes before the empty string in
    /// all 92 `v8run/6` files and in `v8run/4` and `v8run/5`, ten zero
    /// bytes in `v8run/7`; `01 00 00 00` and three zero bytes in all 92
    /// of `v8run/3`).
    pub fn thumbnail_run_len(self) -> usize {
        match self.0 {
            ..=3 => 7,
            4..=6 => 10,
            _ => self.run_len(15),
        }
        .mutated(Rule::ThumbnailRunLen, self)
    }

    /// The document's option sets (§10.5: units, the slideshow, the
    /// scene transitions), with their version before them and the
    /// model's attribute container and preview image after them (§4):
    /// absent before release 7, where the camera follows the empty
    /// string after the thumbnail run (§14, 6 → 7).
    pub fn has_option_sets(self) -> bool {
        (self >= Release::R7).mutated(Rule::HasOptionSets, self)
    }

    /// Where the option sets are before release 7: inside the zero run
    /// that follows the axes (§4), after its first 63 bytes (§14, 6 → 7:
    /// `corpus/6/box.skp` holds the axes' shown flag, their last byte, at
    /// 0xf6b4, 63 zero bytes to 0xf6f3, the version `2` at 0xf6f4, the 4
    /// sets, and 6 zero bytes
    /// before the dimension font at 0xf98a; release 7 holds the 69 bytes
    /// of the run in one piece; the same in all 92 files of releases 4
    /// to 6). **Candidate** for the split of the run, all zero.
    pub fn option_sets_after_axes(self) -> bool {
        (!self.has_option_sets()).mutated(Rule::OptionSetsAfterAxes, self)
    }

    /// The zero bytes after the option sets before release 7
    /// ([`Release::option_sets_after_axes`]): 6 in releases 4 to 6, 4 in
    /// release 3 (§14, 3 → 4: `corpus/3/box.skp` `00 00 00 00` from
    /// 0xd654, after the sets' last empty key, then the dimension font).
    pub fn options_tail_len(self) -> usize {
        (if self >= Release::R4 { 6 } else { 4 }).mutated(Rule::OptionsTailLen, self)
    }

    /// The u32 after the entries of an attribute dictionary (§10.5,
    /// `CAttributeNamed` schema 1); absent before release 7, where the
    /// next object follows the empty key (§14, 6 → 7: `corpus/6/box.skp`
    /// `ff fe ff 00 09 80` at 0x1057 against `ff fe ff 00 00 00 00 00
    /// 07 80` at 0x2246 of the release 7 file).
    pub fn attribute_has_tail(self) -> bool {
        (self >= Release::R7).mutated(Rule::AttributeHasTail, self)
    }

    /// The u32 between a definition's behaviour bits and its kind
    /// (§7.1, `Behaviour::extra`); absent before release 7 (§14, 6 → 7:
    /// after the timestamp the template figure's definition of
    /// `corpus/6/box.skp` holds `01`, 32 zero bytes, `01` and then the
    /// kind, 4 bytes fewer than release 7's; a group's definition in
    /// `box-group.skp` (pinned external evidence, `years-run.sha256`)
    /// ends `01 00 00 00`, its kind, in both).
    pub fn definition_has_extra(self) -> bool {
        (self >= Release::R7).mutated(Rule::DefinitionHasExtra, self)
    }

    /// The current view's watermark list and the anti-aliased-textures
    /// byte (§4), between the style-changed word and the zero run at the
    /// end of the document; absent before release 7, whose files end
    /// with the style-changed u32 and 3 zero bytes (§14, 6 → 7: the last
    /// 7 bytes of every `v8run/6` file, `01 00 00 00` in the three whose
    /// style is changed, against 12 in release 7, whose last byte is the
    /// anti-aliasing flag, 0 in `corpus/7/render-aa-off.skp` only, at 0x115f4). **Candidate**
    /// for which of the zero bytes are the run.
    pub fn has_view_watermarks(self) -> bool {
        (self >= Release::R7).mutated(Rule::HasViewWatermarks, self)
    }

    /// A matched photo (§10.10) holds a zero word, its image, its width
    /// and height, a zero word and its time after the path, and a zero
    /// run after the points; before release 7 (`CBackgroundImage` 1) it
    /// holds the path, the byte `01`, the scale and the points, and then
    /// the image (§14, 6 → 7: `corpus/6/match-photo.skp`, the object at
    /// 0xf8ce: the path ends at 0xf933, `01`, f64 1.0, 12 points from
    /// 0xf941, the `CDib` at 0xfa61).
    pub fn photo_has_size(self) -> bool {
        (self >= Release::R7).mutated(Rule::PhotoHasSize, self)
    }

    /// The length of the bytes after a camera's description (§10.6): 33
    /// from release 6 (`CCamera` 5), holding the two-point-perspective
    /// byte at +8 and the 2D zoom and pan from +9; 8 zero bytes before
    /// (§14, 5 → 6: `corpus/5/box.skp` camera at 0xdf2, description at
    /// 0xe8a, the display settings' preamble at 0xe96).
    pub fn camera_tail_len(self) -> usize {
        (if self >= Release::R6 { 33 } else { 8 }).mutated(Rule::CameraTailLen, self)
    }

    /// The length of the bytes after a font's name (§10.1): 15 from
    /// release 6 (`CSkFont` 1): bold, italic, the u32 size, the
    /// model-height byte and the f64 height; the first 6 before (§14, 5
    /// → 6: `corpus/5/box.skp` "Tahoma" ending at 0xf8b8, followed by
    /// `00 00 0c 00 00 00` and the dimension defaults `00 01 05 00 00 00
    /// 0a …`; `corpus/6/box.skp` holds `00` and the f64 10 between them,
    /// from 0xf9af).
    pub fn font_tail_len(self) -> usize {
        (if self >= Release::R6 { 15 } else { 6 }).mutated(Rule::FontTailLen, self)
    }

    /// The screen-text settings (§10.11): the screen text colour that
    /// ends the text defaults (+18 to +21) and the screen font after
    /// them; absent before release 6, whose text defaults are 18 bytes
    /// followed by the font list (§14, 5 → 6: `corpus/5/box.skp` text
    /// defaults from 0xf8fc, `00 00 01 00 00 00 47 0a` from 0xf90e).
    pub fn has_screen_text(self) -> bool {
        (self >= Release::R6).mutated(Rule::HasScreenText, self)
    }

    /// The styles (§10.8) and everything after them: the current view's
    /// matched photo, the style list, the active and current styles, the
    /// style-changed word and the zero run that ends the document (§4);
    /// absent before release 6, whose files end with the font list
    /// (§14, 5 → 6: `corpus/5/box.skp` is 0xf916 bytes long, ending
    /// with the font reference `47 0a`; no `CSkpStyle` is in the map). A scene's
    /// display settings are not followed by a style pointer either
    /// (`corpus/5/annotation-order.skp`: the first scene's block ends
    /// with the locked colour at 0xc24a3 and the shadow settings follow,
    /// the city name at 0xc24ae; `corpus/6/annotation-order.skp` ends
    /// the block at 0xc345e and declares `CSkpStyle` at 0xc345f).
    pub fn has_styles(self) -> bool {
        (self >= Release::R6).mutated(Rule::HasStyles, self)
    }

    /// A texture (§8.1) holds an attribute-container pointer and a
    /// pointer to its image, a `CDib` object; before release 6
    /// (`CTexture` 4) it holds neither, but a byte `01` and the image's
    /// fields in place (§14, 5 → 6: `corpus/5/annotation-order.skp` after
    /// the material name ending at 0x41fb: `01` textured, `01`, the JPEG
    /// subtype `01 00 00 00`, the length 17984 and the JPEG at 0x4205;
    /// release 6 holds `01 00 00 03 80 01 00 00 00 …` there, the JPEG at
    /// 0x42e0 of `corpus/6/annotation-order.skp`).
    pub fn texture_image_object(self) -> bool {
        (self >= Release::R6).mutated(Rule::TextureImageObject, self)
    }

    /// A JPEG image's quality u32 after its data (§8.2); absent before
    /// release 6 (`CDib` 2), where the texture's applied size follows
    /// the data (§14, 5 → 6: `corpus/5/annotation-order.skp`, the f64 120
    /// at 0x8845 right after the 17984 bytes; `corpus/6/annotation-order.skp`
    /// holds `46 00 00 00`, 70, first, at 0x8920).
    pub fn dib_has_quality(self) -> bool {
        (self >= Release::R6).mutated(Rule::DibHasQuality, self)
    }

    /// A dimension's font, alignment byte and arrow word after its text
    /// (§10.11, `CDimension` 1), in linear and radial dimensions; absent
    /// before release 6, where the anchors (or the measured edge) follow
    /// the text (§14, 5 → 6: `corpus/5/annotation-order.skp` the dimension
    /// at 0xc1a9e: empty text at 0xc1ac0, the first anchor `02 00 00 00
    /// 04 00 00 00` at 0xc1ac4; release 6 holds a font pointer `2b 6d`,
    /// `00` and `01 00 00 00` between them, at 0xc2a2a of
    /// `corpus/6/annotation-order.skp`).
    pub fn dimension_has_font(self) -> bool {
        (self >= Release::R6).mutated(Rule::DimensionHasFont, self)
    }

    /// The text-position word that ends a linear dimension (§10.11,
    /// `CDimensionLinear` 6); absent before release 6 (§14, 5 → 6:
    /// `corpus/5/annotation-order.skp` the dimension ends with its offset
    /// f64 at 0xc1b50 and a zero f64, the next object at 0xc1b60; release
    /// 6 holds `03 00 00 00` after them, at 0xc2ad1).
    pub fn dimension_has_text_position(self) -> bool {
        (self >= Release::R6).mutated(Rule::DimensionHasTextPosition, self)
    }

    /// A scene's matched-photo pointer after its two f64 (§10.10);
    /// absent before release 6, which has no `CBackgroundImage`, and
    /// whose scenes hold one more byte, `01` in every scene observed,
    /// before the animation byte (§14, 5 → 6: `corpus/5/annotation-order.skp`
    /// the second scene's `01 01` at 0xc27d6, two f64 −1 and the selected
    /// scene `7e 6d` at 0xc27e8; every scene of the 92 release 5 files holds `01 01` before its
    /// two f64; release 6 `01`, the two f64 and `00 00`). **Candidate**
    /// for which of the two bytes is the animation flag.
    pub fn scene_has_photo(self) -> bool {
        (self >= Release::R6).mutated(Rule::SceneHasPhoto, self)
    }

    /// The locked byte of a drawing element's base (§5.2), before its
    /// layer; absent before release 5 (`CDrawingElement` 8), where an
    /// element cannot be locked (§14, 4 → 5:
    /// `corpus/5/annotation-order.skp` the instance at 0xa386a holds `00
    /// 01 01` and five zero bytes up to its layer pointer `96 57` at
    /// 0xa3905, `corpus/4/annotation-order.skp` the instance at 0xa0547
    /// `00 00 01 01` and four, the pointer at 0xa05e1; one byte fewer
    /// before every layer pointer, the box's first edge at 0x1682 of
    /// release 5's and 0x1671 of release 4's).
    pub fn drawbase_has_locked(self) -> bool {
        (self >= Release::R5).mutated(Rule::DrawbaseHasLocked, self)
    }

    /// The name string after a component instance's, group's or image's
    /// transform (§7.2); absent before release 5 (`CComponentInstance`
    /// 3), where the next record follows the transform (§14, 4 → 5:
    /// `corpus/4/annotation-order.skp` the instance at 0xa0547, its
    /// transform ending at 0xa064b and six zero bytes after it;
    /// `corpus/5/annotation-order.skp` the instance at 0xa386a holds an
    /// empty string `ff fe ff 00` there, from 0xa396f, and the same six).
    pub fn instance_has_name(self) -> bool {
        (self >= Release::R5).mutated(Rule::InstanceHasName, self)
    }

    /// A camera (§10.6) is a `CCamera` object a pointer names; before
    /// release 4, which has no such class, its fields are held in place
    /// and end after +128 of the body: no aspect ratio, description or
    /// tail (§14, 3 → 4: `corpus/3/box.skp` the document's camera from
    /// 0xd23, after the empty string at 0xd1f, and the display settings'
    /// face style `02 00 00 00` at 0xda4; release 4 declares `CCamera` at
    /// 0xdf2). The second triple of the body is the unit view direction
    /// there, not the target (0xd3b: (−0.123, 0.952, −0.282), the
    /// direction from the eye to the release 4 file's target).
    pub fn camera_is_object(self) -> bool {
        (self >= Release::R4).mutated(Rule::CameraIsObject, self)
    }

    /// The attribute-container pointer that opens every entity preamble
    /// (§5.1); absent before release 4, which has no attribute classes
    /// (§14, 3 → 4: the display settings of `corpus/3/box.skp` begin at
    /// 0xda4 with the face style, no pointer before them).
    pub fn has_attributes(self) -> bool {
        (self >= Release::R4).mutated(Rule::HasAttributes, self)
    }

    /// The zero run after a definition's point (§7.1) and the one before
    /// the font list (§10.11): 3 bytes in 2017, 2 from release 4 to 2016
    /// (§14 R2), absent in release 3. With
    /// [`Release::definition_has_bits`], a release 3 definition holds 35
    /// bytes after its timestamp, 3 fewer than release 4's:
    /// `corpus/3/box.skp` the template figure's definition, from the
    /// timestamp at 0xa47d to the thumbnail class at 0xa4a4;
    /// `corpus/4/box.skp` holds 38, from 0xc337 to 0xc361. **Candidate**
    /// for which of the zero bytes are absent. The lists of an entity
    /// container and the document's scene, line-style and style lists
    /// open with an entity preamble instead, whose length follows
    /// [`Release::has_attributes`] and [`Release::has_pids`] the same
    /// way (`corpus/3/box.skp`: the model's drawing base `00 00 00 01 01
    /// 00 00 00 00` from 0xe25 is followed by the material count at
    /// 0xe2e).
    pub fn space_run_len(self) -> usize {
        if self >= Release::R4 {
            self.run_len(3)
        } else {
            0
        }
        .mutated(Rule::SpaceRunLen, self)
    }

    /// The u16 after a layer's hidden byte and the u32 that ends a layer
    /// (§8.3, `CLayer` 2); absent before release 4 (`CLayer` 1) (§14, 3
    /// → 4: `corpus/3/box.skp` "Layer0" at 0xe84 followed by `00` and the
    /// internal name; the layer ends 17 bytes after its empty string,
    /// release 4's 21).
    pub fn layer_has_words(self) -> bool {
        (self >= Release::R4).mutated(Rule::LayerHasWords, self)
    }

    /// Two edge pointers at the head of a curve (§6.3), before its byte
    /// and member count; present before release 4 only (`CCurve` 3),
    /// where both name the edge the curve is read in for a one-edge
    /// curve (§14, 3 → 4: `corpus/3/box.skp` the curve declared at 0xfa2 holds `0d
    /// 00 0d 00` at 0xfae, slot 13 being the edge at 0xf4f that holds it, then
    /// `00 01 00 00 00`; release 4 `00 00 00 01 00 00 00`, the preamble's
    /// pointer, the byte and the count).
    ///
    /// An arc curve holds them too, before its head: `v8run/3/arc.skp`
    /// (pinned external evidence, `years-run.sha256`) the arc at 0xccc7 holds `f2 09`, a reference to an edge read
    /// before, and `0c 80`, a new edge written in the curve, at 0xccd6.
    pub fn curve_has_ends(self) -> bool {
        (self < Release::R4).mutated(Rule::CurveHasEnds, self)
    }

    /// The behaviour byte of a definition (§7.1: always faces the
    /// camera, shadows face the sun), before its kind; absent before
    /// release 4 (`CComponentBehavior` 3) (§14, 3 → 4: the template
    /// figure's definition of `corpus/3/box.skp`, which faces the camera
    /// in release 4 with the byte `01`, holds `01` and 34 zero bytes
    /// after its timestamp at 0xa47d, the thumbnail at 0xa4a4).
    pub fn definition_has_bits(self) -> bool {
        (self >= Release::R4).mutated(Rule::DefinitionHasBits, self)
    }

    /// The selected-scene pointer after the scene list (§4); absent
    /// before release 4, which has no `CPageList` (§14, 3 → 4:
    /// `corpus/3/box.skp` the scene count `00 00 00 00` at 0xd317 is
    /// followed by the model axes' material pointer and hidden byte `00
    /// 00 00` and the shadow bytes `01 01` at 0xd31e).
    pub fn has_selected_scene(self) -> bool {
        (self >= Release::R4).mutated(Rule::HasSelectedScene, self)
    }

    /// The bytes of the 2017 dimension defaults (§10.11, 61 bytes) a
    /// release holds: all but the last zero byte from release 4 (§14 R2;
    /// 60), and the first 58 in release 3, two zero bytes fewer at the
    /// end (§14, 3 → 4: `corpus/3/box.skp` `00 01 05 00 00 00 0a …` after
    /// the dimension font's 6 bytes at 0xd67b, ending `ff 03 00 00 00`
    /// before the text font at 0xd6b5; release 4 holds `ff 03 00 00 00 00
    /// 00`).
    /// The run before the font list is absent in release 3 too (the font
    /// count follows the text defaults).
    pub fn dimension_defaults_range(self) -> (usize, usize) {
        if self >= Release::R4 {
            (0, self.run_len(61))
        } else {
            (0, 58)
        }
        .mutated(Rule::DimensionDefaultsRange, self)
    }

    /// The two f64 after a scene's animation byte (§10.10, −1 and −1
    /// in every scene observed); absent before release 4 (`CViewPage` 6)
    /// (§14, 3 → 4: `corpus/3/match-photo.skp` the scene at 0xd440 ends
    /// with `01 01 01` at 0xd55b and the model axes follow, `00 00 00`
    /// and the shadow bytes `01 01` at 0xd561; `corpus/4/match-photo.skp`
    /// holds the two f64 −1 at 0xf4c5 and the selected-scene pointer `47
    /// 0a` at 0xf4d5 after `01 01 01` at 0xf4c2).
    pub fn scene_has_pair(self) -> bool {
        (self >= Release::R4).mutated(Rule::SceneHasPair, self)
    }

    /// A guide line's bounds and the 4 bytes that end it (§10.10,
    /// `CConstructionLine` 1); absent before release 4
    /// (`CConstructionLine` 0), whose guide line ends with its direction
    /// (§14, 3 → 4: `corpus/3/annotation-order.skp` the line at 0xa2812:
    /// drawing base from 0xa2829, the point from 0xa2832, the direction
    /// (1, 0, 0) from 0xa284a, then the next object at 0xa2862;
    /// `corpus/4/annotation-order.skp` holds the bounds ∓1e30 at 0xbdf8b
    /// and 0xbdf93 and 4 zero bytes after the direction). A release 3
    /// line reads as unbounded.
    pub fn guide_has_tail(self) -> bool {
        (self >= Release::R4).mutated(Rule::GuideHasTail, self)
    }

    /// Whether SketchUp writes dynamic-component attributes into a
    /// file of the release: not before release 7 (§14, content not
    /// layout: `corpus/4/box.skp` to `corpus/6/box.skp` hold no
    /// `dynamic_attributes` dictionary, `corpus/7/box.skp`,
    /// `corpus/8/box.skp` and `corpus/2013/box.skp` one).
    pub fn stores_dynamic_attributes(self) -> bool {
        (self >= Release::R7).mutated(Rule::StoresDynamicAttributes, self)
    }

    /// Whether SketchUp writes a camera's aspect ratio (+129 of the
    /// body, §10.6): from release 6 (§14, content not layout: 0 in both
    /// cameras of `corpus/3/match-photo.skp` to `corpus/5/match-photo.skp`,
    /// 4:3 in `corpus/6/match-photo.skp`).
    pub fn camera_stores_aspect(self) -> bool {
        (self >= Release::R6).mutated(Rule::CameraStoresAspect, self)
    }

    /// Whether the release's files carry any version string of their
    /// own: release 2017 only (`{17.3.116}` in `corpus/2017/box.skp`);
    /// every earlier release holds one fixed string (`{16.0.1}` in
    /// `corpus/2016/box.skp`, `{8.0.1}` in `corpus/8/box.skp`; the
    /// version-map test of this module).
    pub fn keeps_own_version(self) -> bool {
        (self == Release::R2017).mutated(Rule::KeepsOwnVersion, self)
    }

    /// The u32 after the classification-schema list, the last four bytes
    /// of the file, from 2018: 1 in a model with no section plane, 2
    /// with one, 3 with two (§14, 2017 → 2018).
    pub fn has_tail_count(self) -> bool {
        (self >= Release::R2018).mutated(Rule::HasTailCount, self)
    }

    /// The name and the symbol a section plane holds after its plane,
    /// from 2018 (`corpus/2018/section-plane.skp`: `Section 1`, `1`).
    pub fn section_has_names(self) -> bool {
        (self >= Release::R2018).mutated(Rule::SectionHasNames, self)
    }

    /// The u16 that ends a layer from 2019 (`corpus/2019/box.skp`: two
    /// zero bytes more after the layer's 22 zero bytes, 0x2767; the
    /// layer's line style, **candidate** for its place and its meaning,
    /// all zero in the default layer).
    pub fn layer_has_style(self) -> bool {
        (self >= Release::R2019).mutated(Rule::LayerHasStyle, self)
    }

    /// The list of dash patterns ahead of the style list, from 2019
    /// (`corpus/2019/box.skp` from 0x1272e: 12 `CCustomLineStyle`
    /// objects, `Solid Basic` to `Long-dash double-dash`).
    pub fn has_line_styles(self) -> bool {
        (self >= Release::R2019).mutated(Rule::HasLineStyles, self)
    }

    /// What follows the active layer in the model and in every
    /// definition from 2020: three zero bytes, an empty string and nine
    /// zero bytes (`corpus/2020/layers.skp`: 16 bytes after the active
    /// layer's `13 00`; **candidate** for its meaning, all empty or zero
    /// in the corpus, where the layer-group class is declared and no
    /// object of it is stored).
    pub fn space_has_groups(self) -> bool {
        (self >= Release::R2020).mutated(Rule::SpaceHasGroups, self)
    }

    /// The zero word and second id a dash pattern holds after its
    /// preamble in 2019 and not from 2020 (`corpus/2019/box.skp` 0x12747,
    /// `corpus/2020/box.skp`: `00 00 03 f5 12` twice against once).
    pub fn line_style_has_second_id(self) -> bool {
        (self < Release::R2020).mutated(Rule::LineStyleHasSecondId, self)
    }

    /// The release's version map without its end marker: the 2017 map
    /// with every newer release's changes applied (§14), in the order a
    /// file lists the classes.
    pub fn version_map(self) -> Vec<(&'static str, u32)> {
        let mut map: Vec<(&'static str, u32)> = BASE.to_vec();
        let ups = UPS.iter().filter(|(r, _)| *r <= self);
        let downs = DELTAS.iter().filter(|(r, _)| *r >= self);
        for (_, changes) in ups.chain(downs) {
            for (class, schema) in changes.iter() {
                let at = map.binary_search_by(|(c, _)| c.cmp(class));
                match (at, schema) {
                    (Ok(i), Some(s)) => map[i].1 = *s,
                    (Ok(i), None) => {
                        map.remove(i);
                    }
                    (Err(i), Some(s)) => map.insert(i, (class, *s)),
                    (Err(_), None) => {}
                }
            }
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_name_their_release() {
        assert_eq!(Release::of_version("{16.0.1}"), Some(Release::R2016));
        assert_eq!(Release::of_version("{17.3.116}"), Some(Release::R2017));
        assert_eq!(Release::of_version("{8.0.1}"), Some(Release::R8));
        assert_eq!(Release::of_version("{3.0.1}"), Some(Release::R3));
        for other in [
            "{2.0.1}",
            "{9.0.1}",
            "{12.0.1}",
            "{21.0.1}",
            "{26.1.189}",
            "17.3",
            "",
        ] {
            assert_eq!(Release::of_version(other), None, "{other}");
        }
        for r in Release::ALL {
            assert_eq!(Release::of_version(r.version_string()), Some(r));
        }
        assert!(Release::R8 < Release::R2013 && Release::R2016 < Release::R2017);
    }

    #[test]
    fn the_base_and_the_deltas_are_in_order() {
        assert!(BASE.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(DELTAS.windows(2).all(|w| w[0].0 > w[1].0));
        assert_eq!(Release::R2017.version_map(), BASE);
    }

    /// Each release's version map is the one its files list, order
    /// included.
    #[test]
    fn the_version_map_of_every_release_is_its_files() {
        let corpus = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
        for r in Release::ALL {
            let dir = if r.number() >= 13 {
                format!("20{}", r.number())
            } else {
                r.number().to_string()
            };
            let d = std::fs::read(corpus.join(&dir).join("box.skp")).unwrap();
            let listed = crate::inventory(&d).unwrap();
            let map: Vec<(String, u32)> = r
                .version_map()
                .into_iter()
                .map(|(c, s)| (c.to_string(), s))
                .collect();
            assert_eq!(map, listed, "release {dir}");
            let (version, _) = crate::header_info(&d).unwrap();
            assert_eq!(Release::of_version(&version), Some(r), "release {dir}");
        }
    }
    /// Every layout predicate carries a [`Rule`]: a `pub fn` of
    /// [`Release`] taking `self` and answering a presence, a length, a
    /// range or the display-settings parts has a rule of the same name,
    /// so the mutation test (`docs/DEVELOPMENT.md`) cannot miss it.
    #[cfg(feature = "mutation")]
    #[test]
    fn every_layout_predicate_has_a_rule() {
        const NOT_LAYOUT: &[&str] = &[
            "number",
            "grammar",
            "version_string",
            "rendering_len",
            "version_map",
        ];
        let source = include_str!("release.rs");
        let source = &source[..source.find("#[cfg(test)]\nmod tests").unwrap()];
        let names: Vec<&str> = Rule::ALL.iter().map(|r| r.name()).collect();
        let mut seen = 0;
        for line in source.lines() {
            let Some(rest) = line.trim_start().strip_prefix("pub fn ") else {
                continue;
            };
            let Some((name, after)) = rest.split_once('(') else {
                continue;
            };
            if !after.starts_with("self") {
                continue;
            }
            let ret = after.split_once("->").map_or("", |(_, r)| r.trim());
            let layout = ["bool", "usize", "(usize, usize)", "&'static ["]
                .iter()
                .any(|t| ret.starts_with(t));
            if !layout || NOT_LAYOUT.contains(&name) {
                continue;
            }
            seen += 1;
            assert!(names.contains(&name), "{name} has no Rule");
        }
        assert_eq!(seen, names.len(), "a Rule has no predicate");
    }
}
