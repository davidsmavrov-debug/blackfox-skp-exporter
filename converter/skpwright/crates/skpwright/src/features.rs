//! Model features that appear only in files saved by releases after 2017
//! (spec §16.17 onward): hatch patterns, layer folders and line styles,
//! environments, physically based materials, the coordinate reference
//! system, the newer display options and the newer scene and entity
//! properties. Everything here is read from, and written to, the 2026
//! container; a stream-container file holds none of it, so a model read
//! from one carries an empty [`Features`].

/// The post-2017 features of a model. Empty (`is_empty`) for every model
/// read from a stream-container file and for a 2026 file that uses none.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Features {
    /// The model's hatch patterns, in the order of the file's hatch list
    /// (`0x0211`). Section planes, materials, instances and the rendering
    /// options name one by its index here.
    pub hatch_patterns: Vec<HatchPattern>,
    /// `(material index, hatch pattern index)`: the materials that use a
    /// hatch pattern (`0x32cf` in the material record).
    pub material_hatch: Vec<(usize, usize)>,
    /// `(index into Model::instances, hatch pattern index)`: the groups
    /// and component instances that carry a hatch pattern (`0x1969` in
    /// the instance record).
    pub instance_hatch: Vec<(usize, usize)>,
    /// The hatch settings of the rendering options (`0x7388 > 0x7d0a`),
    /// `None` while they hold the defaults.
    pub render_hatch: Option<RenderHatch>,
    /// The layer folders at the top of the layer tree (`0x3a9b > 0x3e80`),
    /// in file order. Layers stay one flat list in `Model::layers`; a
    /// folder names the layers directly in it by their index there. A
    /// layer in no folder is listed nowhere.
    pub layer_folders: Vec<LayerFolder>,
    /// The current-settings style record (`0x6978 > 0x697b`) when it is
    /// not the plain copy of the active style: another name or description
    /// (a renamed style leaves it behind), or the modified flag set.
    pub current_style: Option<CurrentStyle>,
    /// `(index into Model::layers, index into the file's line style list)`:
    /// the layers that name a line style (`0x3c91`). The list holds the
    /// twelve stock styles, in the order SketchUp lists them (index 0 is
    /// "Solid Basic"). A layer that is absent here names none.
    pub layer_line_styles: Vec<(usize, usize)>,
    /// `(index into Model::layers, value)`: the layers whose page
    /// behaviour (`0x3c90`, a `u32`) is not 0. Only 0 and 1 are values
    /// SketchUp's authoring offers; the meaning of 1 is a candidate
    /// (non-default behaviour when pages are added). Other values are
    /// kept as read.
    pub layer_page_behaviour: Vec<(usize, u32)>,
    /// The model's coordinate reference system (`0x0218 > 0x7e90`); `None`
    /// while the record is empty.
    pub crs: Option<Crs>,
    /// `(index into Model::materials, properties)`: the materials whose
    /// physically based properties differ from a plain material's (every
    /// default is listed on [`PbrMaterial`]). The properties are the
    /// `mat:pbrMR` block of the material's `material.xml` and a few
    /// attributes of its `mat:material` element; the material stays
    /// `Solid` or `Textured` as before.
    pub material_pbr: Vec<(usize, PbrMaterial)>,
    /// The environments, in the order of the file's environment list
    /// (`0x0210 > 0x7918 > 0x7919`).
    pub environments: Vec<Environment>,
    /// Index into `environments` of the current environment (`0x791a`);
    /// `None` when the file names none.
    pub current_environment: Option<usize>,
    /// `(index into Model::scenes, settings)`: the pages whose environment
    /// settings differ from a page that saves no environment choice (the
    /// default: the page keeps the "environment" property, names no
    /// environment and has both flags clear).
    pub scene_environment: Vec<(usize, PageEnvironment)>,
    /// `(index into Model::scenes, extras)`: the pages that hold more than
    /// the typed `Scene` fields carry and the defaults give (the layer
    /// folders they hide, a display name that is not derived, their own
    /// transition time and delay).
    pub scene_pages: Vec<(usize, PageExtras)>,
    /// The snaps of the model and of its definitions (`0x139d > 0x7530`),
    /// in file order, the model's before the definitions'.
    pub snaps: Vec<Snap>,
    /// The definitions made by a procedure (`0x1588`), at most one each.
    pub procedures: Vec<Procedure>,
    /// The procedure kinds the model names (`0x0215 > 0x7d82`).
    pub procedure_kinds: Vec<ProcedureKind>,
}

