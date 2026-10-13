# Amberol Windows 移植版

[English](README.md)

![Amberol 图标](data/icons/hicolor/scalable/apps/io.bassi.Amberol.svg)

这是 [Amberol](https://gitlab.gnome.org/World/amberol) 音乐播放器的 Windows 移植版。Amberol 原项目由 **Emmanuele Bassi** 创建，Windows 移植与维护者为 **JRNitre**。本仓库保留了原项目的版权和许可证声明。

移植版提供 Windows 原生播放、系统媒体控制、可拖动定位的波形，以及简体中文界面。便携包附带 GTK、GStreamer、翻译文件和常见音频格式所需的插件。源码使用 MSYS2 UCRT64 工具链构建。

## 使用便携包

1. 获取 `amberol-windows-portable.zip`。本地测试包可能放在 `release/`，该目录不纳入 Git 历史。
2. 完整解压 ZIP。
3. 双击解压目录中的 `Amberol.exe`。

请保留程序旁的 DLL 文件以及 `lib/`、`share/` 目录。用户运行便携包时不需要点击 `.bat`，也不需要安装 MSYS2。这个包是可解压运行的文件夹，尚不是单文件程序或安装程序。

## 从源码构建

在 64 位 Windows 上将 [MSYS2](https://www.msys2.org/) 安装到 `C:\msys64`，打开 **MSYS2 UCRT64** 终端，安装构建与运行依赖：

```sh
pacman -Syu
pacman -S --needed mingw-w64-ucrt-x86_64-toolchain mingw-w64-ucrt-x86_64-rust mingw-w64-ucrt-x86_64-meson mingw-w64-ucrt-x86_64-ninja mingw-w64-ucrt-x86_64-pkgconf mingw-w64-ucrt-x86_64-python mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-libadwaita mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad mingw-w64-ucrt-x86_64-gettext-tools
```

如果更新 MSYS2 后提示重启终端，先重启再安装依赖。在仓库根目录执行：

```sh
meson setup build-win --prefix="$(cygpath -m /ucrt64)"
meson compile -C build-win
```

需要在开发环境中运行时，可执行：

```sh
meson devenv -C build-win build-win/src/amberol.exe
```

## 打包便携版

完成构建后，在仓库根目录执行：

```sh
python build-aux/package-windows.py
```

脚本生成 `dist/amberol-windows-portable/Amberol.exe` 和 `dist/amberol-windows-portable.zip`。默认使用 `C:\msys64\ucrt64`；若 MSYS2 安装在别处，可传入 `--ucrt PATH`。修改源码后需要重新构建并打包。构建产物和 ZIP 不会提交到 Git。

## 仓库结构

- `src/`：Rust 程序和 Windows 系统媒体控制实现。
- `data/`：图标、应用元数据和 GSettings 配置。
- `po/`：翻译文件，包括简体中文。
- `build-aux/package-windows.py`：便携包打包脚本。
- `LICENSES/`：保留的原项目许可证文本。

## 作者与许可

Amberol 原项目作者：**Emmanuele Bassi**；Windows 移植：**JRNitre**。项目采用 GPL-3.0-or-later，详见 [许可证文本](LICENSES/GPL-3.0-or-later.txt)。Windows 移植版的 GitHub 地址尚未设置。正式公开分发前，还需核对随包运行库的许可要求，并按相应许可提供源码。
