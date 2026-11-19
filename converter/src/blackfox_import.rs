use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn count(value: &Value, key: &str) -> usize {
    value.get(key)
        .and_then(Value::as_array)
        .map_or(0, |items| items.len())
}

fn cad_geometry(mesh: &Value) -> Value {
    let runs = mesh.get("runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut geometry = Vec::new();

    for (index, run) in runs.iter().enumerate() {
        geometry.push(json!({
            "run_id": index,
            "coordinate_unit": "metre",
            "vertices": run.get("vertices_m").cloned().unwrap_or(json!([])),
            "vertex_pids": run.get("vertex_pids").cloned().unwrap_or(json!([])),
            "edges": run.get("edges").cloned().unwrap_or(json!([])),
            "faces": run.get("faces").cloned().unwrap_or(json!([])),
            "vertex_count": count(run, "vertices_m"),
            "edge_count": count(run, "edges"),
            "face_count": count(run, "faces")
        }));
    }

    Value::Array(geometry)
}

fn cad_placements(mesh: &Value) -> Value {
    let scene = mesh.get("scene")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut placements = Vec::new();

    for (index, node) in scene.iter().enumerate() {
        placements.push(json!({
            "placement_id": index,
            "run_id": node.get("run").cloned().unwrap_or(Value::Null),
            "definition": node.get("definition").cloned().unwrap_or(Value::Null),
            "is_group": node.get("is_group").cloned().unwrap_or(Value::Null),
            "world_transform": node.get("world").cloned().unwrap_or(Value::Null),
            "material": node.get("material").cloned().unwrap_or(Value::Null),
            "layer": node.get("layer").cloned().unwrap_or(Value::Null),
            "hidden": node.get("hidden").cloned().unwrap_or(Value::Null),
            "children": node.get("children").cloned().unwrap_or(json!([]))
        }));
    }

    Value::Array(placements)
}

// BLACKFOX PHASE 2E UNIFIED HIERARCHY
// Preserve reader-composed hierarchy, source identities, and instance paths.
fn hierarchy_nodes(model: &skpwright::Model) -> Value {
    fn convert(model: &skpwright::Model, node: &skpwright::Node, path: Vec<usize>) -> Value {
        let identity = node.slot.and_then(|slot| {
            model.instances.iter().find(|i| i.slot == Some(slot))
                .map(|i| (i.pid, i.guid.clone(), i.locked))
                .or_else(|| model.geometry.iter()
                    .flat_map(|r| r.placed.iter())
                    .find(|p| p.slot == Some(slot))
                    .map(|p| (p.pid, p.guid.clone(), Some(p.locked))))
        }).or_else(|| {
            // Phase 2F: recover identity when archive slots are unavailable.
            let matches: Vec<_> = model.instances.iter().filter(|i| {
                i.definition == node.definition
                    && i.is_group == node.is_group
                    && i.transform.iter().zip(node.local.iter())
                        .all(|(a, b)| (*a - *b).abs() < 1e-9)
            }).collect();

            if matches.len() == 1 {
                let instance = matches[0];
                Some((
                    instance.pid,
                    instance.guid.clone(),
                    instance.locked
                ))
            } else {
                None
            }
        });
        let children: Vec<Value> = node.children.iter().enumerate()
            .map(|(i, child)| {
                let mut child_path = path.clone();
                child_path.push(i);
                convert(model, child, child_path)
            }).collect();
        json!({
            "definition": node.definition,
            "run_id": node.run,
            "local_transform_inches": node.local,
            "world_transform_metres": node.world,
            "material": node.material,
            "hidden": node.hidden,
            "layer": node.layer,
            "is_group": node.is_group,
            "slot": node.slot,
            "source_pid": identity.as_ref().map(|v| v.0),
            "source_guid": identity.as_ref().map(|v| v.1.clone()),
            "locked": identity.as_ref().and_then(|v| v.2),
            "instance_path": path,
            "children": children
        })
    }
    Value::Array(model.scene().iter().enumerate()
        .map(|(i, node)| convert(model, node, vec![i]))
        .collect())
}

pub fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    // CLI arguments provided by unified dispatcher.

    if args.len() != 3 && args.len() != 4 {
        return Err(
            "Usage: BlackFoxSKP import input.skp output.json".into()
        );
    }

    let input = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);

    if !input.is_file() {
        return Err(format!("Input not found: {}", input.display()).into());
    }

    if input == output {
        return Err("Input and output must differ".into());
    }

    println!("BlackFox SKP Importer 1.0.0");
    println!("=======================");

    let source_size = fs::metadata(&input)?.len();
    let model = skpwright::Model::read(&input)?;
    // BLACKFOX_PHASE_2P_NATIVE_WATERMARK_DIAGNOSTIC
    println!("BLACKFOX 2P NATIVE WATERMARK DIAGNOSTIC");
    println!("STYLE COUNT: {}", model.styles.len());

    for (style_index, style) in model.styles.iter().enumerate() {
        println!("STYLE {}: {}", style_index, style.name);

        for wm in &style.watermarks {
            println!(
                "STYLE WATERMARK [{}] IMAGE BYTES: {:?}",
                wm.name,
                wm.image_bytes.as_ref().map(|bytes| bytes.len())
            );
        }
    }

    println!("CURRENT-VIEW WATERMARK COUNT: {}", model.watermarks.len());

    for wm in &model.watermarks {
        println!(
            "CURRENT WATERMARK [{}] IMAGE BYTES: {:?}",
            wm.name,
            wm.image_bytes.as_ref().map(|bytes| bytes.len())
        );
    }

    // Optional, explicitly requested lossless texture extraction.
    // The existing JSON import schema is intentionally unchanged.
    if args.len() == 4 {
        let assets_dir = PathBuf::from(&args[3]);
        if assets_dir.exists() && !assets_dir.is_dir() {
            return Err("Texture destination exists and is not a directory".into());
        }
        fs::create_dir_all(&assets_dir)?;
        let mut entries = Vec::new();
        for (index, material) in model.materials.iter().enumerate() {
            if let skpwright::Material::Textured {
                name, texture, image_bytes, applied_size_in, ..
            } = material {
                let (file, size) = match image_bytes {
                    Some(bytes) => {
                        let ext = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                            "png"
                        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
                            "jpg"
                        } else {
                            return Err(format!("Unsupported embedded image format at material {}", index).into());
                        };
                        let filename = format!("material_{:04}.{}", index, ext);
                        let target = assets_dir.join(&filename);
                        if target.exists() {
                            return Err(format!("Refusing to overwrite {}", target.display()).into());
                        }
                        fs::write(&target, bytes)?;
                        (Some(filename), Some(bytes.len()))
                    }
                    None => (None, None),
                };
                entries.push(json!({
                    "material_index": index,
                    "name": name,
                    "original_texture_path": texture,
                    "applied_size_in": applied_size_in,
                    "file": file,
                    "embedded_bytes": size
                }));
            }
        }
        // D4N61AC: extract PBR channels from THIS SKP, never a fixed asset library.
        let mut pbr_entries = Vec::new();
        for (material_index, pbr) in &model.features.material_pbr {
            if *material_index >= model.materials.len() {
                return Err(format!("Invalid PBR material index {}", material_index).into());
            }
            for (channel, texture) in [
                ("metallic", &pbr.metallic),
                ("roughness", &pbr.roughness),
                ("normal", &pbr.normal),
                ("occlusion", &pbr.occlusion),
            ] {
                if let Some(texture) = texture {
                    if !texture.png.starts_with(b"\x89PNG\r\n\x1a\n") {
                        return Err(format!("Missing or invalid PBR PNG: material {} channel {}", material_index, channel).into());
                    }
                    let filename = format!("pbr_material_{:04}_{}.png", material_index, channel);
                    let target = assets_dir.join(&filename);
                    if target.exists() {
                        return Err(format!("Refusing to overwrite {}", target.display()).into());
                    }
                    fs::write(&target, &texture.png)?;
                    pbr_entries.push(json!({
                        "material_index": material_index,
                        "channel": channel,
                        "id": texture.id,
                        "path": texture.path,
                        "file_name": texture.file_name,
                        "file": filename,
                        "bytes": texture.png.len(),
                    }));
                }
            }
        }
        let pbr_manifest = assets_dir.join("blackfox_pbr_manifest.json");
        if pbr_manifest.exists() {
            return Err(format!("Refusing to overwrite {}", pbr_manifest.display()).into());
        }
        fs::write(&pbr_manifest, serde_json::to_vec_pretty(&json!({
            "schema": "BlackFoxPbrAssetsV1", "textures": pbr_entries
        }))?)?;
        println!("PBR manifest: {}", pbr_manifest.display());
        let manifest = assets_dir.join("blackfox_textures.json");
        if manifest.exists() {
            return Err(format!("Refusing to overwrite {}", manifest.display()).into());
        }
        fs::write(&manifest, serde_json::to_vec_pretty(&json!({
            "schema": "BlackFoxTextureAssetsV1", "materials": entries
        }))?)?;
        println!("Texture manifest: {}", manifest.display());
    }

    let model_json: Value = serde_json::from_str(&model.to_json())?;
    // D4N61AE: preserve full radial/diametral records directly from the typed model.
    // model.to_json() omits definition ownership and several display flags.
    let native_radial_dimensions: Vec<Value> = model.radial_dimensions.iter().map(|d| json!({
        "pid": d.pid,
        "def_index": d.def_index,
        "edge": d.edge,
        "arc": d.arc.as_ref().map(|a| json!({
            "center_m": a.center_m,
            "normal": a.normal,
            "x_axis_m": a.x_axis_m,
            "y_axis_m": a.y_axis_m,
            "radius_m": a.radius_m,
            "start_rad": a.start_rad,
            "end_rad": a.end_rad
        })),
        "diameter": d.diameter,
        "leader": d.leader,
        "text_override": d.text_override,
        "font": d.font,
        "aligned": d.aligned,
        "arrow": d.arrow,
        "layer": d.layer,
        "material": d.material,
        "hidden": d.hidden,
        "casts_shadows": d.casts_shadows,
        "receives_shadows": d.receives_shadows
    })).collect();
    let mesh_json: Value = serde_json::from_str(&model.mesh_json())?;

    let scene_hierarchy = hierarchy_nodes(&model);
    let geometry = cad_geometry(&mesh_json);
    let placements = cad_placements(&mesh_json);

    let geometry_count = geometry.as_array().map_or(0, Vec::len);
    let placement_count = placements.as_array().map_or(0, Vec::len);

    let summary = json!({
        "definitions": count(&model_json, "definitions"),
        "instances": count(&model_json, "instances"),
        "layers": count(&model_json, "layers"),
        "materials": count(&model_json, "materials"),
        "scenes": count(&model_json, "scenes"),
        "geometry_runs": geometry_count,
        "scene_roots": placement_count,
        "diagnostics": model.diagnostics.len(),
        "diagnostic_messages": model.diagnostics
            .iter()
            .map(|d| format!("{:?}", d))
            .collect::<Vec<String>>()
    });

    let cad = json!({
        "schema_version": 1,
        "coordinate_unit": "metre",
        "geometry": geometry,
        "placements": placements,
        "scene_hierarchy": scene_hierarchy,
        "definitions": model_json.get("definitions").cloned().unwrap_or(json!([])),
        "instances": model_json.get("instances").cloned().unwrap_or(json!([])),
        "layers": model_json.get("layers").cloned().unwrap_or(json!([])),
        "materials": model_json.get("materials").cloned().unwrap_or(json!([])),
        "source_units": model_json.get("units").cloned().unwrap_or(Value::Null)
    });

    // Phase 2N.23: preserve embedded classification archives losslessly.
    // Hex encoding uses no external dependencies and retains every byte.
    let source_classifications: Vec<Value> = model.classifications.iter().map(|c| {
        let mut data_hex = String::with_capacity(c.data.len() * 2);
        for byte in &c.data {
            use std::fmt::Write;
            write!(&mut data_hex, "{:02x}", byte)
                .expect("Writing to String cannot fail");
        }
        json!({
            "name": c.name,
            "source_path": c.source_path,
            "bytes": c.data.len(),
            "data_hex": data_hex
        })
    }).collect();
    // BLACKFOX_PHASE_2P_LOSSLESS_WATERMARKS
    // Preserve native image payloads independently of external source paths.
    let watermark_json = |w: &skpwright::Watermark| -> Value {
        let image_hex = w.image_bytes.as_ref().map(|bytes| {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                use std::fmt::Write;
                write!(&mut hex, "{:02x}", byte)
                    .expect("Writing to String cannot fail");
            }
            hex
        });

        json!({
            "name": w.name,
            "source_file": w.source_file,
            "background": w.background,
            "tiled": w.tiled,
            "stretched": w.stretched,
            "keep_aspect_ratio": w.keep_aspect_ratio,
            "position": w.position,
            "mask": w.mask,
            "blend": w.blend,
            "scale": w.scale,
            "time": w.time,
            "image_bytes": w.image_bytes.as_ref().map(|b| b.len()),
            "image_hex": image_hex
        })
    };

    let source_style_watermarks: Vec<Value> = model.styles.iter()
        .enumerate()
        .map(|(index, style)| json!({
            "style_index": index,
            "watermarks": style.watermarks.iter()
                .map(&watermark_json)
                .collect::<Vec<Value>>()
        }))
        .collect();

    let source_current_watermarks: Vec<Value> = model.watermarks.iter()
        .map(&watermark_json)
        .collect();
    let bundle = json!({
        "format": "BlackFoxSKPImport",
        "schema_version": 2,
        "importer": {
            "name": "BlackFoxSKP",
            "version": "1.0.0",
            "backend": "Skpwright"
        },
        "source": {
            "file_name": input.file_name().and_then(|s| s.to_str()).unwrap_or("unknown"),
            "file_size_bytes": source_size,
            "sketchup_version": model.version,
            "container": model_json.get("container").cloned().unwrap_or(Value::Null)
        },
        "summary": summary,
        "native_radial_dimensions": native_radial_dimensions,
        "source_classifications": source_classifications,
        "source_style_watermarks": source_style_watermarks,
        "source_current_watermarks": source_current_watermarks,
        "source_native_watermark_section_value": model.native_watermark_section_value,
        "model": model_json,
        "mesh": mesh_json,
        "material_links": model.material_links,
        "cad": cad
    });

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let serialized = serde_json::to_vec_pretty(&bundle)?;
    fs::write(&output, serialized)?;

    let verified: Value = serde_json::from_slice(&fs::read(&output)?)?;

    if verified["schema_version"] != 2 ||
       verified["format"] != "BlackFoxSKPImport" {
        return Err("Output verification failed".into());
    }

    println!("Version: {}", model.version);
    println!("Geometry runs: {}", geometry_count);
    println!("Scene roots: {}", placement_count);
    println!("Diagnostics: {}", model.diagnostics.len());
    println!("SUCCESS: {}", output.display());

    Ok(())
}