impl Features {
    /// Whether the model uses none of the features.
    pub fn is_empty(&self) -> bool {
        *self == Features::default()
    }

    /// The kinds of feature in use, each once, in the order of [`Feature`].
    pub fn used(&self) -> Vec<Feature> {
        let mut out = Vec::new();
        let mut add = |f: Feature, on: bool| {
            if on {
                out.push(f);
            }
        };
        add(Feature::HatchPatterns, !self.hatch_patterns.is_empty());
        add(Feature::MaterialHatch, !self.material_hatch.is_empty());
        add(Feature::InstanceHatch, !self.instance_hatch.is_empty());
        add(Feature::RenderHatch, self.render_hatch.is_some());
        add(Feature::LayerFolders, !self.layer_folders.is_empty());
        add(Feature::CurrentStyle, self.current_style.is_some());
        add(
            Feature::ModifiedStyle,
            self.current_style.as_ref().is_some_and(|c| c.modified),
        );
        add(Feature::LayerLineStyles, !self.layer_line_styles.is_empty());
        add(
            Feature::LayerPageBehaviour,
            !self.layer_page_behaviour.is_empty(),
        );
        add(Feature::Crs, self.crs.is_some());
        add(Feature::PbrMaterials, !self.material_pbr.is_empty());
        add(
            Feature::Environments,
            !self.environments.is_empty() || self.current_environment.is_some(),
        );
        add(
            Feature::PageEnvironments,
            !self.scene_environment.is_empty(),
        );
        add(Feature::PageExtras, !self.scene_pages.is_empty());
        add(Feature::Snaps, !self.snaps.is_empty());
        add(Feature::Procedures, !self.procedures.is_empty());
        add(Feature::ProcedureKinds, !self.procedure_kinds.is_empty());
        out
    }

    /// The lowest revision of the 2026 container (§16.27) that holds every
    /// feature of *these fields* in use: [`Revision::R17`](crate::Revision::R17)
    /// when none needs more, else the newest [`Feature::since`] among them.
    /// Two features live outside `Features`, on the section planes and in
    /// the display settings ([`Feature::SectionHatch`],
    /// [`Feature::DefaultSectionFillStyle`]); this method cannot see them,
    /// so for a whole model use
    /// [`Model::min_revision`](crate::Model::min_revision).
    pub fn min_revision(&self) -> crate::Revision {
        self.used()
            .into_iter()
            .map(Feature::since)
            .max()
            .unwrap_or(crate::Revision::R17)
    }
}

/// A kind of post-2017 content, for the revision that holds it
/// ([`since`](Self::since)) and the notes of a writer that leaves it out.
/// Most are fields of [`Features`]; [`SectionHatch`](Self::SectionHatch)
/// and [`DefaultSectionFillStyle`](Self::DefaultSectionFillStyle) are on
/// the section planes and the display settings
/// ([`Model::features_used`](crate::Model::features_used)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Feature {
    /// `Features::hatch_patterns` (§16.17).
    HatchPatterns,
    /// `Features::material_hatch`.
    MaterialHatch,
    /// `Features::instance_hatch`.
    InstanceHatch,
    /// `SectionPlane::hatch` (§16.19).
    SectionHatch,
    /// `Features::render_hatch` (§16.22).
    RenderHatch,
    /// `Features::layer_folders` (§16.18).
    LayerFolders,
    /// `Features::current_style`: the current-settings style record apart
    /// from the active style's (§16.14).
    CurrentStyle,
    /// The modified flag of `Features::current_style`, observed only in
    /// SketchUp 2027's files.
    ModifiedStyle,
    /// `Features::layer_line_styles` (§16.18).
    LayerLineStyles,
    /// `Features::layer_page_behaviour` (§16.18).
    LayerPageBehaviour,
    /// `Features::crs` (§16.23).
    Crs,
    /// `Features::material_pbr` (§16.20).
    PbrMaterials,
    /// `Features::environments` and `current_environment` (§16.21).
    Environments,
    /// `Features::scene_environment` (§16.24).
    PageEnvironments,
    /// `Features::scene_pages` (§16.24).
    PageExtras,
    /// `Features::snaps` (§16.25).
    Snaps,
    /// `Features::procedures` (§16.26).
    Procedures,
    /// `Features::procedure_kinds` (§16.26).
    ProcedureKinds,
    /// `RenderingOptions::default_section_fill_style` set (`0x738c`,
    /// §16.22), in the model's display settings or a scene's.
    DefaultSectionFillStyle,
}

