#[path = "blackfox_export.rs"]
mod exporter;
#[path = "blackfox_import.rs"]
mod importer;
#[path = "blackfox_thumbnails.rs"]
mod thumbnails;

use std::error::Error;

const HELP: &str = r#"BlackFox SKP - Importer 1.0.0 / Exporter 1.0.0

USAGE:
  BlackFoxSKP help
  BlackFoxSKP --version
  BlackFoxSKP import <input.skp> <output.json>
  BlackFoxSKP import-assets <input.skp> <output.json> <assets_directory>
  BlackFoxSKP export <export_input.json> <output.skp> [skp_version]
  BlackFoxSKP roundtrip <input.skp> <output.skp>

COMMANDS:
  import         Read a SketchUp file into the native import JSON bundle.
  import-assets  Also extract embedded textures and create blackfox_textures.json.
  export         Write a SketchUp file from BlackFox export-input JSON.
  roundtrip      Read and rewrite an SKP using the native Rust model.

IMPORTANT:
  The import JSON bundle is NOT directly accepted by 'export'.
  Convert it to the export-input schema with the preserved C001_6D
  conversion bridge and matching texture manifest before exporting.
  The bridge is a separate component, not included in these three Rust files.

  The exporter refuses to overwrite an existing output file.
  SketchUp version support depends on the underlying SKP writer.
"#;

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { print!("{}", HELP); return Ok(()); }
    match args[1].to_ascii_lowercase().as_str() {
        "help" | "--help" | "-h" => { print!("{}", HELP); Ok(()) }
        "--version" | "-v" | "version" => {
            println!("BlackFox SKP Importer 1.0.0 / Exporter 1.0.0"); Ok(())
        }
        "roundtrip" => {
            if args.len() != 4 {
                return Err("Usage: BlackFoxSKP roundtrip input.skp output.skp".into());
            }

            let input = std::path::Path::new(&args[2]);
            let output = std::path::Path::new(&args[3]);

            if !input.is_file() {
                return Err("Input SKP does not exist".into());
            }

            if output.exists() {
                return Err("Output already exists; refusing to overwrite".into());
            }

            if input == output {
                return Err("Input and output must be different".into());
            }

            println!("BLACKFOX NATIVE ROUNDTRIP");
            println!("Reading: {}", input.display());

            let model = skpwright::Model::read(input)?;

            println!("Writing: {}", output.display());
            model.write(output)?;

            println!("BLACKFOX ROUNDTRIP COMPLETE");
            Ok(())
        }
        "export" => {
            if args.len() != 4 && args.len() != 5 { return Err("Usage: BlackFoxSKP export input.json output.skp [skp_version]".into()); }
            let mut forwarded = vec![args[0].clone(),args[2].clone(),args[3].clone()];
            if args.len() == 5 { forwarded.push(args[4].clone()); }
            exporter::run(forwarded)
        }
        "extract-scene-thumbnails" => {
            if args.len() != 4 {
                return Err("Usage: BlackFoxSKP extract-scene-thumbnails input.skp output_directory".into());
            }
            thumbnails::run(vec![args[0].clone(), args[2].clone(), args[3].clone()])
        }
        "import" => {
            if args.len() != 4 { return Err("Usage: BlackFoxSKP import input.skp output.json".into()); }
            importer::run(vec![args[0].clone(),args[2].clone(),args[3].clone()])
        }
        "import-assets" => {
            if args.len() != 5 { return Err("Usage: BlackFoxSKP import-assets input.skp output.json assets_directory".into()); }
            importer::run(vec![args[0].clone(),args[2].clone(),args[3].clone(),args[4].clone()])
        }
        _ => Err(format!("Unknown command: {}. Run BlackFoxSKP help", args[1]).into())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("BLACKFOX ERROR: {}", error);
        std::process::exit(1);
    }
}
