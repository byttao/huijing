# 绘境

桌面 AI 出图工具：对接 [GRSAI](https://grsai.com/zh/models) 的出图接口，支持**文生图 / 图生图 / 继续修改**三种模式，Rust 端任务队列并发出图并自动下载到本地。

界面：Tauri 2 + React + TypeScript + Ant Design 6，按 [ui-kit](https://github.com/byttao/ui-kit)（v1.4.0，`src/ui-kit/`）规范搭建。后端：Rust（`src-tauri/src/`）。

## 功能

- **出图**：左侧任务队列，右侧参数栏，底部日志。
  - 文生图：只写提示词。
  - 图生图：提示词 + 本地参考图（拖到窗口任意位置，或点击选择；可多张，最多 8 张）。参考图在 Rust 端读取并转 base64，长边超过 2048 先缩小。
  - 继续修改：队列或图库里任一已完成的图点「继续修改」，结果图作为参考图带入，沿用原模型与参数；新任务记录来源（`parentId`），列表里显示「改自「…」」。
  - **多合一**（张数 2–4）：一次请求让模型在一张图里按宫格画出 N 个不同方案，只扣一次费、任务仍是 1 个；整张图就是所选比例，下载后在 Rust 端按宫格等分拆成 N 张（合成原图也保留）。三种模式都支持。详见下文「多合一」。
  - 比例选择带预览：下拉项里每个比例有按真实宽高比画的小矩形；参数区的预览框就是整张图的形状，多合一时画出宫格等分线，并提示「拆切后每张约 x:y（约 W×H）」。
  - 每条任务有持久的自增序号（#12，删除后不复用；旧记录按创建时间补号），多合一的切片是 #12-1 … ；「改自 #7」/「改自 #7-2」可点击，滚动到原记录并高亮闪一下（原记录已移出队列时去图库定位，已删除时置灰）。
  - 提示词框里 `⌘/Ctrl + Enter` 直接加入队列。
- **任务队列**（Rust 端，`src-tauri/src/queue.rs`）
  - 状态：排队中 / 生成中（异步任务带百分比）/ 下载中 / 已完成 / 失败 / 下载失败 / 已取消。
  - 并发数 1–5（默认 2），超出的排队。
  - 取消（排队中直接取消；进行中中断并丢弃结果）、失败重试、**仅重试下载**（沿用结果 URL，不重新扣费）、删除记录、清空已结束（失败 / 取消的删除，已完成的只从队列移除、仍在图库）。
  - 队列与历史存在 `tasks.json`；重启后：已完成的保留；生成中的异步任务（有任务 id）继续轮询，下载中的继续下载，其余中断的标为失败可重试。
  - 成功后自动下载到输出目录，文件名 `20261113-173012_nano-banana-2.1_01.png`（时间_模型_序号），不覆盖已有文件（重名加 ` (2)`）。
  - 下载是流式的：队列行显示「下载中 45% · 1.5 / 3.3 MB」与进度条；先写 `xxx.png.part`，下载完成后才原子改名为 `xxx.png`，下载中途文件夹里不会出现打不开的半截图片。
  - 接口错误转成中文提示（Key 无效、余额不足、内容违规、超时、网络、结果过期…）。
- **多合一**（`src-tauri/src/grid.rs`、`split.rs`）
  - 所选比例就是实际请求、实际出图（整张图）的比例；多合一只是把这张图按宫格等分拆切。每格比例 = (a/cols):(b/rows)，约分后显示。4 张固定 2×2；2 张、3 张可在横排 / 竖排间切换，默认取每张更接近方形的一种（横图并排、竖图上下叠）。比例为「自动」也可以多合一，每张比例取决于实际出图。GPT 输出 GIF / SVG 时不能多合一。各比例的结果见 [docs/多合一布局表.md](docs/多合一布局表.md)。
  - 提示词自动包一层宫格指令（N 个不同方案、几列几行、每格比例约为多少、格间无边框无留白无文字）；任务同时保存原始提示词与实际发送的提示词，界面显示原始的。
  - 拆切：按宫格等分，在每条格线附近窄带里先检测整列 / 整行近似纯色的分隔线并裁掉；没有分隔线时找相邻行 / 列差异明显最大的内容接缝（实测模型的格线可能偏离等分位置 10 像素）；内部边再内缩 0.4%。不再按目标比例裁切。
  - 「拆切后放大到整张图的长边」（设置，默认开）：每张用 [fast_image_resize](https://crates.io/crates/fast_image_resize) 的 Lanczos3（运行时自动启用 SSE4.1 / AVX2 / NEON）等比放大，使长边等于整张图的长边（以下载到的合成图为准）。参考图预压缩也用同一个缩放器。
  - Nano「合图使用更高一档分辨率」（默认关）：如 1K 档用 2K 请求，放大目标仍按原档位整张图的长边；4 合 1 时切出来正好是原生 1K。高档位单价可能更高。
  - 文件：`…_01-宫格.png`（合成原图）、`…_01-1.png` … `…_01-N.png`（拆切后）。队列行显示合成图与各张缩略图（#12-1 …）；「继续修改」可选其中任意一张。
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
| 拆切后放大到整张图的长边 | 开 | 多合一等分后每张等比放大，长边等于整张图的长边（Lanczos3） |
| 主题 | 森翠雅韵 · 浅色 | 即时生效，同时写进设置文件 |

数据（`settings.json`、`tasks.json`）：

- macOS：`~/Library/Application Support/com.aowu.huijing/`
- Windows 便携版：exe 同目录的 `绘境数据/`，整个文件夹拷走即用（exe 所在目录不可写时退回 `%APPDATA%\com.aowu.huijing\`）。

## 应用名称

所有地方都显示「绘境」：窗口标题、macOS 的 Dock / 菜单栏 / 访达（`productName` + `src-tauri/Info.plist` 的 `CFBundleName`、`CFBundleDisplayName`）、Windows exe 版本资源的 `FileDescription` / `ProductName`。

可执行文件名保持 ASCII（`huijing`）：实测中文可执行文件名会让 macOS 的 CoreFoundation 在 `NSBundle bundleWithIdentifier`（wry 启动时调用）里崩溃——未打包时 SIGTRAP、放进 .app 后 SIGSEGV；同一个二进制改成 ASCII 名就正常。

开发运行时，未打包的可执行文件在 Dock 里只能显示文件名，所以 macOS 的 `tauri dev` 通过 `src-tauri/tauri.macos.conf.json` 配置的 runner（`scripts/macos/dev-runner.sh`）把编译出的二进制装进一个最小的 `target/debug/绘境.app` 再启动，Dock、菜单栏显示「绘境」和应用图标，与正式包一致。

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

单元测试覆盖：两个模型的请求体构造与路由、响应解析与错误归类、队列状态流转与重启恢复、并发调度、文件命名与不覆盖、参考图缩放编码、设置读写、多合一布局（两个模型 × 各比例 × N=2/3/4）、提示词包装、宫格拆切（含偏移的分隔线）与 Lanczos3 缩放。

真实出图验证多合一（会扣费，与应用同一套规划 / 提示词 / 接口 / 下载 / 拆切代码）：

```bash
HUIJING_KEY=sk-… MODEL=nano-banana-2.1 RATIO=16:9 COUNT=2 LAYOUT=row PROMPT="…" OUT=/tmp/multi \
  cargo test real_multi -- --ignored --nocapture
```

用真实合成图离线验证拆切：

```bash
COMPOSITE=~/Pictures/绘境/xxx-宫格.png MODEL=nano-banana-2.1 RATIO=16:9 TIER=1K COUNT=4 OUT=/tmp/split \
  cargo test resplit_real -- --ignored --nocapture
```

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

`src-tauri/app-icon.svg`：森翠绿圆角底板 + 米色画框山水 + 金色星光笔触，按 macOS 惯例在 1024 画布里留边（底板 824×824 居中、圆角 185），底板外透明。改完后重新生成并校验透明：

```bash
bash src/ui-kit/scripts/icon/make-icons.sh src-tauri/app-icon.svg   # AppKit 渲染透明底 → tauri icon → 逐尺寸校验四角透明
```

侧栏品牌标记用 `src/assets/brand-mark.png`（同一图形裁到底板、去阴影，不带 macOS 留边），以 `brand.mark={{ src }}` 交给 ui-kit 的 `AppShell`。