impl Feature {
    /// The first revision of the 2026 container known to hold the
    /// feature (§16.27). A convention of this crate, not an observed gate:
    /// 18 for everything of §16.17 to §16.26, which the `{26.2.0}` files of
    /// `corpus/2026` do not use and SketchUp 2027's files do; 17 for the
    /// current-settings record of a renamed style, which both hold.
    /// SketchUp 2027 opens a revision-17 file that holds a hatch pattern
    /// (`corpus/2027/rev-r74-17-hatch.skp`); whether SketchUp 2026 or the
    /// web app open a revision-17 file with a revision-18 feature is not
    /// known.
    pub fn since(self) -> crate::Revision {
        match self {
            Feature::CurrentStyle => crate::Revision::R17,
            _ => crate::Revision::R18,
        }
    }

    /// What the feature is, in the plural, for a note.
    pub fn describe(self) -> &'static str {
        match self {
            Feature::HatchPatterns => "hatch patterns",
            Feature::MaterialHatch => "material hatch patterns",
            Feature::InstanceHatch => "instance hatch patterns",
            Feature::SectionHatch => "section plane hatch patterns",
            Feature::RenderHatch => "the section hatch of the display settings",
            Feature::LayerFolders => "layer folders",
            Feature::CurrentStyle => "current style records",
            Feature::ModifiedStyle => "the modified flag of the current style",
            Feature::LayerLineStyles => "layer line styles",
            Feature::LayerPageBehaviour => "layer page behaviours",
            Feature::Crs => "the coordinate reference system",
            Feature::PbrMaterials => "physically based materials",
            Feature::Environments => "environments",
            Feature::PageEnvironments => "page environments",
            Feature::PageExtras => "page extras",
            Feature::Snaps => "snaps",
            Feature::Procedures => "procedures",
            Feature::ProcedureKinds => "procedure kinds",
            Feature::DefaultSectionFillStyle => "the default section fill style",
        }
    }
}

/// The current-settings style record (§16.14) of a model whose current
/// settings differ from the active style's own record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CurrentStyle {
    /// The name of its document's folder in the archive (`styles/<name>`),
    /// the active style's name followed by `_1` unless it was renamed.
    pub name: String,
    pub description: String,
    /// The u8 `0x697c`: set while the current settings differ from the
    /// active style in a setting the style holds; a setting changed and
    /// changed back, or the style updated, clears it (§16.14).
    pub modified: bool,
}

/// The physically based properties of a material: the `mat:pbrMR` block
/// of `materials/<folder>/material.xml` and the `mat:material` attributes
/// that are not part of the colour or the texture. The defaults are the
/// values of a plain material of a converted file.
///
/// Authoring evidence (`corpus/2027/README.md`): setting any metallic,
/// roughness, normal or occlusion texture, or enabling a map, makes the
/// workflow 1 on its own; setting the factors alone leaves it 0.
#[derive(Debug, Clone, PartialEq)]
pub struct PbrMaterial {
    /// `workflow`: 0 classic, 1 metallic-roughness.
    pub workflow: u32,
    /// `pbrPromoState`: 0 on the materials of a converted file, 2 on a
    /// material made in SketchUp 2027. Its meaning is a candidate (the
    /// physically based block was created with the material).
    pub promo_state: u32,
    /// The `trans` attribute while `useTrans` is 0, when it is not 1: a
    /// material made in SketchUp holds 0.5 there. It has no effect on the
    /// opacity and is not a physically based property; it is carried
    /// along with the others and a material that has only this is not
    /// listed.
    pub trans_unused: Option<f64>,
    /// `enable_metalness`, `enable_roughness`, `enable_normal` and
    /// `enable_occlusion`: the map is in use.
    pub enable_metalness: bool,
    pub enable_roughness: bool,
    pub enable_normal: bool,
    pub enable_occlusion: bool,
    /// `roughness_texture_invert`. Candidate: the roughness map is
    /// inverted; no authored file sets it.
    pub roughness_texture_invert: bool,
    /// `metallicFactor` and `roughnessFactor`.
    pub metallic_factor: f32,
    pub roughness_factor: f32,
    /// `normalMapStyle`: 0 OpenGL, 1 DirectX (the default).
    pub normal_map_style: u32,
    /// `normalScale`.
    pub normal_scale: f32,
    /// `occlusionStrength`.
    pub occlusion_strength: f32,
    /// `baseColorFactor`, an unsigned integer; 0xFFFFFFFF in every file
    /// seen, whatever the material colour.
    pub base_color_factor: u32,
    /// `xScale` and `yScale` of `mat:pbrMR` when they are not the
    /// material's own: 1 and 1 for a solid colour, the applied size of
    /// the texture otherwise. Never varied in an authored file.
    pub scale: Option<[f64; 2]>,
    /// The four texture slots, each an empty element when unset.
    pub metallic: Option<PbrTexture>,
    pub roughness: Option<PbrTexture>,
    pub normal: Option<PbrTexture>,
    pub occlusion: Option<PbrTexture>,
    /// Attributes of `mat:material` the reader does not type, in file
    /// order.
    pub other_material_attrs: Vec<(String, String)>,
    /// Attributes of `mat:pbrMR` besides the scale, in file order.
    pub other_pbr_attrs: Vec<(String, String)>,
    /// Elements of `mat:pbrMR` the reader does not know, as the file has
    /// them, written after the known ones.
    pub other_elements: Vec<String>,
}

