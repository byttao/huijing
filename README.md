# 绘境

桌面 AI 出图工具：对接 [GRSAI](https://grsai.com/zh/models) 的出图接口，支持**文生图 / 图生图 / 继续修改**三种模式，Rust 端任务队列并发出图并自动下载到本地。

界面：Tauri 2 + React + TypeScript + Ant Design 6，按 [ui-kit](https://github.com/byttao/ui-kit)（v1.2.0，`src/ui-kit/`）规范搭建。后端：Rust（`src-tauri/src/`）。

## 功能

- **出图**：左侧任务队列，右侧参数栏，底部日志。
  - 文生图：只写提示词。
  - 图生图：提示词 + 本地参考图（拖到窗口任意位置，或点击选择；可多张，最多 8 张）。参考图在 Rust 端读取并转 base64，长边超过 2048 先缩小。
  - 继续修改：队列或图库里任一已完成的图点「继续修改」，结果图作为参考图带入，沿用原模型与参数；新任务记录来源（`parentId`），列表里显示「改自「…」」。
  - 张数 1–4：展开成多个独立任务，分别排队、分别计费。
  - 提示词框里 `⌘/Ctrl + Enter` 直接加入队列。
- **任务队列**（Rust 端，`src-tauri/src/queue.rs`）
  - 状态：排队中 / 生成中（异步任务带百分比）/ 下载中 / 已完成 / 失败 / 下载失败 / 已取消。
  - 并发数 1–5（默认 2），超出的排队。
  - 取消（排队中直接取消；进行中中断并丢弃结果）、失败重试、**仅重试下载**（沿用结果 URL，不重新扣费）、删除记录、清空已结束（失败 / 取消的删除，已完成的只从队列移除、仍在图库）。
  - 队列与历史存在 `tasks.json`；重启后：已完成的保留；生成中的异步任务（有任务 id）继续轮询，下载中的继续下载，其余中断的标为失败可重试。
  - 成功后自动下载到输出目录，文件名 `20261113-173012_nano-banana-2.1_01.png`（时间_模型_序号），不覆盖已有文件（重名加 ` (2)`）。
  - 接口错误转成中文提示（Key 无效、余额不足、内容违规、超时、网络、结果过期…）。
- **图库**：已完成图片网格，双击或「大图预览」用 antd Image 预览（可左右翻），继续修改、打开所在文件夹、复制提示词、删除记录。
- **设置**：API Key（可显隐、测试连接不扣费）、节点、代理、输出目录、并发数、默认模型与参数、主题与明暗、版本与官网链接。

## 接口说明

鉴权：`Authorization: Bearer <API Key>`。节点：国内 `https://grsai.dakka.com.cn`（默认）、全球 `https://grsaiapi.com`。

| 模型 | 场景 | 接口 | 说明 |
|---|---|---|---|
| `gpt-image-2.6` | 文生图 | `POST /v1/images/generations` | OpenAI 格式，同步返回 `data[].url`，约 40 秒；`size` 由比例换算为 1K 像素值，`quality` 只能 `auto`；可选 `background: transparent`、`output_format: gif/svg` |
| `gpt-image-2.6` | 带参考图 | `POST /v1/api/generate` + 轮询 | `images[]` 传 data URL，`aspectRatio`、`quality`、`background`、`outputFormat`，`replyType: async` |
| `nano-banana-2.1` | 全部 | `POST /v1/api/generate` + 轮询 | `images[]`、`aspectRatio`、`imageSize: 1K/2K/4K`、`replyType: async` |

- 轮询：`GET /v1/api/result?id=<id>`，`status` 为 `running / succeeded / failed / violation`，带 `progress`。结果约 2 小时内有效。
- 为什么 GPT 带参考图改走异步接口：实测 generations 的 `image` 数组传 data URL 时，请求挂起约 9 分钟后连接被断开、拿不到结果；异步接口明确支持 base64，实测约 25 秒出图，且有进度、可在重启后续上。
- 测试连接（不扣费）：向 generations 提交空提示词。Key 有效返回 `prompt is empty`，无效返回 `apikey error`。（`/v1/api/result` 查询接口不校验 Key，不能用来测。）
- 结果图在 `fileN.aitohumanize.com`。如果开着 Clash 等代理且图片域名走代理出口，可能出现 TLS 握手失败：此时任务停在「下载失败」，调整网络后点「重试下载」即可，不重新扣费。

代码位置：`src-tauri/src/api/`（`gpt_image.rs`、`nano_banana.rs`、`async_task.rs`、`error.rs`）。

## 设置项与数据位置

| 设置 | 默认 | 说明 |
|---|---|---|
| API Key | 空 | 只保存在本机设置文件里，不进仓库 |
| 节点 | 国内 | 国内 / 全球 |
| 代理 | 不使用 | 不使用 / 跟随系统 / 自定义（`http://` 或 `socks5://`）；可分别勾选用于「API 请求」（默认勾）与「图片下载」（默认不勾） |
| 输出目录 | `图片/绘境` | 可选择、可在访达 / 资源管理器中打开 |
| 同时生成 | 2 | 1–5 |
| 默认模型 / 参数 / 张数 | Nano Banana 2.1 · 自动比例 · 1K · 1 张 | 出图页启动时套用 |
| 主题 | 森翠雅韵 · 浅色 | 即时生效，同时写进设置文件 |

数据（`settings.json`、`tasks.json`）：

- macOS：`~/Library/Application Support/com.aowu.huijing/`
- Windows 便携版：exe 同目录的 `绘境数据/`，整个文件夹拷走即用（exe 所在目录不可写时退回 `%APPDATA%\com.aowu.huijing\`）。

## 开发

```bash
npm install
npm run tauri dev      # 开发运行（Vite 端口 1460）
npm run build          # 前端类型检查 + 打包
antd lint ./src --format json
```

Rust 测试与检查：

```bash
cd src-tauri
cargo test
cargo clippy --all-targets
```

单元测试覆盖：两个模型的请求体构造与路由、响应解析与错误归类、队列状态流转与重启恢复、并发调度、文件命名与不覆盖、参考图缩放编码、设置读写。

## 打包

### 本机

macOS 通用版（arm64 + x86_64）：

```bash
npx tauri build --target universal-apple-darwin --bundles app
bash scripts/macos/make-dmg.sh "" 打包产物
```

DMG 内含 `绘境.app`、`Applications` 快捷方式、`通用安装-去隔离.command` 与 `安装说明.txt`。

Windows 便携版（在 macOS 上用 cargo-xwin 交叉编译，不打安装包）：

```bash
npx tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --no-bundle
cp src-tauri/target/x86_64-pc-windows-msvc/release/huijing.exe "打包产物/绘境_0.1.0_便携版.exe"
```

便携版依赖系统自带的 WebView2（Windows 11 自带，Windows 10 通常已随 Edge 安装）；发布版是 GUI 子系统，不弹控制台窗口。

### GitHub Actions

`.github/workflows/build.yml`：在 Actions 页面手动运行「打包」，或推送 `v*` 标签（同时发布 Release）。

## 应用图标

`src-tauri/app-icon.svg`：森翠绿圆角底板 + 米色画框山水 + 金色星光笔触；侧栏品牌标记（`src/components/BrandMark.tsx`）用同一图形的单色版。改完后重新生成：

```bash
qlmanage -t -s 1024 -o /tmp src-tauri/app-icon.svg && cp /tmp/app-icon.svg.png src-tauri/app-icon.png
npx tauri icon src-tauri/app-icon.png && rm -rf src-tauri/icons/android src-tauri/icons/ios
```
