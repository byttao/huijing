#!/bin/bash
# 用本机打好的两个产物创建 GitHub Release（不经过 GitHub Actions）。
# 用法：scripts/release.sh <版本> [--dry-run]
#   例：scripts/release.sh 0.1.1
#   --dry-run：只做全部校验、准备附件并打印发布说明，不打标签、不创建 Release。
#
# 前提：
#   - 工作区干净，main 已推送（本地 main 与 origin/main 一致）；
#   - package.json / Cargo.toml / tauri.conf.json 的版本都等于 <版本>，CHANGELOG.md 有「## <版本> 」章节；
#   - 打包产物/ 里有 绘境_<版本>_universal.dmg 与 绘境_<版本>_便携版.exe（见 README「本机打包」）；
#   - gh 已登录，且标签 v<版本> 与同名 Release 都还不存在。
# 附件改成 ASCII 文件名 Huijing_<版本>_universal.dmg / Huijing_<版本>_portable.exe（GitHub 会丢掉附件名里的中文），
# 中文写在附件的显示标签里。标签 v<版本> 由 gh 在 main 的最新提交上创建。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:-}"
DRY_RUN=0
[ "${2:-}" = "--dry-run" ] && DRY_RUN=1
fail() { echo "✗ $*" >&2; exit 1; }
[ -n "$VERSION" ] || fail "用法：scripts/release.sh <版本> [--dry-run]，例：scripts/release.sh 0.1.1"
VERSION="${VERSION#v}"
TAG="v$VERSION"

# 1. 工作区与分支
[ -z "$(git status --porcelain)" ] || fail "工作区有未提交的改动，先提交或清理"
[ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || fail "当前不在 main 分支"
git fetch -q origin main
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || fail "本地 main 与 origin/main 不一致，先 git push origin main（或拉取）"

# 2. 版本号
CONF_V=$(node -p "require('./src-tauri/tauri.conf.json').version")
PKG_V=$(node -p "require('./package.json').version")
CARGO_V=$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)
[ "$CONF_V" = "$VERSION" ] || fail "tauri.conf.json 的版本是 $CONF_V，与 $VERSION 不一致"
[ "$PKG_V" = "$VERSION" ] && [ "$CARGO_V" = "$VERSION" ] || fail "版本不一致：package.json=$PKG_V Cargo.toml=$CARGO_V tauri.conf.json=$CONF_V"
grep -q "^## ${VERSION} " CHANGELOG.md || fail "CHANGELOG.md 里没有「## ${VERSION} 」章节"

# 3. 标签与 Release 不能已存在
git rev-parse -q --verify "refs/tags/$TAG" >/dev/null && fail "本地已有标签 $TAG"
git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1 && fail "远端已有标签 $TAG"
gh release view "$TAG" >/dev/null 2>&1 && fail "已有 Release $TAG"

# 4. 产物
DMG_SRC="打包产物/绘境_${VERSION}_universal.dmg"
EXE_SRC="打包产物/绘境_${VERSION}_便携版.exe"
[ -f "$DMG_SRC" ] || fail "找不到 $DMG_SRC"
[ -f "$EXE_SRC" ] || fail "找不到 $EXE_SRC"
COMMIT_TIME=$(git log -1 --format=%ct)
for f in "$DMG_SRC" "$EXE_SRC"; do
  if [ "$(stat -f %m "$f")" -lt "$COMMIT_TIME" ]; then
    echo "! 提醒：$f 早于最新提交，确认它是用当前代码打的包" >&2
  fi
done

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
DMG="$STAGE/Huijing_${VERSION}_universal.dmg"
EXE="$STAGE/Huijing_${VERSION}_portable.exe"
cp "$DMG_SRC" "$DMG"
cp "$EXE_SRC" "$EXE"

# 5. 发布说明：CHANGELOG 对应章节 + 下载与安装
NOTES="$STAGE/notes.md"
awk -v v="$VERSION" '
  index($0, "## " v " ") == 1 { found = 1; next }
  found && /^## / { exit }
  found { print }
' CHANGELOG.md > "$NOTES"
cat >> "$NOTES" <<EOF

## 下载与安装

- **macOS**（Apple 芯片与 Intel 通用，需要 macOS 10.15 或更新）：下载 \`Huijing_${VERSION}_universal.dmg\`，打开后双击「通用安装-去隔离.command」，它会把「绘境.app」复制到「应用程序」、清除隔离属性并打开。若提示无法打开这个脚本：右键 → 打开。也可以把「绘境.app」拖到「Applications」，首次打开提示无法验证开发者时在终端执行 \`xattr -cr "/Applications/绘境.app"\`。
- **Windows**（便携版，不需要安装）：下载 \`Huijing_${VERSION}_portable.exe\`，放到任意文件夹双击运行。设置、队列与历史保存在 exe 同目录的「绘境数据」文件夹里，整个文件夹拷走即用。依赖系统自带的 WebView2（Windows 11 自带，Windows 10 通常已随 Edge 安装）。
- **首次使用**：到「设置」填写 GRSAI 的 API Key，点「测试连接」（不扣费）确认可用。生成的图片默认保存在「图片/绘境」文件夹。
EOF

echo "── 发布说明 ──────────────────────────"
cat "$NOTES"
echo "──────────────────────────────────────"
ls -l "$DMG" "$EXE"
echo "标签：$TAG（main @ $(git rev-parse --short HEAD)）  标题：绘境 $VERSION"

if [ "$DRY_RUN" = 1 ]; then
  echo "✓ 校验通过（--dry-run：未打标签、未创建 Release）"
  exit 0
fi

gh release create "$TAG" \
  --target main \
  --title "绘境 $VERSION" \
  --notes-file "$NOTES" \
  "$DMG#绘境 $VERSION · macOS 通用版（DMG）" \
  "$EXE#绘境 $VERSION · Windows 便携版（exe）"
echo "✓ 已发布：$(gh release view "$TAG" --json url -q .url)"