impl Default for PbrMaterial {
    /// The properties of a plain material of a converted file.
    fn default() -> PbrMaterial {
        PbrMaterial {
            workflow: 0,
            promo_state: 0,
            trans_unused: None,
            enable_metalness: false,
            enable_roughness: false,
            enable_normal: false,
            enable_occlusion: false,
            roughness_texture_invert: false,
            metallic_factor: 1.0,
            roughness_factor: 1.0,
            normal_map_style: 1,
            normal_scale: 1.0,
            occlusion_strength: 1.0,
            base_color_factor: 0xFFFF_FFFF,
            scale: None,
            metallic: None,
            roughness: None,
            normal: None,
            occlusion: None,
            other_material_attrs: Vec::new(),
            other_pbr_attrs: Vec::new(),
            other_elements: Vec::new(),
        }
    }
}

/// One texture slot of a physically based material: the element's
/// attributes and the image it names. SketchUp stores the image
/// re-encoded (grayscale for the metallic, roughness and occlusion
/// slots, RGB for the normal slot); the PNG is kept as the archive holds
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct PbrTexture {
    /// The element's `id`: 1 for the first slot that holds an image, 2
    /// for the next, in slot order.
    pub id: u32,
    /// The `path` attribute: the image's place relative to the material's
    /// folder, e.g. `./tex.png`. Several slots given the same source image
    /// are stored as separate entries (`tex.png`, `tex_1.png`, ...).
    pub path: String,
    /// The `file_name` attribute: the source image's file name.
    pub file_name: String,
    /// The PNG as the archive holds it.
    pub png: Vec<u8>,
    /// Further attributes of the element, in file order.
    pub other_attrs: Vec<(String, String)>,
}

/// An environment: the settings of `environment.xml`, its name and
/// thumbnail path from the list item in `model.dat`, and the images in
/// `environments/<folder>/`.
#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// `0x7b0d`; also the `name` attribute. A name of an environment made
    /// without one is the image's file name without its extension.
    pub name: String,
    /// `description`.
    pub description: String,
    /// `ibl_enabled`. `ibl_enabled`, `skybox_enabled` and
    /// `shadow_light_enabled` read 0, 0 and 0 in the one file that
    /// switched three environment options off together (sun linked to the
    /// image, image used as the sky, image used for reflections); which
    /// attribute follows which option is not separated (candidate).
    pub ibl_enabled: bool,
    /// `rotation_angle`, stored as given.
    pub rotation_angle: f32,
    /// `skybox_enabled`; see `ibl_enabled`.
    pub skybox_enabled: bool,
    /// `skybox_gain`: the skydome exposure.
    pub skybox_gain: f32,
    /// `reflection_gain`: the reflection exposure.
    pub reflection_gain: f32,
    /// `shadow_light_enabled`; see `ibl_enabled`. 0 in every file seen.
    pub shadow_light_enabled: bool,
    /// `shadow_light_positionX` and `shadow_light_positionY`.
    pub shadow_light_position: [f32; 2],
    /// `ibl_filename`: the image's file name.
    pub ibl_filename: String,
    /// The `ibl_image` element: its `id`, `path` (`./sky.hdr`) and
    /// `file_name`.
    pub image: EnvironmentImage,
    /// The environment image as the archive holds it (the source file,
    /// untouched).
    pub hdr: Vec<u8>,
    /// The JPEG preview (`thumbnail.jpg`), as the archive holds it.
    pub thumbnail: Option<Vec<u8>>,
    /// Further attributes of `env:environment`, in file order.
    pub other_attrs: Vec<(String, String)>,
}

