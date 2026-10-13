#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Amberol contributors
# SPDX-License-Identifier: GPL-3.0-or-later

"""Assemble a local Windows runtime from an MSYS2 UCRT64 installation."""

import argparse
from collections import deque
from pathlib import Path
import re
import shutil
import subprocess


PLUGINS = (
    "app", "audioconvert", "audiofx", "audioparsers", "audioresample", "autodetect", "coreelements",
    "directsound", "fdkaac", "flac", "id3demux", "isomp4", "level",
    "mpg123", "ogg", "opus", "playback", "taglib", "typefindfunctions",
    "volume", "vorbis", "wasapi", "wasapi2", "wavparse",
)
PIXBUF_LOADERS = ("jpeg", "png", "svg")


def imports(objdump: Path, binary: Path) -> set[str]:
    result = subprocess.run(
        [str(objdump), "-p", str(binary)], capture_output=True, text=True, check=True
    )
    return set(re.findall(r"DLL Name:\s*(\S+)", result.stdout, re.I))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ucrt", type=Path, default=Path("C:/msys64/ucrt64"))
    parser.add_argument("--build", type=Path, default=Path("build-win"))
    parser.add_argument("--output", type=Path, default=Path("dist/amberol-windows-portable"))
    args = parser.parse_args()

    ucrt, build, output = args.ucrt.resolve(), args.build.resolve(), args.output.resolve()
    binary = build / "src/amberol.exe"
    if not binary.is_file():
        parser.error(f"Build the application first: {binary}")
    if not (ucrt / "bin/objdump.exe").is_file():
        parser.error(f"MSYS2 UCRT64 not found: {ucrt}")

    bin_dir = output
    bin_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, bin_dir / "Amberol.exe")
    shutil.copy2(build / "src/amberol.gresource", bin_dir / "amberol.gresource")

    queue = deque([binary])
    plugin_dir = ucrt / "lib/gstreamer-1.0"
    bundled_plugins = output / "lib/gstreamer-1.0"
    bundled_plugins.mkdir(parents=True, exist_ok=True)
    for name in PLUGINS:
        src = plugin_dir / f"libgst{name}.dll"
        if not src.is_file():
            raise FileNotFoundError(src)
        shutil.copy2(src, bundled_plugins / src.name)
        queue.append(src)

    loader_dir = ucrt / "lib/gdk-pixbuf-2.0/2.10.0/loaders"
    bundled_loaders = output / "lib/gdk-pixbuf-2.0/2.10.0/loaders"
    bundled_loaders.mkdir(parents=True, exist_ok=True)
    for name in PIXBUF_LOADERS:
        src = loader_dir / ("pixbufloader_svg.dll" if name == "svg" else f"libpixbufloader-{name}.dll")
        shutil.copy2(src, bundled_loaders / src.name)
        queue.append(src)

    available = {path.name.lower(): path for path in (ucrt / "bin").glob("*.dll")}
    copied = set()
    while queue:
        for name in imports(ucrt / "bin/objdump.exe", queue.popleft()):
            key = name.lower()
            if key in copied or key not in available:
                continue  # Windows system DLL or already bundled.
            src = available[key]
            shutil.copy2(src, bin_dir / src.name)
            copied.add(key)
            queue.append(src)

    schemas = output / "share/glib-2.0/schemas"
    schemas.mkdir(parents=True, exist_ok=True)
    for src in (ucrt / "share/glib-2.0/schemas").glob("*.xml"):
        shutil.copy2(src, schemas / src.name)
    shutil.copy2(Path("data/io.bassi.Amberol.gschema.xml"), schemas / "io.bassi.Amberol.gschema.xml")
    subprocess.run([str(ucrt / "bin/glib-compile-schemas.exe"), "--strict", str(schemas)], check=True)

    for theme in ("Adwaita", "hicolor"):
        shutil.copytree(ucrt / "share/icons" / theme, output / "share/icons" / theme, dirs_exist_ok=True)
    shutil.copytree(Path("data/icons/hicolor"), output / "share/icons/hicolor", dirs_exist_ok=True)
    subprocess.run(
        [str(ucrt / "bin/gtk4-update-icon-cache.exe"), "-f", str(output / "share/icons/hicolor")],
        check=True,
    )

    translations = output / "share/locale/zh_CN/LC_MESSAGES"
    translations.mkdir(parents=True, exist_ok=True)
    shutil.copy2(build / "po/zh_CN/LC_MESSAGES/amberol.mo", translations / "amberol.mo")
    for domain in ("glib20", "gtk40", "libadwaita", "gdk-pixbuf"):
        src = ucrt / "share/locale/zh_CN/LC_MESSAGES" / f"{domain}.mo"
        if src.is_file():
            shutil.copy2(src, translations / src.name)

    shutil.copytree(Path("LICENSES"), output / "LICENSES", dirs_exist_ok=True)
    shutil.copy2(Path("README.md"), output / "README.md")

    archive = shutil.make_archive(
        str(output), "zip", root_dir=output.parent, base_dir=output.name
    )
    print(f"Packaged {len(copied)} runtime DLLs in {output}")
    print(f"Archive: {archive}")


if __name__ == "__main__":
    main()
