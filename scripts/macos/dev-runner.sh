#!/bin/bash
# macOS 上 `tauri dev` 的 cargo runner（见 src-tauri/tauri.macos.conf.json）。
# 未打包的可执行文件在 Dock / 强制退出里只能显示文件名（huijing）；可执行文件又不能取中文名
# （中文名会让 CoreFoundation 崩溃）。所以编译后把它装进一个最小的「绘境.app」再启动，
# 显示名与图标就和正式包一致。`tauri build` 等其他子命令原样交给 cargo。
set -euo pipefail
if [ "${1:-}" != "run" ]; then exec cargo "$@"; fi
shift

cargo_args=(); app_args=(); after=0
for a in "$@"; do
  if [ $after = 1 ]; then app_args+=("$a"); elif [ "$a" = "--" ]; then after=1; else cargo_args+=("$a"); fi
done
cargo build "${cargo_args[@]}"

profile=debug
for a in "${cargo_args[@]}"; do [ "$a" = "--release" ] && profile=release; done
target="${CARGO_TARGET_DIR:-target}/$profile"
app="$target/绘境.app/Contents"
mkdir -p "$app/MacOS" "$app/Resources"
cp -f "$target/huijing" "$app/MacOS/huijing"
cp -f icons/icon.icns "$app/Resources/icon.icns"
cat > "$app/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>huijing</string>
  <key>CFBundleIdentifier</key><string>com.aowu.huijing</string>
  <key>CFBundleName</key><string>绘境</string>
  <key>CFBundleDisplayName</key><string>绘境</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
exec "$app/MacOS/huijing" ${app_args[@]+"${app_args[@]}"}
