# 绘境 · 使用 ui-kit v1.2.0 的摩擦点清单

项目：`/Users/aowu/Documents/同步盘/项目开发/绘境`（ui-kit 原样拷在 `src/ui-kit/`，未改动）。

## 一、缺失 / 不够用，只好在 kit 外自写的组件

1. **任务行 / 富列表行**（`src/components/TaskRow.tsx`）
   `FileRow`（components/app/file-list.tsx）固定 `h-8` 单行文字，放不下缩略图 + 两三行信息 + 一排图标操作。只复用了 `FILE_ROW_ERROR_CLASS`。
   - 失败样式只考虑纯文字行：行内 antd `Typography.Text strong` 有自己的颜色，要手动加 `text-inherit`；行内 Tag 仍保留各自颜色。
   - 建议：`ListRow`（`leading` 缩略图槽、`title`、`meta`、`trailing` 操作区、`status`、多行高度）。
2. **本地图缩略图**（`src/components/Thumb.tsx`）：`convertFileSrc` + antd `Image` + 加载 / 失败占位 + 固定方框裁切 + 受控预览。AI_GUIDE 规则 14 只说用 `convertFileSrc`，没有组件。
3. **图片网格 / 可选中瓦片**（`src/pages/GalleryPage.tsx`）：选中态、双击预览、配合 `Image.PreviewGroup` 的 `items` 受控翻页。只能用原生 `<button>` + `<img>`——与规则 1「不要自己写按钮」冲突，但 kit 自己的 `ThemePicker`、`DropZone` 也是原生 button，规则没说明这种「可选中卡片」的例外。
4. **参考图缩略图墙**（`src/components/RefImages.tsx`）：带移除角标的小图网格，DropZone 之外最常见的配套件。
5. **设置页骨架**（`src/pages/SettingsPage.tsx`）：没有表单分区（`SettingsSection`）、长表单滚动容器、「草稿 / 已保存 / 放弃修改」模式的指引。`Panel fill` 只把 body 变成 flex，表单滚动要自己包 `min-h-0 flex-1 overflow-y-auto`；`ListWell` 带描边和底色，是列表样式，不适合表单。design.md 也没写设置页怎么排。
6. **导航徽标**：`NavItem.badge` 只给了槽位，没有默认样式。antd `Badge` 默认是红色（危险色语义，不该用来表示「进行中」）；侧栏为主题色时用主色徽标又会看不见。最后用 `classNames.indicator="bg-sidebar-active-text text-sidebar"`（照搬品牌标记的反色块规则）。建议 kit 提供 `NavBadge`。
7. **多任务队列的进度**：`ProgressPanel` / `useBatchProgress` 只针对「一个批次、一个总进度」，并发任务各自有进度、排队、取消、重试的模式完全没有对应件，Rust 端约定（AI_GUIDE「批处理与 Rust 端约定」）也只有批处理。
8. **大号空状态**：模板只有 `Empty.PRESENTED_IMAGE_SIMPLE`，没有「图标 + 引导文案 + 操作按钮」的空状态 / 缺配置引导（例如缺 API Key 时引导去设置）。我先用了 `text-text-quaternary` 画图标，后来发现 design.md 规定 quaternary 只用于禁用态，改成 tertiary——kit 给个现成件能避免这种误用。

## 二、文档与实际不一致

1. `AI_GUIDE.md`「第 2 步」写「当前版本 1.1.0」，而 `package.json` / `index.ts` 的 `UI_KIT_VERSION` 是 1.2.0。
2. `AI_GUIDE.md` 第 4 步要求开 `assetProtocol`，但没提 `src-tauri/Cargo.toml` 的 tauri 依赖必须加 `features = ["protocol-asset"]`（不加编译报错）；`tauri/tauri.conf.snippet.json` 也没注明。
3. 第 3 步只装 dialog 插件；规则 14 / 常见需求（打开链接、在访达中显示）还要 opener 插件与 `opener:default` 权限，没写。
4. design.md 的 Layout 写「每个功能页 = 左列表 + 右设置栏 + 底部日志」，但 Workspace 的右栏是整体 `overflow-y-auto`、模板把主按钮放在 `mt-auto`：参数一多（本应用模式 + 模型 + 提示词 + 参考图 + 4 组参数 + 张数），**主按钮会被挤出可视区**，要滚到底才看得到。我改成「参数 Panel fill + 内部滚动、主按钮 shrink-0 常驻」。建议 `Workspace` 提供 `asideFooter` 槽，或模板直接按这个写。

## 三、规则之间的冲突