impl Default for Environment {
    /// An environment as SketchUp makes one from an image: lit, shown as
    /// the sky, unit exposures, no rotation.
    fn default() -> Environment {
        Environment {
            name: String::new(),
            description: String::new(),
            ibl_enabled: true,
            rotation_angle: 0.0,
            skybox_enabled: true,
            skybox_gain: 1.0,
            reflection_gain: 1.0,
            shadow_light_enabled: false,
            shadow_light_position: [0.0, 0.0],
            ibl_filename: String::new(),
            image: EnvironmentImage::default(),
            hdr: Vec::new(),
            thumbnail: None,
            other_attrs: Vec::new(),
        }
    }
}

/// The `ibl_image` element of `environment.xml`.
#[derive(Debug, Clone, PartialEq)]
pub struct EnvironmentImage {
    pub id: u32,
    pub path: String,
    pub file_name: String,
}

impl Default for EnvironmentImage {
    fn default() -> EnvironmentImage {
        EnvironmentImage {
            id: 1,
            path: String::new(),
            file_name: String::new(),
        }
    }
}

/// The environment settings of a page (scene). A page that saves the
/// environment property holds `0x715b` with two flags and, when it names
/// an environment, `0x715a`.
#[derive(Debug, Clone, PartialEq)]
pub struct PageEnvironment {
    /// Whether the page saves the environment property: bit `0x200` of
    /// its saved-properties word (`0x7149`). With it clear (the page's
    /// "use environment" option off, whether or not an environment was
    /// assigned) the page holds neither `0x715a` nor `0x715b`.
    pub saved: bool,
    /// Index into `Features::environments` of the page's environment
    /// (`0x715a`).
    pub environment: Option<usize>,
    /// `0x7b0f` and `0x7b10`, two bytes of `0x715b`. Both are 1 on a
    /// page with an environment and 0 on a page without one; no file
    /// separates them, so which means what is a candidate (one of them
    /// "has an environment", the other "uses it").
    pub flag_a: bool,
    pub flag_b: bool,
}

impl Default for PageEnvironment {
    /// A page that saves no environment choice.
    fn default() -> PageEnvironment {
        PageEnvironment {
            saved: true,
            environment: None,
            flag_a: false,
            flag_b: false,
        }
    }
}

/// What a page (scene) holds beyond `Scene`'s fields. Every field at its
/// default is the same as no entry in `Features::scene_pages`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageExtras {
    /// Indices into the pre-order list of `Features::layer_folders` (each
    /// folder before its subfolders) of the folders the page lists in
    /// `0x7159`. The record is the layer folders hidden on the page
    /// (candidate: its one authored sample lists the one folder whose only
    /// layer the page hides).
    pub hidden_layer_folders: Vec<usize>,
    /// The page's display name (`0x7153`) when it is not the name, or the
    /// name in parentheses while the page is left out of the animation.
    /// SketchUp 2027 stores the plain name for a page left out of the
    /// animation, where the older files show the parentheses.
    pub display_name: Option<String>,
    /// The page's own transition time in seconds (`0x7154`); `None` is
    /// the stored -1, the model's own.
    pub transition_s: Option<f64>,
    /// The page's own delay in seconds (`0x7155`); `None` is the stored -1.
    pub delay_s: Option<f64>,
}

