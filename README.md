# BlackFox SKP Converter v1.0.0

Free and open-source Rust converter for importing and exporting SketchUp SKP models in IngeTrazo.

## Download

Get the standalone executable for your platform from the [v1.0.0 release](https://github.com/davidsmavrov-debug/blackfox-skp-exporter/releases/tag/v1.0.0). Windows, Linux, and macOS builds are available for x86-64 and ARM64. The release includes SHA-256 checksums and the download manifest.

## IngeTrazo integration

The BlackFox SKP Exporter extension prompts before downloading the matching converter. It checks the file size, SHA-256, and executable architecture before installing it and reuses the verified local copy. On Windows the extension stores the executable inside `%APPDATA%\ingetrazo\plugins\BlackFoxSKP\bin`.

## Credits and licenses

- Project creator: David Mavrov
- Development assistance: ChatGPT (OpenAI)
- SKP library: [skpwright](https://github.com/hew3d/skpwright) by Kurt Granroth and contributors
- BlackFox SKP Converter and skpwright: GPL-3.0-only. Dependencies retain their own licenses under `converter/vendor/`.

## Verification

All six native builds passed SKP 2017 and 2027 converter round trips. The IngeTrazo integration was tested on Windows, including fixtures from SketchUp 3 through 2027. Mac and Linux verification covers the native converters and separate Python file-dialog tests.
