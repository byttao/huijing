# 绘境 · 使用 ui-kit v1.2.0 的摩擦点清单

项目：绘境（ui-kit 原样拷在 `src/ui-kit/`，未改动）。

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

## 七、第三轮（序号、跳转、品牌图标）新增

1. **`AppShell` 的 `brand.mark` 只适合单色字形**：mark 被放进 40px 的反色块（`bg-sidebar-active-text text-sidebar`），想直接显示彩色应用图标时色块会从透明圆角处露出来。没改 kit，只能把图标画成 42px 略大于色块、把色块盖住（`src/components/BrandMark.tsx`）。建议 `brand` 增加 `icon`（原样显示图片、不套色块）或 `markVariant="plain"`。
2. **列表行的定位 / 高亮没有约定**：「改自 #7」要滚动到原记录并闪一下，kit 没有「定位高亮」的样式或时长约定，只能用 `bg-primary-bg` + `duration-slow` 自写；深色下 `primary-bg` 很暗，高亮不太显眼。
3. **可点击的 Tag**：规则 1 不让自写按钮，但「改自 #7」这种既是状态又能点的标签，antd `Tag` 本身不是按钮，只能补 `role="button"`、`tabIndex`、键盘事件；kit 没给「可点击标签」的写法。
4. **图片上的角标**：图库瓦片左上角的编号要在任意图片上都看得清，`bg-spotlight` 在深色主题下是浅底（配白字对比不够），最后改用 `bg-container` + 描边；kit 没有「图片上的叠加文字」用哪个 token 的说明。
5. **没有全局错误边界**：渲染出错时整页空白，kit 的 `ThemeProvider` / `AppShell` 不带错误兜底，这轮在入口自加了 `ErrorBoundary`（antd `Result`）。

## 八、升级到 1.3.0 时新发现

升级本身顺利：rsync 覆盖后旧代码不改也能编译（没有删除 / 改名导出），深色主区按主题着色已修好（森翠深绿、马卡龙暗紫褐），`antd lint` 与新版自检全过。替换组件时遇到的问题：

1. **`ThumbStrip` 没有「图下说明」槽**：多合一切片要常显编号 #12-1，`ThumbStrip` 只有悬停 `showName`，所以队列行的切片条仍自写（`TaskRow.tsx` 的 `PartStrip`，内部用 kit `Thumb`）。建议 `ThumbItem` 加 `caption`。
2. **`ImageGrid` 不会把选中项滚到可见**：外部改了 `selectedKey`（如「改自 #7」跳到图库定位）后，瓦片不在可视区就看不到；瓦片也没有 id / ref。只好在外层 `ListWell` 上按 `[aria-pressed="true"]` 查找再 `scrollIntoView`。建议选中变化时自动 `scrollIntoView({ block: "nearest" })`，或提供 `scrollToSelected`。
3. **`ListRow` 的 `highlight` 是常驻状态，没有「闪一下」的约定**：页面得自己管定时器（绘境 2.5 秒后取消）。建议给个 `highlight="flash"` 或在文档里写明推荐时长。高亮本身（主色 2px 内描边）在深色下足够显眼，比 1.2 时自写的 `bg-primary-bg` 好。
4. **主题受控要把设置提到 `ThemeProvider` 之上**：读设置、写回设置都得在 `ThemeProvider` 外面做，而那里拿不到 `App.useApp()`（`message` 在 `ThemeProvider` 里面），读设置失败只能把错误传进 `App` 再提示。建议文档给出「读设置 → 受控主题」的完整入口写法（含错误提示）。
5. **`useTaskEvents` 在非 Tauri 环境返回空列表**：这对 demo 友好，但用 mock 后端做页面测试时必须伪造 `window.isTauri = true`，否则列表永远是空的。建议加 `enabled` 选项或允许传入自定义 `listen`。
6. **`Thumb` 的 antd `Image` 带 `role="button"`**：`ListRow` 里同时有可点击的 Tag（`role="button"`）时，按 role 选元素会先选到缩略图；不算 bug，但「可点击标签」仍没有 kit 写法（见七·3）。
7. **深色下 `Descriptions` 里带 `copyable` 的长路径偶尔不省略**：设置页「数据目录」在一次离屏截图里没有显示省略号，而是在单词中间折行（Applicat / ion）；浅色正常。只复现过一次，未确认是否与 kit 有关，先记下。
8. **`RatioPreview` 的 `cellRatio` 语义随绘境改了**：1.3 的文档按「合成比例 + 每格裁切」写（第二轮旧逻辑），绘境第三轮起整张比例不变、只等分，所以不传 `cellRatio`，每张比例写在 `caption` 里。若 kit 想覆盖「等分拆切」这类用法，可以加一个「每格比例」标注而不是裁切框。

## 九、第五轮（真实窗口验收后的修复）新增

