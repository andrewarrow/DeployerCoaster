#!/usr/bin/env python3
"""Create portable DeployerCoaster Studio archives for native CI builds."""

from __future__ import annotations

import argparse
import plistlib
import shutil
import subprocess
import tarfile
import tomllib
import zipfile
from pathlib import Path


PACKAGING_DIR = Path(__file__).resolve().parent
CRATE_DIR = PACKAGING_DIR.parent
REPOSITORY_DIR = CRATE_DIR.parent


def run(*command: str) -> None:
    subprocess.run(command, check=True)


def version_from_manifest() -> str:
    with (CRATE_DIR / "Cargo.toml").open("rb") as manifest:
        return tomllib.load(manifest)["package"]["version"]


def add_zip_tree(archive: zipfile.ZipFile, source: Path, archive_root: str) -> None:
    for path in sorted(source.rglob("*")):
        if path.is_file():
            archive.write(path, Path(archive_root) / path.relative_to(source))


def package_macos(binary: Path, destination: Path, version: str) -> Path:
    app = destination / "DeployerCoaster Studio.app"
    contents = app / "Contents"
    macos = contents / "MacOS"
    resources = contents / "Resources"
    macos.mkdir(parents=True)
    resources.mkdir(parents=True)
    shutil.copy2(binary, macos / "studio")
    shutil.copy2(CRATE_DIR / "assets" / "logo.png", resources / "logo.png")
    shutil.copy2(REPOSITORY_DIR / "LICENSE", resources / "LICENSE")

    iconset = destination / "logo.iconset"
    iconset.mkdir()
    logo = CRATE_DIR / "assets" / "logo.png"
    for size in (16, 32, 128, 256, 512):
        run("sips", "-z", str(size), str(size), str(logo), "--out", str(iconset / f"icon_{size}x{size}.png"))
        doubled = size * 2
        run("sips", "-z", str(doubled), str(doubled), str(logo), "--out", str(iconset / f"icon_{size}x{size}@2x.png"))
    run("iconutil", "-c", "icns", str(iconset), "-o", str(resources / "logo.icns"))
    shutil.rmtree(iconset)

    template = PACKAGING_DIR / "macos" / "Info.plist"
    with template.open("rb") as source:
        metadata = plistlib.load(source)
    metadata["CFBundleShortVersionString"] = version
    metadata["CFBundleVersion"] = version
    with (contents / "Info.plist").open("wb") as output:
        plistlib.dump(metadata, output, sort_keys=True)

    # Ad hoc signing makes the bundle structurally valid. It does not provide
    # Developer ID signing or notarization for distribution through Gatekeeper.
    run("codesign", "--force", "--deep", "--sign", "-", str(app))
    archive_path = destination / f"deployercoaster-studio-{version}-macos-arm64.zip"
    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        add_zip_tree(archive, app, app.name)
    return archive_path


def package_windows(binary: Path, destination: Path, version: str, arch: str) -> Path:
    staging = destination / f"deployercoaster-studio-{version}-windows-{arch}"
    staging.mkdir()
    shutil.copy2(binary, staging / "studio.exe")
    shutil.copy2(CRATE_DIR / "assets" / "logo.png", staging / "logo.png")
    shutil.copy2(REPOSITORY_DIR / "LICENSE", staging / "LICENSE")
    readme = REPOSITORY_DIR / "README.md"
    if readme.is_file():
        shutil.copy2(readme, staging / "README.md")
    archive_path = destination / f"{staging.name}.zip"
    with zipfile.ZipFile(archive_path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        add_zip_tree(archive, staging, staging.name)
    shutil.rmtree(staging)
    return archive_path


def package_linux(binary: Path, destination: Path, version: str, arch: str) -> Path:
    staging = destination / f"deployercoaster-studio-{version}-linux-{arch}"
    staging.mkdir()
    shutil.copy2(binary, staging / "studio")
    shutil.copy2(CRATE_DIR / "assets" / "logo.png", staging / "logo.png")
    shutil.copy2(REPOSITORY_DIR / "LICENSE", staging / "LICENSE")
    readme = REPOSITORY_DIR / "README.md"
    if readme.is_file():
        shutil.copy2(readme, staging / "README.md")
    archive_path = destination / f"{staging.name}.tar.gz"
    with tarfile.open(archive_path, "w:gz") as archive:
        archive.add(staging, arcname=staging.name)
    shutil.rmtree(staging)
    return archive_path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True, choices=("macos", "windows", "linux"))
    parser.add_argument("--target", required=True)
    parser.add_argument("--out-dir", type=Path, required=True)
    args = parser.parse_args()

    target_parts = args.target.split("-")
    if len(target_parts) < 3:
        parser.error(f"invalid Rust target triple: {args.target}")
    arch = {"aarch64": "arm64", "x86_64": "x86_64"}.get(target_parts[0])
    if arch is None:
        parser.error(f"unsupported architecture in target triple: {args.target}")
    expected_platform = {"apple": "macos", "pc": "windows", "unknown": "linux"}.get(target_parts[1])
    if expected_platform != args.platform:
        parser.error(f"target {args.target} does not match platform {args.platform}")

    version = version_from_manifest()
    executable = "studio.exe" if args.platform == "windows" else "studio"
    binary = CRATE_DIR / "target" / args.target / "release" / executable
    if not binary.is_file():
        parser.error(f"built executable not found: {binary}")

    destination = args.out_dir.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    if args.platform == "macos":
        archive = package_macos(binary, destination, version)
    elif args.platform == "windows":
        archive = package_windows(binary, destination, version, arch)
    else:
        archive = package_linux(binary, destination, version, arch)
    print(f"Created {archive}")


if __name__ == "__main__":
    main()