1. **规则 9 vs 自检第 5 条**：规则 9 要求 `const { message } = App.useApp()`，调用时就是 `message.success(...)`；而「完成后自检」第 5 条全局搜索 `message.success(` 视为违规。合规代码也会被命中。只好把解构改名 `const { message: toast } = App.useApp()`。建议自检改搜 `import { message }` / `antd.message.` 这类静态用法。
2. **规则 1（控件一律 antd）vs 可选中卡片**：见一·3。
3. **规则 5（每区一个主按钮、写明数量）**在图库、设置页没有「数量」可写，只能写「继续修改 · 这张」「保存设置」；规则没说明非批处理页怎么办。
4. **规则 11（每页各自 useLogger）+ 页面切换**：AppShell 只渲染 children，若按路由切换卸载页面，日志、草稿与事件订阅都会丢；我让三页常驻、用 `hidden` 切换。kit 没说明推荐哪种。

## 四、想改 kit 但按约束没改

1. `ThemeProvider`（theme.tsx）的明暗模式与主题只存 `localStorage`，只有 `accent` 能受控，`mode` 不能受控，也没有 `onChange` / `storage` 适配。Windows 便携版换机器后 WebView 数据不跟着走，只好在 App 里写 `useThemeSync` 把两者同步进自己的 settings.json。想给 ThemeProvider 加 `mode` 受控 + `onAccentChange/onModeChange`。
2. 想给 `FileRow` 加 `leading` 槽与 `size="lg"`（见一·1）。
3. 想给 `LogViewer` 加内置「清空」与「复制全部」（现在每页在 Panel extra 自己写清空按钮）。
4. 想给 `useLogger` 一个可在 React 外写入的通道：Rust 端在 setup 里恢复队列时发的日志，页面还没开始监听就丢了，只能前端载入后自己补一条摘要。

## 五、用起来顺手的地方（供对照）

- `AppShell` / `Page` / `Workspace` / `Panel` / `LogViewer` 骨架直接可用，深浅色、毛玻璃、森翠主题下对比度都没问题；`antd lint` 零告警。
- `useFileDrop` 窗口级拖拽 + `DropZone compact` 很顺；`cn` 已登记 `text-h2` 等自定义刻度，不会被 tailwind-merge 误合并。

## 六、第二轮（多合一、比例预览、名称与图标）新增

1. **比例选择没有现成件**：要在 antd `Select` 的选项里画按真实宽高比的小矩形、在参数区画比例预览框，只能自写 `RatioIcon` / `GridPreview`（`src/components/Ratio.tsx`）。kit 没有「比例 / 尺寸选择器」，也没有任何「示意图」类组件的配色约定。
2. **动态尺寸与「禁止 `style={{`」冲突**：预览框的宽高比是运行时算出来的，自检第 5 条禁止 `style={{`，只能改用 SVG 的 `viewBox` 让浏览器按比例缩放来绕开。kit 自己的 `Workspace`（gridTemplateColumns）、`ThemePicker`（色块背景）都用了内联 style，规则没有给「数据驱动的尺寸」留例外。
3. **SVG 里怎么用 token 颜色没写**：Tailwind 的 `fill-primary-bg` / `stroke-primary` 可用（preset 的 colors 会生成 fill / stroke 类），但 design.md 和 AI_GUIDE 都没提，只能自己试出来。
4. **图片宫格 / 缩略图条**：队列行里要放「合成图 + N 张拆切图」的小缩略图条，图库要按「每张图」而不是「每个任务」展示——还是没有 `ImageGrid` / `ThumbStrip`，`FileRow` 更放不下（见一·1、一·3）。
5. **行内操作的二级菜单**：多合一时「继续修改」要选具体哪一张，只能在行尾图标按钮上套 antd `Dropdown`；kit 对行尾操作的数量、溢出（「更多」菜单）没有约定，`FileRow.trailing` 只是个槽。
6. **说明文字的位置**：「合图使用更高一档分辨率（可能影响单价）」「比例为自动时不能多合一」这类依赖其他参数的提示，只能塞进 `Form.Item extra`；kit 的右栏没有「参数提示 / 警示」的样式约定（`Alert` 在 300px 宽的右栏里太重）。
7. **品牌与图标流程**：kit 只管界面，`brand.mark` 之外没提应用图标；这次发现 `qlmanage` 渲染 SVG 会垫白底导致图标四角不透明，改用 AppKit 渲染并写了逐尺寸读 alpha 的校验脚本（`scripts/icon/`）。若 kit 定位是「Tauri 桌面工具套件」，可以附带图标生成 / 校验脚本与 macOS 留边规范。