/// A snap (`0x7530`, in `0x139d`): a position with a normal and an x axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Snap {
    /// Index into `Model::definitions` of the definition that holds it;
    /// `None` for the model itself.
    pub definition: Option<usize>,
    /// Persistent id.
    pub pid: u32,
    /// `0x7531`.
    pub position_m: [f64; 3],
    /// `0x7532`.
    pub normal: [f64; 3],
    /// `0x7533`, kept as stored: the x axis crossed with the normal
    /// (`corpus/2027/j-snap-skew.skp`, `j-snap-tilt.skp`).
    pub third_axis: [f64; 3],
    /// `0x7534`.
    pub x_axis: [f64; 3],
    /// `0x7535`. Candidate: the snap kind; 2 in the one sample.
    pub kind: u32,
}

impl Default for Snap {
    /// A snap at the origin facing up with its x axis along x, as the one
    /// sample stores it.
    fn default() -> Snap {
        Snap {
            definition: None,
            pid: 0,
            position_m: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            third_axis: [0.0, -1.0, 0.0],
            x_axis: [1.0, 0.0, 0.0],
            kind: 2,
        }
    }
}

/// A definition a procedure made (`0x1588 > 0x7ec2`), such as the result
/// of a solid operation. The definition keeps the operands as ordinary
/// instances; the result sits in a wrapper (`0x13a4`) of the definition's
/// entities and is kept here. The operation is not redone on a read.
#[derive(Debug, Clone, PartialEq)]
pub struct Procedure {
    /// Index into `Model::definitions`.
    pub definition: usize,
    /// `0x7ec3`: the procedure's name.
    pub name: String,
    /// `0x7ec4`. Candidate: a version; 1 in the three samples.
    pub version: u32,
    /// `0x7ec5`. Candidate: flags; 0 in the three samples.
    pub flags: u32,
    /// `0x7ec6`: the parameters, a JSON document as stored.
    pub parameters: String,
    /// The instances in the wrapper (`0x13a4 > 0x13ec > 0x1388`), in file
    /// order: the one instance of the result in the samples. The result's
    /// own definition is an ordinary entry of `Model::definitions`.
    pub results: Vec<ProcedureResult>,
}

/// A placement in the wrapper of a procedure's result: a
/// `PlacedInstance` that names its definition by index.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcedureResult {
    pub pid: u32,
    /// Index into `Model::definitions`.
    pub definition: usize,
    pub transform: [f64; 13],
    pub is_group: bool,
    /// As `PlacedInstance::material` and `layer`: slots, 0 for none and
    /// for the default layer.
    pub material: u16,
    pub layer: u16,
    pub hidden: bool,
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    pub locked: bool,
    /// The placement's GUID, hex; empty makes the writer pick one.
    pub guid: String,
}

/// An entry of the model's procedure registry (`0x0215 > 0x7d82`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProcedureKind {
    /// `0x7d83`: the name a procedure carries as `Procedure::name`.
    pub name: String,
    /// `0x7d84`.
    pub description: String,
}

/// The deepest layer-folder nesting read or written: a folder at this
/// depth (the top level being 0) is cut by the readers and refused by the
/// writers, so a pre-order index names the same folder in both.
pub(crate) const FOLDER_DEPTH: usize = 64;

/// Whether any folder of `list` (at `depth`) lies at or past
/// [`FOLDER_DEPTH`].
pub(crate) fn folders_too_deep(list: &[LayerFolder], depth: usize) -> bool {
    !list.is_empty()
        && (depth >= FOLDER_DEPTH || list.iter().any(|f| folders_too_deep(&f.folders, depth + 1)))
}

/// A layer folder (`0x3e80`): a name, the layers directly in it and its
/// subfolders.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerFolder {
    /// `0x3e81`.
    pub name: String,
    /// The folder is not visible (`0x3e82`). It does not change the
    /// visibility of the layers in it.
    pub hidden: bool,
    /// `0x3e85`. Candidate: the folder is not visible on new pages (the
    /// only authored change that sets it, with nothing else changed).
    pub hidden_on_new_pages: bool,
    /// Indices into `Model::layers` of the layers directly in the folder
    /// (`0x3e84`).
    pub layers: Vec<usize>,
    /// The subfolders (`0x3e83`).
    pub folders: Vec<LayerFolder>,
}

