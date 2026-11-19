# skpwright

Clean-room reader and writer for the SketchUp `.skp` format, every
release: 3, 4, 5, 6, 7, 8, 2013, 2014, 2015, 2016, 2017 and 2026.
Reads meshes, materials (solid and textured, including embedded images
and per-corner UVs), the component/group hierarchy with transforms,
layers, scenes, guides, attribute dictionaries, and the document's
settings (camera, rendering options, shadows, units, styles and
watermarks, fonts, and text/dimension annotations), with one runtime
dependency (a DEFLATE decoder) and **no Trimble SDK anywhere in its
lineage**.

Every format fact derives from observed `.skp` files, their COLLADA
exports, and public knowledge of MFC `CArchive` serialization. The
evidence corpus and the full format specification live in the
[repository](https://github.com/hew3d/skpwright);
[`docs/SKP_FORMAT.md`](https://github.com/hew3d/skpwright/blob/main/docs/SKP_FORMAT.md)
is detailed enough to build an independent parser.

## Reading a model

```rust
let model = skpwright::Model::read("model.skp")?;   // from a path
let model = skpwright::Model::parse(&bytes)?;       // from memory

println!("{} ({} definitions, {} materials)",
         model.version, model.definitions.len(), model.materials.len());
```

`Model` exposes the document as plain data: `definitions`, `instances`,
per-definition `geometry` runs with materialized meshes, `materials`,
`layers`, `scenes` (typed `Scene`s), `guides`, `attributes`, the
document's `camera`, `rendering` options, `shadows` and `units`, its
`styles` and `watermarks`, `fonts`/`texts`/`dimensions`, and
`diagnostics` (an empty desync set is the clean-parse signal). `scene()`
composes the instance tree into `Node`s carrying world transforms,
inherited materials, and visibility. Coordinates are exposed in metres
(`skpwright::INCH` converts from the file's f64 inches). `header_info`
identifies any `.skp` from its header alone.

**Supported:** releases 3 to 8 and 2013 to 2020 (`{3.x}` to `{20.x}`),
which share the stream container, one MFC `CArchive` object stream, and
the 2026 container (`{26.2.0}`, `{27.0.267}` as observed), a ZIP archive
of tagged records; both read and written. Releases 2021 to 2025 are
supported through the 2026 container. All produce the same `Model`,
and `Model::release()` reports which stream-container release a model was
read from. Every 2017 corpus file but `section-plane-deactivated.skp`
has a 2026 twin that reads equivalent to it, and the COLLADA-oracle
tests run on both containers. Production-scale models read on both paths
(a 464 MB release-2017 house in 12 s and 2 GB, a local model outside the
repository); a process too small to hold a model gets a named
out-of-memory error instead of aborting, and a damaged record is skipped
and reported rather than failing the file.

## Writing a model

```rust
use skpwright::Release;

model.write("out.skp")?;                          // a 2026 file
let notes = model.write_as("old.skp", Release::R2013)?;
for note in &notes {
    eprintln!("{note}");                          // what release 2013 could not hold
}
```

`Document` round-trips a 2026 file byte for byte and takes surgical
edits; `Document::from_model` authors a 2026 file from any model.
`StreamDocument` does the same for a file of any release 3 to 2020
(`StreamDocument::from_model(&model, Release::R2017)` authors one), takes
in-place edits, and checks a file against the whole-file grammar with
`verify()`. Writing to an earlier release drops what it cannot hold and
reports each drop as a note.

Releases 2021 to 2025 are supported: a file whose header strings are
followed by a ZIP archive is read as a 2026 file. Every observed file of
releases 3 to 6 and 2013 to 2016 was written by SketchUp Make 2017
saving to that release, so a few layout details of those releases are
marked candidate in the spec, and each release other than 2017 and 2026
has a smaller evidence set (`corpus/README.md` in the repository).

See [`docs/SDK.md`](https://github.com/hew3d/skpwright/blob/main/docs/SDK.md)
for the full guide, and the companion crates
[`skpwright-cli`](https://crates.io/crates/skpwright-cli) (command-line
tool) and [`skpwright-capi`](https://crates.io/crates/skpwright-capi)
(C ABI).

## License

GPL-3.0-only. Contributions must satisfy the clean-room policy in
[`CONTRIBUTING.md`](https://github.com/hew3d/skpwright/blob/main/CONTRIBUTING.md).
