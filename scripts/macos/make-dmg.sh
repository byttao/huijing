#!/bin/bash
# 把 tauri build 生成的「绘境.app」与安装脚本、安装说明一起打成 DMG。
# 用法：scripts/macos/make-dmg.sh [bundle 目录] [输出目录] [DMG 文件名]
#   默认 bundle 目录：src-tauri/target/universal-apple-darwin/release/bundle/macos
#   默认输出目录：dist-mac
#   默认文件名：<productName>_<version>_universal.dmg（如「绘境_0.1.1_universal.dmg」）；
#   CI 发布用 ASCII 文件名（GitHub Release 会丢掉附件名里的中文），传 Huijing_<version>_universal.dmg
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUNDLE_DIR="${1:-$ROOT/src-tauri/target/universal-apple-darwin/release/bundle/macos}"
OUT="${2:-$ROOT/dist-mac}"
DMG_NAME="${3:-}"
CONF="$ROOT/src-tauri/tauri.conf.json"
NAME="$(node -p "require('$CONF').productName")"
VERSION="$(node -p "require('$CONF').version")"
APP="$BUNDLE_DIR/$NAME.app"
[ -d "$APP" ] || { echo "找不到 $APP" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

ditto "$APP" "$STAGE/$NAME.app"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/scripts/macos/通用安装-去隔离.command" "$STAGE/"
cp "$ROOT/scripts/macos/安装说明.txt" "$STAGE/"
chmod +x "$STAGE/通用安装-去隔离.command"

mkdir -p "$OUT"
DMG="$OUT/${DMG_NAME:-${NAME}_${VERSION}_universal.dmg}"
rm -f "$DMG"
hdiutil create -volname "$NAME" -srcfolder "$STAGE" -fs HFS+ -format UDZO -ov "$DMG"
echo "已生成：$DMG"