/// The coordinate reference system record (`0x0218 > 0x7e90`). Positions
/// and the scale are stored as given; the units of `eastings`, `northings`
/// and `height` are a candidate (metres: round authored values are stored
/// without a conversion).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Crs {
    /// `0x7e91`.
    pub eastings: f64,
    /// `0x7e92`.
    pub northings: f64,
    /// `0x7e93`.
    pub height: f64,
    /// The x axis direction in the map plane (`0x7e94`, `0x7e95`), stored
    /// as a 2D vector, not an angle.
    pub x_axis: [f64; 2],
    /// `0x7e96`.
    pub scale: f64,
    /// `0x7e97`.
    pub name: String,
    /// `0x7e98`.
    pub description: String,
    /// `0x7e9c`.
    pub map_zone: String,
    /// The geodetic datum (`0x7e99`); empty when the file has no record.
    pub geodetic_datum: String,
    /// The vertical datum (`0x7e9a`).
    pub vertical_datum: String,
    /// The map projection (`0x7e9b`).
    pub map_projection: String,
    /// The map unit (`0x7e9d`), by the name SketchUp stores: `METRE`,
    /// `foot` or `US survey foot` for the three it offers.
    pub map_unit: String,
    /// What the map unit is in metres (`0x7e9e`, an f64: 1 for `METRE`,
    /// 0.3048 for `foot`, 0.3048006096 for `US survey foot`); `None` when
    /// the file has no record.
    pub map_unit_metres: Option<f64>,
    /// The `tgl_id` (`0x7e9f`).
    pub tgl_id: String,
    /// The calibration file name (`0x7ea0`).
    pub calibration_file_name: String,
    /// Any other record the reader does not know (past `0x7ea0`: never
    /// authored), as `(tag, bytes)` in file order.
    pub unknown: Vec<(u16, Vec<u8>)>,
}

/// A hatch pattern: the model-wide definition a section plane, a
/// material, an instance or the rendering options can use. The list item
/// in `model.dat` holds only the name and the thumbnail's path; every
/// other field is the `hatchpatterns/<name>/hatchpattern.xml` entry.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchPattern {
    pub name: String,
    /// The fill colour (`colorRed`, `colorGreen`, `colorBlue`).
    pub color: [u8; 3],
    /// `xScale` and `yScale`.
    pub x_scale: f64,
    pub y_scale: f64,
    /// `rotationAngle`, in degrees.
    pub rotation: f64,
    /// The path of the `.pat` file the pattern was defined from, as given
    /// when it was made (`patName`); `None` when the pattern has no
    /// line data (`hasPat="0"`).
    pub pat_name: Option<String>,
    /// The pattern's lines (`patData`); empty without line data.
    pub lines: Vec<HatchLine>,
    /// The image of a texture-based pattern (`hasTexture="1"`).
    pub texture: Option<HatchTexture>,
    /// The JPEG preview (`thumbnail.jpg`), as the archive holds it.
    pub thumbnail: Option<Vec<u8>>,
}

impl Default for HatchPattern {
    /// A white pattern at unit scale and no rotation, without lines.
    fn default() -> HatchPattern {
        HatchPattern {
            name: String::new(),
            color: [255, 255, 255],
            x_scale: 1.0,
            y_scale: 1.0,
            rotation: 0.0,
            pat_name: None,
            lines: Vec::new(),
            texture: None,
            thumbnail: None,
        }
    }
}

/// One line family of a hatch pattern, as `patData` stores it: the angle
/// in degrees, the origin, the two offsets of the `.pat` line (`shift`
/// along the line and `offset` across it), and the dash list as
/// `(on, off)` pairs. Dash lengths are magnitudes: a `.pat` file's
/// negative (gap) entries are stored without their sign. A line without
/// a dash list is stored as the single pair `(1.0, 0.0)`.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchLine {
    pub angle: f32,
    pub origin: [f32; 2],
    pub shift: f32,
    pub offset: f32,
    pub dashes: Vec<(f32, f32)>,
}

/// The image of a texture-based hatch pattern: the `textureName` element
/// of the XML and the entry it names, next to the XML.
#[derive(Debug, Clone, PartialEq)]
pub struct HatchTexture {
    /// The element's `id` attribute.
    pub id: u32,
    /// The `path` attribute: the image's place relative to the XML's
    /// folder, e.g. `./tex.png`.
    pub path: String,
    /// The `file_name` attribute: the path of the source image as given
    /// when the pattern was made.
    pub file_name: String,
    /// The PNG as the archive holds it (a re-encoding of the source).
    pub png: Vec<u8>,
}

