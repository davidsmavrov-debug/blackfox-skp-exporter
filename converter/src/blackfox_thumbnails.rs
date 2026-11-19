use std::error::Error;
use std::fs;
use std::path::PathBuf;

use serde_json::json;

pub fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    if args.len() != 3 {
        return Err(
            "Usage: BlackFoxSKP extract-scene-thumbnails input.skp output_directory".into()
        );
    }

    let input = PathBuf::from(&args[1]);
    let output_dir = PathBuf::from(&args[2]);

    if !input.is_file() {
        return Err(format!("Input SKP not found: {}", input.display()).into());
    }

    if output_dir.exists() {
        return Err(format!(
            "Refusing existing thumbnail destination: {}",
            output_dir.display()
        ).into());
    }

    let model = skpwright::Model::read(&input)?;

    let png_signature: &[u8] = b"\x89PNG\r\n\x1a\n";

    let mut entries = Vec::new();
    let mut files = Vec::new();

    for (index, scene) in model.scenes.iter().enumerate() {
        match &scene.thumbnail {
            Some(bytes) => {
                if !bytes.starts_with(png_signature) {
                    return Err(format!(
                        "Scene {} has non-PNG thumbnail data",
                        index
                    ).into());
                }

                let filename = format!("scene_{:04}.png", index);

                entries.push(json!({
                    "scene_index": index,
                    "scene_name": scene.name,
                    "file": filename,
                    "bytes": bytes.len()
                }));

                files.push((filename, bytes));
            }
            None => {
                entries.push(json!({
                    "scene_index": index,
                    "scene_name": scene.name,
                    "file": null,
                    "bytes": null
                }));
            }
        }
    }

    fs::create_dir(&output_dir)?;

    for (filename, bytes) in files {
        fs::write(output_dir.join(filename), bytes)?;
    }

    let manifest = output_dir.join("blackfox_scene_thumbnails.json");

    fs::write(
        &manifest,
        serde_json::to_vec_pretty(&json!({
            "schema": "BlackFoxSceneThumbnailsV1",
            "scenes": entries
        }))?
    )?;

    println!("Scenes: {}", model.scenes.len());
    println!("Manifest: {}", manifest.display());
    println!("SUCCESS: Scene thumbnail extraction completed");

    Ok(())
}
