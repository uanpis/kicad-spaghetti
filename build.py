#!/usr/bin/env python3
"""
Build script for packaging a KiCad IPC plugin.
Usage:
    python build.py                     # Creates a .zip archive for the current platform
    python build.py --target x86_64-pc-windows-gnu   # Build for a specific target triple
    python build.py --debug             # Build in debug mode (default is release)
    python build.py --install           # Install directly to the KiCad plugins directory
    python build.py --target aarch64-apple-darwin --install
"""

import os
import sys
import json
import shutil
import platform
import subprocess
import argparse
import tempfile
import re
from pathlib import Path

def get_cargo_metadata():
    try:
        output = subprocess.check_output(
            ["cargo", "metadata", "--format-version=1"],
            stderr=subprocess.DEVNULL,
            text=True
        )
        return json.loads(output)
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print("Error: Could not run `cargo metadata`. Is cargo installed?")
        sys.exit(1)

def get_package_info():
    metadata = get_cargo_metadata()
    packages = metadata.get("packages", [])
    if not packages:
        print("Error: No packages found in cargo metadata.")
        sys.exit(1)

    root_cargo = Path(__file__).parent.absolute() / "Cargo.toml"
    root_cargo_str = str(root_cargo)

    pkg = None
    for p in packages:
        if p.get("manifest_path", "") == root_cargo_str:
            pkg = p
            break

    if pkg is None:
        print(f"Error: Could not find package with manifest_path = {root_cargo_str}")
        print("Available packages:")
        for p in packages:
            print(f"  {p.get('name')} -> {p.get('manifest_path')}")
        sys.exit(1)

    name = pkg["name"]
    version = pkg["version"]
    description = pkg.get("description", "")
    authors = pkg.get("authors", [])

    binary_name = name
    for target in pkg.get("targets", []):
        if "bin" in target.get("kind", []):
            binary_name = target["name"]
            break

    return {
        "name": name,
        "version": version,
        "description": description,
        "authors": authors,
        "binary_name": binary_name,
    }

def get_current_platform_triple():
    system = platform.system()
    machine = platform.machine().lower()
    if machine in ("amd64", "x86_64"):
        arch = "x86_64"
    elif machine in ("aarch64", "arm64"):
        arch = "aarch64"
    elif machine.startswith("arm"):
        arch = "arm"
    else:
        arch = machine

    if system == "Windows":
        return f"{arch}-pc-windows-msvc"
    elif system == "Darwin":
        return f"{arch}-apple-darwin"
    else:
        return f"{arch}-unknown-linux-gnu"

def get_target_directory(package_name, target_triple, debug):
    project_root = Path(__file__).parent.absolute()
    build_mode = "debug" if debug else "release"
    if target_triple:
        base = project_root / "target" / target_triple / build_mode
    else:
        base = project_root / "target" / build_mode
    return base

def get_kicad_plugins_dir():
    system = platform.system()
    if system == "Windows":
        base = Path.home() / "Documents" / "kicad"
    elif system == "Darwin":
        base = Path.home() / "Library" / "Application Support" / "kicad"
    else:
        base = Path.home() / ".local" / "share" / "kicad"

    if not base.exists():
        return base / "9.0" / "3rdparty" / "plugins"

    version_pattern = re.compile(r'^(\d+)\.(\d+)$')
    version_dirs = []
    for item in base.iterdir():
        if item.is_dir():
            match = version_pattern.match(item.name)
            if match:
                major = int(match.group(1))
                minor = int(match.group(2))
                version_dirs.append((major, minor, item))

    if not version_dirs:
        return base / "9.0" / "3rdparty" / "plugins"

    version_dirs.sort(key=lambda x: (x[0], x[1]), reverse=True)
    latest = version_dirs[0][2]
    return latest / "3rdparty" / "plugins"

def create_plugin_manifest(dest_dir, binary_filename, package_info):
    manifest = {
        "$schema": "https://go.kicad.org/api/schemas/v1",
        "identifier": f"com.github.uanpis.{package_info['name']}",
        "name": package_info["name"],
        "version": package_info["version"],
        "description": package_info.get("description") or "",
        "runtime": {"type": "exec"},
        "actions": [
            {
                "identifier": package_info['name'],
                "name": package_info['name'],
                "description": package_info.get("description") or "",
                "scopes": ["pcb"],
                "entrypoint": binary_filename,
                "show-button": True,
                "icons-light": ["icon.png"],
                "icons-dark": ["icon.png"]
            }
        ]
    }
    manifest_path = dest_dir / "plugin.json"
    with open(manifest_path, "w") as f:
        json.dump(manifest, f, indent=2)
    print(f"Created: {manifest_path}")