/// The hatch settings of the rendering options (`0x7388 > 0x7d0a`), the
/// binary form of the style document's `hatch_pattern_settings` item.
/// Its colour is the section fill colour, written twice.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderHatch {
    /// The fill colour (`0x7d0c`), RGBA.
    pub color: [u8; 4],
    /// `0x7d0e` and `0x7d0f`.
    pub x_scale: f32,
    pub y_scale: f32,
    /// `0x7d14`, in degrees.
    pub rotation: f32,
    /// The `.pat` path (`0x7d13`); empty without line data.
    pub pat_name: String,
    /// The pattern lines (`0x7d11`, the XML's `patData` as bytes); empty
    /// when the record is absent.
    pub lines: Vec<HatchLine>,
    /// `0x7d0d` and `0x7d12`: two slots that stay empty while no texture
    /// is used (candidate: texture data and texture path). Kept as the
    /// bytes the file holds.
    pub texture_slots: [Vec<u8>; 2],
}

impl Default for RenderHatch {
    /// The settings of a model without a section hatch: opaque black,
    /// unit scale, no rotation, no pattern.
    fn default() -> RenderHatch {
        RenderHatch {
            color: [0, 0, 0, 255],
            x_scale: 1.0,
            y_scale: 1.0,
            rotation: 0.0,
            pat_name: String::new(),
            lines: Vec::new(),
            texture_slots: [Vec::new(), Vec::new()],
        }
    }
}

/// `patData` of `lines`: per line the angle, the origin, the shift, the
/// offset and the dash pair count (a float), then the pairs, all
/// little-endian `f32`.
pub(crate) fn pattern_bytes(lines: &[HatchLine]) -> Vec<u8> {
    let mut out = Vec::new();
    for l in lines {
        for v in [l.angle, l.origin[0], l.origin[1], l.shift, l.offset] {
            out.extend(v.to_le_bytes());
        }
        out.extend((l.dashes.len() as f32).to_le_bytes());
        for (on, off) in &l.dashes {
            out.extend(on.to_le_bytes());
            out.extend(off.to_le_bytes());
        }
    }
    out
}

/// The lines in `patData` bytes; `None` when they do not tile as lines.
pub(crate) fn pattern_lines(b: &[u8]) -> Option<Vec<HatchLine>> {
    if !b.len().is_multiple_of(4) {
        return None;
    }
    let f: Vec<f32> = b
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    let mut lines = Vec::new();
    let mut i = 0;
    while i < f.len() {
        let head = f.get(i..i + 6)?;
        let n = head[5];
        if !(n >= 0.0 && n.fract() == 0.0 && n <= 65535.0) {
            return None;
        }
        let n = n as usize;
        let pairs = f.get(i + 6..i + 6 + 2 * n)?;
        lines.push(HatchLine {
            angle: head[0],
            origin: [head[1], head[2]],
            shift: head[3],
            offset: head[4],
            dashes: pairs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| (p[0], p[1]))
                .collect(),
        });
        i += 6 + 2 * n;
    }
    Some(lines)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub(crate) fn base64(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode standard base64 (padding optional, whitespace ignored); `None`
/// on any other character.
pub(crate) fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST1: &str = "AAA0QgAAAAAAAAAAAAAAAAAAgD4AAIA/AACAPwAAAAA=";

    #[test]
    fn base64_round_trips() {
        let b = unbase64(TEST1).unwrap();
        assert_eq!(b.len(), 32);
        assert_eq!(base64(&b), TEST1);
        for n in 0..9 {
            let v: Vec<u8> = (0..n as u8).collect();
            assert_eq!(unbase64(&base64(&v)).unwrap(), v);
        }
    }

    #[test]
    fn pattern_data_tiles_as_lines() {
        // `45, 0,0, 0,0.25`: a line with no dash list, stored as (1, 0).
        let b = unbase64(TEST1).unwrap();
        let lines = pattern_lines(&b).unwrap();
        assert_eq!(
            lines,
            [HatchLine {
                angle: 45.0,
                origin: [0.0, 0.0],
                shift: 0.0,
                offset: 0.25,
                dashes: vec![(1.0, 0.0)],
            }]
        );
        assert_eq!(pattern_bytes(&lines), b);
        assert!(pattern_lines(&b[..30]).is_none());
        assert!(pattern_lines(&b[..28]).is_none());
    }
}
