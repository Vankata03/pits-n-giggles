# MIT License
#
# Copyright (c) [2025] [Ashwin Natarajan]
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.

# ----------------------------------------------------------------------------------------------------------------------

import os
import shutil
import subprocess
import sys
import time
import ast

APP_NAME = "pits_n_giggles"  # or load from the spec file dynamically if needed
COLLECT_DIR_NAME = f"{APP_NAME}_build_tmp"
STAGING_DIST_DIR_NAME = f"{APP_NAME}_dist_tmp"

def remove_dir_if_exists(path: str):
    if os.path.isdir(path):
        shutil.rmtree(path)

def replace_dir_if_possible(src: str, dest: str) -> str:
    if os.path.abspath(src) == os.path.abspath(dest):
        return dest

    if os.path.isdir(dest):
        try:
            shutil.rmtree(dest)
        except PermissionError:
            timestamp = time.strftime("%Y%m%d_%H%M%S")
            backup = f"{dest}_locked_{timestamp}"
            try:
                os.replace(dest, backup)
                print(f"Existing dist directory was in use; moved it to: {backup}")
            except PermissionError:
                print(
                    "Existing dist directory is locked by another process; "
                    f"leaving the new build output in: {src}"
                )
                return src

    try:
        os.replace(src, dest)
    except PermissionError:
        print(
            "Could not replace the dist directory because it is locked by another process; "
            f"leaving the new build output in: {src}"
        )
        return src
    return dest

def build_rust_binary(package_name: str) -> str:
    project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    manifest_path = os.path.join(project_root, "apps", "backend", "rust", "Cargo.toml")
    cargo = shutil.which("cargo")
    if not cargo:
        raise RuntimeError("cargo was not found in PATH. Rust is required for packaged builds.")

    subprocess.run(
        [
            cargo,
            "build",
            "--release",
            "--manifest-path",
            manifest_path,
            "-p",
            package_name,
        ],
        check=True,
    )

    binary_name = f"{package_name}.exe" if os.name == "nt" else package_name
    binary_path = os.path.join(project_root, "apps", "backend", "rust", "target", "release", binary_name)
    if not os.path.isfile(binary_path):
        raise RuntimeError(f"Rust {package_name} binary not found after build: {binary_path}")
    return binary_path

def build_rust_hud_binaries() -> list[str]:
    project_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
    manifest_path = os.path.join(project_root, "apps", "backend", "rust", "Cargo.toml")
    cargo = shutil.which("cargo")
    if not cargo:
        raise RuntimeError("cargo was not found in PATH. Rust is required for packaged builds.")

    subprocess.run(
        [
            cargo,
            "build",
            "--release",
            "--manifest-path",
            manifest_path,
            "-p",
            "hud-renderer",
            "--bins",
        ],
        check=True,
    )

    release_dir = os.path.join(project_root, "apps", "backend", "rust", "target", "release")
    binary_names = ["hud-renderer", "lap_timer", "track_radar", "timing_tower"]
    built_binaries = []
    for binary_name in binary_names:
        filename = f"{binary_name}.exe" if os.name == "nt" else binary_name
        binary_path = os.path.join(release_dir, filename)
        if not os.path.isfile(binary_path):
            raise RuntimeError(f"Rust HUD binary not found after build: {binary_path}")
        built_binaries.append(binary_path)
    return built_binaries

def resolve_pyinstaller_command() -> list[str]:
    pyinstaller = shutil.which("pyinstaller")
    if pyinstaller:
        return [pyinstaller]
    return [sys.executable, "-m", "PyInstaller"]

def verify_rust_binary_packaged(project_root: str, rust_binary: str):
    toc_path = os.path.join(project_root, "build", "png", "PKG-00.toc")
    if not os.path.isfile(toc_path):
        raise RuntimeError(f"Expected PyInstaller TOC not found: {toc_path}")

    with open(toc_path, "r", encoding="utf-8") as f:
        toc_contents = ast.literal_eval(f.read())

    binary_name = os.path.basename(rust_binary)
    expected_path = os.path.normcase(os.path.normpath(rust_binary))
    packaged_entries = toc_contents[2] if isinstance(toc_contents, tuple) and len(toc_contents) >= 3 else []

    packaged = any(
        isinstance(entry, tuple)
        and len(entry) >= 3
        and entry[0] == binary_name
        and os.path.normcase(os.path.normpath(entry[1])) == expected_path
        and entry[2] == "BINARY"
        for entry in packaged_entries
    )

    if not packaged:
        raise RuntimeError(
            f"Rust binary was built, but PyInstaller did not package it into PKG-00.toc: {rust_binary}"
        )

def verify_rust_binaries_packaged(project_root: str, rust_binaries: list[str]):
    for rust_binary in rust_binaries:
        verify_rust_binary_packaged(project_root, rust_binary)

def main():
    script_dir = os.path.dirname(__file__)
    project_root = os.path.abspath(os.path.join(script_dir, ".."))
    spec_path = os.path.join(script_dir, "png.spec")
    dist_dir = os.path.join(project_root, "dist")
    staging_dist_dir = os.path.join(project_root, STAGING_DIST_DIR_NAME)
    collect_dir = os.path.join(staging_dist_dir, COLLECT_DIR_NAME)

    # 0. Cleanup previous files
    remove_dir_if_exists(os.path.join(project_root, "build"))
    remove_dir_if_exists(staging_dist_dir)

    # 1. Build the Rust backend companion binary
    rust_backend_binary = build_rust_binary("backend")
    rust_hud_binaries = build_rust_hud_binaries()

    # 2. Run PyInstaller
    start_time = time.time()
    env = os.environ.copy()
    env["PNG_RUST_BACKEND_BIN"] = rust_backend_binary
    env["PNG_HUD_RENDERER_BIN"] = os.path.dirname(rust_hud_binaries[0])
    env["PNG_PROJECT_ROOT"] = project_root
    subprocess.run(
        [
            *resolve_pyinstaller_command(),
            "--clean",
            "--noconfirm",
            "--distpath",
            staging_dist_dir,
            spec_path,
        ],
        check=True,
        env=env,
    )
    verify_rust_binary_packaged(project_root, rust_backend_binary)
    verify_rust_binaries_packaged(project_root, rust_hud_binaries)

    # 3. Cleanup the custom COLLECT dir
    remove_dir_if_exists(collect_dir)
    final_dist_dir = replace_dir_if_possible(staging_dist_dir, dist_dir)

    end_time = time.time()
    elapsed = end_time - start_time
    print(f"\n Build completed in {elapsed:.2f} seconds.")
    print(f" Output available in: {final_dist_dir}")

if __name__ == "__main__":
    main()