def create_metadata_json(dest_dir, package_info, target_triple):
    if target_triple:
        platform_label = target_triple.replace("-", "_")
    else:
        sysname = platform.system().lower()
        arch = platform.machine().lower()
        platform_label = f"{sysname}_{arch}"

    archive_name = f"{package_info['name']}-{package_info['version']}-{platform_label}.zip"
    author_name = package_info["authors"][0] if package_info.get("authors") else "Unknown"

    metadata = {
        "$schema": "https://go.kicad.org/pcm/schemas/v2",
        "name": package_info["name"],
        "identifier": f"com.github.uanpis.{package_info['name']}",
        "version": package_info["version"],
        "description": package_info.get("description") or "",
        "author": {"name": author_name, "contact": ""},
        "license": "GPL",
        "resources": {"icon": "resources/icon.png"},
        "tags": ["pcbnew", "rust"],
        "packages": [
            {
                "platform": "all",
                "sha256": "",
                "download_url": "",
                "archive": archive_name
            }
        ]
    }
    metadata_path = dest_dir / "metadata.json"
    with open(metadata_path, "w") as f:
        json.dump(metadata, f, indent=2)
    print(f"Created: {metadata_path}")

def build_package(target_triple, debug, install=False):
    package_info = get_package_info()
    binary_name = package_info["binary_name"]
    if target_triple is None:
        target_triple = get_current_platform_triple()
        print(f"Using detected platform triple: {target_triple}")

    build_mode = "debug" if debug else "release"
    print(f"Building in {build_mode} mode for target {target_triple}...")
    cargo_args = ["cargo", "build"]
    if build_mode == "release":
        cargo_args.append("--release")
    cargo_args.extend(["--target", target_triple])
    try:
        subprocess.check_call(cargo_args)
    except subprocess.CalledProcessError:
        print("Build failed.")
        sys.exit(1)

    target_dir = get_target_directory(package_info["name"], target_triple, debug)
    binary_filename = binary_name if not platform.system() == "Windows" else f"{binary_name}.exe"
    binary_path = target_dir / binary_filename
    if not binary_path.exists():
        print(f"Error: Binary not found at {binary_path}")
        sys.exit(1)

    with tempfile.TemporaryDirectory() as tmpdir:
        stage_dir = Path(tmpdir)
        plugins_dir = stage_dir / "plugins"
        resources_dir = stage_dir / "resources"
        plugins_dir.mkdir(parents=True, exist_ok=True)
        resources_dir.mkdir(parents=True, exist_ok=True)

        dst_binary = plugins_dir / binary_filename
        shutil.copy2(binary_path, dst_binary)
        print(f"Copied binary to: {dst_binary}")

        create_plugin_manifest(plugins_dir, binary_filename, package_info)

        icon_src = Path(__file__).parent / "resources" / "icon.png"
        if icon_src.exists():
            shutil.copy2(icon_src, resources_dir / "icon.png")
            shutil.copy2(icon_src, plugins_dir / "icon.png")
            print("Copied icon.")
        else:
            print("Warning: icon.png not found. Skipping.")

        create_metadata_json(stage_dir, package_info, target_triple)
        if install:
            kicad_plugins = get_kicad_plugins_dir()
            plugin_subfolder = kicad_plugins / package_info["name"]
            print(f"Installing to: {plugin_subfolder}")
            plugin_subfolder.mkdir(parents=True, exist_ok=True)

            for item in plugins_dir.iterdir():
                dst = plugin_subfolder / item.name
                if item.is_dir():
                    shutil.copytree(item, dst, dirs_exist_ok=True)
                else:
                    shutil.copy2(item, dst)
            print("Installation complete.")
        else:
            zip_name = f"{package_info['name']}-{package_info['version']}-{target_triple.replace('-', '_')}.zip"
            zip_path = Path(__file__).parent / zip_name
            print(f"Creating archive: {zip_path}")
            shutil.make_archive(str(zip_path.with_suffix('')), 'zip', stage_dir)
            print(f"Package created: {zip_path}")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Build and package a KiCad plugin written in Rust.")
    parser.add_argument("--target", help="Target triple (e.g., x86_64-unknown-linux-gnu). If not provided, auto-detected.")
    parser.add_argument("--debug", action="store_true", help="Build in debug mode (default is release).")
    parser.add_argument("--install", action="store_true", help="Install directly to the KiCad plugins directory.")
    args = parser.parse_args()

    build_package(args.target, args.debug, args.install)
