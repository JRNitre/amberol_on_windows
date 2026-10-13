# Amberol for Windows

[简体中文](README.zh-CN.md)

![Amberol icon](data/icons/hicolor/scalable/apps/io.bassi.Amberol.svg)

This is a Windows port of [Amberol](https://gitlab.gnome.org/World/amberol), a simple music player originally created by Emmanuele Bassi. The Windows port is maintained by **JRNitre**. It retains the original project's copyright and license notices.

The port provides native Windows playback, Windows media controls, a seekable waveform, and a Simplified Chinese interface. The portable package includes GTK, GStreamer, translations, and audio plugins for common formats. The source project remains buildable with the MSYS2 UCRT64 toolchain.

## Run the portable build

The feature branch adds album-tinted Acrylic, a tray, a mini player, local LRC lyrics, favorites and playlists, playback session restoration, and continuous playback. See the [Windows feature notes (Chinese)](docs/WINDOWS_FEATURES.zh-CN.md) for usage and validation limits.

1. Obtain `amberol-windows-portable.zip`. A local test package may be present in `release/`; this directory is intentionally excluded from Git.
2. Extract the ZIP completely.
3. Double-click `Amberol.exe` in the extracted folder.

Keep the DLL files and the `lib/` and `share/` directories beside the executable. The portable build does not require a batch file or an MSYS2 installation on the user's computer. It is a folder-based package, not a single-file executable or an installer.

## Build from source

Use a 64-bit Windows system with [MSYS2](https://www.msys2.org/) installed at `C:\msys64`. Open the **MSYS2 UCRT64** terminal and install the build and runtime dependencies:

```sh
pacman -Syu
pacman -S --needed mingw-w64-ucrt-x86_64-toolchain mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-meson mingw-w64-ucrt-x86_64-ninja mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-python mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-libadwaita mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad mingw-w64-ucrt-x86_64-gettext-tools
```

If MSYS2 asks you to restart the terminal after the update, do so before installing packages. From the repository root, run:

```sh
meson setup build-win --prefix="$(cygpath -m /ucrt64)"
meson compile -C build-win
```

To run the development build in the configured environment:

```sh
meson devenv -C build-win build-win/src/amberol.exe
```

## Make a portable ZIP

After the release build completes, run this from the repository root:

```sh
python build-aux/package-windows.py
```

The script creates `dist/amberol-windows-portable/Amberol.exe` and `dist/amberol-windows-portable.zip`. It uses `C:\msys64\ucrt64` by default; pass `--ucrt PATH` if MSYS2 is installed elsewhere. Rebuild and repackage after changing the source. Build outputs and ZIP files are excluded from Git.

## Repository layout

- `src/`: Rust application and Windows media control implementation.
- `data/`: application icons, metadata, and GSettings schema.
- `po/`: translations, including Simplified Chinese.
- `build-aux/package-windows.py`: portable package assembly.
- `LICENSES/`: license texts retained from the original project.

## Credits and license

Amberol was created by **Emmanuele Bassi**. The Windows port is by **JRNitre**.

Built with ChatGPT 6 Sol.

The project is licensed under GPL-3.0-or-later; see [LICENSES/GPL-3.0-or-later.txt](LICENSES/GPL-3.0-or-later.txt). The repository URL for this Windows port has not been set yet. Before public redistribution, review the licenses of the bundled runtime libraries and provide the corresponding source as required by their licenses.
