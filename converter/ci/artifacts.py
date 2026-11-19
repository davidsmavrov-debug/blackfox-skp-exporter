"""Verify a native converter, then assemble metadata from the tested artifacts."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import tomllib

CONVERTER = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
}
RELEASE_ABI = 1


def check(condition, message):
    if not condition:
        raise RuntimeError(message)


def artifact_name(target):
    return f"blackfox-skp-{target}" + (".exe" if "windows" in target else "")


def binary_target(path):
    def number(data, start, length):
        return int.from_bytes(data[start:start + length], "little")

    with path.open("rb") as file:
        head = file.read(64)
        if len(head) < 64:
            return None
        if head[:2] == b"MZ":
            file.seek(number(head, 60, 4))
            pe = file.read(26)
            flags = number(pe, 22, 2)
            if pe[:4] == b"PE\0\0" and pe[24:26] == b"\x0b\x02" and flags & 2 and not flags & 0x2000:
                return {0x8664: "x86_64-pc-windows-msvc", 0xAA64: "aarch64-pc-windows-msvc"}.get(number(pe, 4, 2))
        if head[:6] == b"\x7fELF\x02\x01" and number(head, 16, 2) in (2, 3):
            return {62: "x86_64-unknown-linux-gnu", 183: "aarch64-unknown-linux-gnu"}.get(number(head, 18, 2))
        if head[:4] == b"\xcf\xfa\xed\xfe" and number(head, 12, 4) == 2:
            return {0x01000007: "x86_64-apple-darwin", 0x0100000C: "aarch64-apple-darwin"}.get(number(head, 4, 4))
    return None


def smoke_test(exe):
    def run(*args):
        return subprocess.run([exe, *args], check=True, timeout=60, text=True,
                              encoding="utf-8", errors="replace", stdout=subprocess.PIPE).stdout

    text = run("--version").strip()
    check("Importer" in text and "Exporter" in text, "Missing unified converter version")
    with tempfile.TemporaryDirectory(prefix="blackfox-ci-") as folder:
        root = Path(folder)
        source = root / "model.json"
        source.write_text(json.dumps({"loose_geometry": [{
            "name": "Triangle",
            "vertices": [[0, 0, 0], [1, 0, 0], [0, 1, 0]],
            "edges": [],
            "faces": [{"outer": [0, 1, 2], "normal": [0, 0, 1]}],
        }]}), encoding="utf-8")
        for version in ("2017", "2027"):
            skp = root / f"triangle-{version}.skp"
            native = root / f"native-{version}.json"
            assets = root / f"assets-{version}"
            assets.mkdir()
            run("export", source, skp, version)
            run("import-assets", skp, native, assets)
            result = json.loads(native.read_text(encoding="utf-8"))
            faces = sum(entry["face_count"] for entry in result["cad"]["geometry"])
            edges = sum(entry["edge_count"] for entry in result["cad"]["geometry"])
            points = {tuple(round(value, 8) for value in point)
                      for entry in result["cad"]["geometry"] for point in entry["vertices"]}
            check(faces == 1 and edges == 3 and points == {(0,0,0), (1,0,0), (0,1,0)},
                  f"SKP {version} triangle did not roundtrip")
            check((assets / "blackfox_textures.json").is_file(), "Missing texture manifest")
    print(f"{text} | SKP 2017 and 2027 export/import-assets smoke tests passed.")
    return text


def asset_metadata(path):
    return {"name": path.name, "size": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def verify(target):
    host_arch = {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}
    check(host_arch.get(platform.machine().lower()) == target.split("-")[0],
          f"Native runner architecture {platform.machine()} does not match {target}.")
    exe = "BlackFoxSKP.exe" if "windows" in target else "BlackFoxSKP"
    binary = CONVERTER / "target" / target / "release" / exe
    check(binary_target(binary) == target, f"Wrong binary header for {target}.")
    version_output = smoke_test(binary)
    directory = CONVERTER / "dist" / target
    directory.mkdir(parents=True, exist_ok=True)
    shipped = directory / artifact_name(target)
    shutil.copy2(binary, shipped)
    info = {"target": target, "asset": asset_metadata(shipped), "version_output": version_output}
    (directory / "asset.json").write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
    print(f"Verified {target}: {info['asset']}")


def manifest(downloads):
    assets = {}
    versions = set()
    for target in sorted(TARGETS):
        folder = downloads / f"converter-{target}"
        row = json.loads((folder / "asset.json").read_text(encoding="utf-8"))
        path = folder / artifact_name(target)
        check(row["target"] == target and binary_target(path) == target, f"Wrong artifact target: {target}")
        actual = asset_metadata(path)
        check(row["asset"] == actual, f"Artifact SHA-256 or size mismatch: {target}")
        assets[target] = actual
        versions.add(row["version_output"])
    check(len(versions) == 1, "The six binaries reported different versions.")
    cargo_manifest = (CONVERTER / "Cargo.toml").read_text(encoding="utf-8-sig")
    build_version = tomllib.loads(cargo_manifest)["package"]["version"]
    check(re.fullmatch(r"[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}", build_version), "Unexpected version format.")
    expected_cli_version = f"BlackFox SKP Importer {build_version} / Exporter {build_version}"
    check(versions == {expected_cli_version}, "CLI version does not match Cargo package version.")
    draft = {
        "schema": 1,
        "repository": None,
        "releases": [{"version": build_version, "abi": RELEASE_ABI, "assets": assets}],
    }
    output = downloads / "metadata"
    output.mkdir(exist_ok=True)
    (output / "blackfox_release.draft.json").write_text(json.dumps(draft, indent=2) + "\n", encoding="utf-8")
    (output / "SHA256SUMS.txt").write_text(
        "".join(f"{asset['sha256']}  {asset['name']}\n" for asset in assets.values()), encoding="utf-8")
    print(f"Verified all {len(assets)} artifacts. Draft Cargo version {build_version}; CLI reports {versions.pop()}.")
    print("No official repository is assigned to this draft manifest.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command", required=True)
    subcommands.add_parser("verify").add_argument("target", choices=sorted(TARGETS))
    subcommands.add_parser("manifest").add_argument("downloads", type=Path)
    args = parser.parse_args()
    if args.command == "verify":
        verify(args.target)
    else:
        manifest(args.downloads)
