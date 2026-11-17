# MugenZip

跨平台压缩包管理器：浏览、解压、压缩，macOS 额外提供桌面悬浮球和状态栏入口。

## 下载

| 平台 | 产物 |
| --- | --- |
| macOS Apple Silicon | `MugenZip-<version>-macos-arm64.dmg` |
| Windows x64 | `MugenZip-<version>-windows-x64-setup.exe` |
| Linux x64 | `mugenzip_<version>_amd64.deb` / `mugenzip-<version>-linux-x64.tar.gz` |

### macOS 说明

当前构建使用 ad-hoc 签名，没有 Apple Developer ID，也没有公证。首次打开会被 Gatekeeper 拦截：

1. 打开 DMG，把 `MugenZip.app` 和 `MugenZipWidget.app` 拖进「应用程序」。
2. 在 Finder 里右键 `MugenZip.app`，选择「打开」。
3. 如果仍被拦截，执行：

```bash
xattr -dr com.apple.quarantine /Applications/MugenZip.app
xattr -dr com.apple.quarantine /Applications/MugenZipWidget.app
```

悬浮球需要和主程序一起安装；只装主程序时，状态栏与悬浮菜单不可用。

### Windows 说明

安装包没有代码签名，SmartScreen 可能提示「未知发布者」。选择「更多信息」后仍可继续安装。

### Linux 说明

`.deb` 用系统包管理器安装：

```bash
sudo apt install ./mugenzip_<version>_amd64.deb
```

便携版解压后直接运行目录里的 `mugenzip`。

## 从源码构建

需要 Rust 1.99.1。Cove 以 `v0.3.2` 标签作为 git 依赖，首次构建会联网拉取。

```bash
git clone https://github.com/AlexCat315/RichZip.git
cd RichZip
cargo build --release
```

macOS 打包还需要同级目录的 Cove 源码，用来构建 `cove-package`：

```bash
git clone --branch v0.3.2 https://github.com/AlexCat315/Cove.git ../Cove
COVE_ROOT=../Cove bash scripts/package-macos.sh --release
```

## 许可

GPL-3.0，见 `LICENSE`。