1. **`LogViewer` 的浮动工具条会盖住长日志行的末尾**：传 `onClear` / `copyable` 时，「复制全部」「清空」浮在日志区右上角，长行的最后几个字被挡住；又没有把按钮放到 `Panel extra` 的选项。绘境改为不传 `onClear`，自己在 Panel 的 `extra` 放两个按钮（`src/components/LogActions.tsx`，复制用 kit 导出的 `logsToText`）。建议 `LogViewer` 提供 `toolbar="extra"`，或单独导出一个 `LogToolbar` 给 `Panel extra` 用；至少给日志区留出右侧内边距。
2. **`ImageGrid` 没有 `fit` 参数**：瓦片里的图固定 `object-cover`，1376×383 这类长条切片被裁成方块，看不出形状。绘境从外层用 `className="[&_img]:object-contain"` 改成完整显示（瓦片底色作留白）。建议加 `fit="cover" | "contain"`（`Thumb` 已经有）。
3. **`SettingsScroll` 没有「下面还有内容」的提示，也拿不到滚动容器**：最小窗口下参数卡片只露出模式、模型、提示词，看不出能滚。绘境用 IntersectionObserver 观察预览块，不可见时在 Panel 标题栏放「比例与张数 ↓」入口，选张数 / 排法后 `scrollIntoView`。建议 `SettingsScroll` 支持 `ref`，并提供可选的底部渐隐 / 「更多」提示。
4. **右栏参数的「高级」折叠没有约定**：低频参数收进 antd `Collapse ghost`，需要 `-mx-3` 抵消左右内边距才能与其他表单项对齐；kit 可以给一个 `SettingsSection` 的可折叠变体。

## 十、升级到 1.4.0 时新发现

升级顺利：从 `v1.4.0` 标签 `git archive` 取内容覆盖，旧代码不改也能编译。已换用 `ErrorBoundary`（入口一层 + 三页各包 `size="page"`）、`ThumbStrip caption`、`LinkTag`、`useRowFlash(2500)`、`ImageBadge` + `badgePlacement`、`RatioSelect`、`createMockTaskBackend`（浏览器模式的任务数据源，`src/lib/taskSource.ts`），删掉了自写的 `ErrorBoundary.tsx`、`Ratio.tsx`、`PartStrip`、图库手动滚动和 `[aria-pressed]` 查找。真实窗口里 `LinkTag` 的跳转 / 置灰、`useRowFlash` 的 2.5 秒高亮、`ImageGrid` 选中定位都正常；八·7（深色下 `Descriptions` 长路径不省略）这次没有复现。

1. **`ThumbStrip` / `ImageGrid` 仍然没有 `fit`**：两者内部的图都是 `object-cover`，多合一的长条切片（如 1376×383）会被裁成方块。绘境继续从外层用 `className="[&_img]:object-contain"` 覆盖（`TaskRow.tsx` 的切片条、`GalleryPage.tsx` 的网格）。`Thumb` 已有 `fit`，建议 `ThumbStrip` 与 `ImageGrid` 透传同名参数。
2. **`LogViewer` 浮动工具条遮挡长日志行（九·1）在 1.4.0 里仍未处理**：绘境继续用自写的 `LogActions` 放在 `Panel extra`。
3. **`createMockTaskBackend` 只覆盖任务列表**：页面里其他 `invoke`（读设置、宫格规划等）在浏览器里仍要自己伪造 `__TAURI_INTERNALS__`。若配方 / demo 想支持「整页在浏览器里跑」，可以再给一个通用的 `createMockInvoke(handlers)`。
4. **设置页配方的输入校验落到项目自己写**：这次实测发现「自定义代理 + 空地址」能被保存，保存后图片下载全部失败。绘境已补 `validateStatus` / `help` 并禁用保存（照配方写法），Rust 端加载设置时也会把无效的自定义代理回退为「不使用」。配方里若能放一个「依赖开关的必填字段」示例，更容易被照抄。

## 十一、升级到 1.5.0 时新发现

升级顺利：从 `v1.5.0` 标签 `git archive` 取内容覆盖，旧代码不改也能编译。已换用 kit `LogActions`（删除自写 `LogActions.tsx`）、`ImageGrid` / `ThumbStrip` 的 `fit="contain"`（删除外层 `[&_img]:object-contain`）、`SettingsScroll` 的 ref + 自带「下面还有」（删除 IntersectionObserver，标题栏「比例与张数 ↓」改为用 ref 把比例项滚到顶部）、`AdvancedSection`（带「已改 N 项」与「恢复默认」，删除 `Collapse` + `-mx-3`）。现在页面里没有为 kit 缺口保留的自写组件了。

1. **「下面还有」按钮会盖住它下面那一行内容**：按钮浮在滚动区底部正中，没滚到底时会挡住底边那一行的几个字（出图参数里遮住预览说明「按 2 列 × 1 行等分拆切……」的中段、「高级」里的升档提示）。设置页里它正好落在「代理」分段控件右边，看上去像控件的一个选项。建议滚动区底部留出与按钮等高的内边距，或把按钮放到卡片底边线上 / 右下角。
2. **`AdvancedSection` 的标题在 300px 右栏里容易换行**：标题 +「已改 N 项」+ 右侧「恢复默认」三者同一行，「高级 · 合图分辨率」这种带说明的标题会被挤成两行。绘境改为只写「高级」。建议文档提示右栏里标题保持两三个字，或给 `description` 放到第二行。
3. **跳转入口要靠 `querySelector("#id")?.closest(".ant-form-item")`**：依赖 antd 内部类名；`SettingsScroll` 若能提供 `scrollToField(id)` 之类的方法会更稳。
