# Amberol Windows 移植版

本目录是 Amberol 的独立 Windows 移植版源码仓库。Windows 移植：JRNitre；Amberol 原项目作者：Emmanuele Bassi。保留原项目的许可证和版权声明。

## 目录

- `src/`、`data/`、`po/`：程序源码、界面资源和翻译。
- `build-aux/package-windows.py`：收集 Windows 运行库、音频插件和资源，生成便携包。
- `release/amberol-windows-portable.zip`：已构建的便携包，供本地测试；该目录不纳入 Git 历史。
- `LICENSES/`：原项目许可证文件。

## 构建与打包

在安装好 MSYS2 UCRT64 依赖后，按 `README.md` 的 Windows development build 章节配置并构建。之后在仓库根目录运行：

```text
python build-aux/package-windows.py
```

脚本生成 `dist/amberol-windows-portable/Amberol.exe` 和 `dist/amberol-windows-portable.zip`。用户解压 ZIP 后直接双击 `Amberol.exe`，并保留同目录的 DLL、`lib/` 和 `share/`。

GitHub 地址尚未确定；`src/application.rs` 中的 `WINDOWS_PORT_GITHUB_URL` 留待发布时填写。
