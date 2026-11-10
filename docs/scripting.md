# JS 配置

行为契约见[阶段 D](emacs-fidelity.md#阶段-djs-配置与文本变换)及 [F2](emacs-fidelity.md#阶段-f2status-与单文件-index-操作)。此处说明配置入口和 API，不记录实现 / 验收进度。

## 配置入口

读取绝对 `$XDG_CONFIG_HOME/semacs/init.js`；变量未设置、为空或不是绝对路径时使用 `$HOME/.config/semacs/init.js`。`HOME` 也须为绝对路径；无法定位时报告配置失败，不回退到工作目录。不从被编辑项目自动加载配置，不自动创建文件。文件不存在时使用默认配置。

配置是 UTF-8 JavaScript 普通脚本，不是 Node.js 程序或 ES module。没有 `require` / `import`、浏览器 API 或模块搜索路径；当前可在单文件内定义普通函数。宿主读取入口文件不表示脚本获得文件系统权限。

示例见 [examples/init.js](../examples/init.js)。不要直接覆盖已有配置；合并需要的声明即可。执行 `M-x reload-configuration` 显式重载，不监视文件变化。

## 宿主 API

### `semacs.setFont({ family?, size? })`

声明启动和重载时的字体基准。可多次调用，后一次提供的字段覆盖前一次字段；整份配置中未声明的字段使用默认值。未指定 `family` 时使用随包 Lilex，只调整字号不必显式填写字体族。默认字体与自定义具体族名的边界见[显示设置契约](emacs-fidelity.md#字体与字号)。字体族按既有规则去除首尾空白、拒绝空白和控制字符，平台仍可能回退；字号为 8–72 逻辑像素，作为 `reset-font-size` 的基准。

### 主题与命名样式

`semacs.defineTheme(name, { appearance, styles })` 定义一个主题；`appearance` 为 `"light"` 或 `"dark"`，未指定的属性从该模式的内置主题补齐，不从上次主题继承。名称为 1–64 个小写 ASCII 字母、数字或连字符，不能重复或覆盖内置 `light` / `dark`。不加载 elisp 主题，不使用 Emacs font-lock / face 接口。

`semacs.loadTheme(name)` 声明初始主题，默认 `light`；多次调用最后一次生效，可在定义前引用，最终必须存在。`M-x load-theme` 提供当前定义的主题候选，切换只影响本次运行，不写回文件。

`semacs.setStyle(name, attributes)` 声明独立于当前主题的用户覆盖；同一样式可多次调用，按属性覆盖。切换主题保留这些显式覆盖；成功重载整体替换主题定义、选择和覆盖，删除声明即恢复默认，失败保持旧配置。所有主题 API 仅配置阶段可用。

颜色仅支持不透明 `#RRGGBB`（十六进制大小写均可）。文字属性包括 `weight: "normal" | "bold"`，以及布尔值 `italic`、`underline`、`strikethrough`。未指定的属性不覆盖下层样式；`"normal"` / `false` 显式取消相应强调。样式与可用属性：

| 样式名 | 属性 / 用途 |
|---|---|
| `default` | 前景 / 背景及全部文字属性，正文及输入基准 |
| `cursor` | `foreground` 为覆盖文字颜色，`background` 为方块填色 |
| `region` | 仅 `background`，保留正文文字颜色 |
| `mode-line` / `mode-line-inactive` | `foreground` / `background` |
| `minibuffer-prompt` | 前景 / 背景及全部文字属性 |
| `echo-area` | `foreground` / `background`；读入末尾临时注记只使用其前景色 |
| `completion` / `completion-selected` | `foreground` / `background` |
| `completion-match` | 仅 `foreground`，优先于候选行文字颜色 |
| `completion-annotation` | 仅 `foreground`，候选注释及计数 |
| `separator` | 仅 `foreground`，分屏分隔线 |
| `syntax.keyword` / `syntax.comment` / `syntax.string` / `syntax.number` | 前景 / 背景及全部文字属性，语法关键字 / 注释 / 字符串 / 数字 |
| `syntax.type` / `syntax.function` / `syntax.variable` / `syntax.constant` | 前景 / 背景及全部文字属性，语法类型 / 函数 / 变量 / 常量 |
| `syntax.operator` / `syntax.punctuation` | 前景 / 背景及全部文字属性，语法运算符 / 标点 |
| `markup.heading` / `markup.strong` / `markup.emphasis` | 前景 / 背景及全部文字属性，Markdown 标题 / 强强调 / 强调；内置分别为加粗 / 加粗 / 斜体 |
| `diff.file` / `diff.hunk` / `diff.added` / `diff.removed` / `diff.context` | 前景 / 背景及全部文字属性，diff 文件头、hunk 头、增删行及上下文；heading 默认加粗，显式背景铺满逻辑行 |
| `diff.file-highlight` / `diff.hunk-highlight` / `diff.added-highlight` / `diff.removed-highlight` / `diff.context-highlight` | 当前 section 内对应 diff 角色的样式；缺省沿用普通角色的有效属性，再使用 `section.current` 背景，文件 / hunk 头默认加粗；可独立覆盖，包括 `weight: "normal"` |
| `diffstat.added` / `diffstat.removed` | 前景 / 背景及全部文字属性，diffstat 的增删计数 |
| `section.heading` | 前景 / 背景及全部文字属性，status 分组名及 diff 总览标题；不把 status 数量、路径或文件头混用为分组标题 |
| `magit.branch` / `magit.hash` | 前景 / 背景及全部文字属性，status 中的本地分支名与 HEAD OID |
| `section.current` | 仅 `background`，当前 section 整行背景；该反馈独立于 region |

当前 section 按各 view 的 point 分别合成，不改写共享样式范围；颜色变化仅重绘，当前字体变化参与该 view 的布局身份。逻辑行背景按行首对应角色决定，背景铺至视图右缘，文字 / region / 光标仍保留既有合成优先级。高亮角色到普通角色的回退是上述固定对应关系，不提供任意继承。配色由 semacs 主题直接控制，不运行 Delta；保留 `+` / `-`，不提供逐词差异高亮。

未知样式、未支持的属性及非法颜色均拒绝整份配置。不支持逐段字体族 / 字号、任意继承、多个启用主题叠加或自动跟随系统。字体实际效果依赖平台字体及回退，不保证所请求字形一定存在。语法启用见 [E1 契约](emacs-fidelity.md#阶段-e1rust-tree-sitter)，字体与背景合成见 [E3 契约](emacs-fidelity.md#阶段-e3文本样式与高亮合成)。

```js
semacs.defineTheme("night", {
    appearance: "dark",
    styles: {
        default: { foreground: "#e6e6e6", background: "#202124" },
        "mode-line": { foreground: "#ffffff", background: "#304060" },
        "minibuffer-prompt": { foreground: "#a8c7fa" },
    },
});
semacs.loadTheme("night");
semacs.setStyle("cursor", { foreground: "#202124", background: "#ffcc66" });
semacs.setStyle("syntax.keyword", { weight: "bold" });
semacs.setStyle("syntax.comment", { italic: true });
```

### `semacs.registerCommand(name, transform)`

注册 `(text: string) => string` 文本变换命令。名称为 1–128 个小写 ASCII 字母、数字或连字符，不能与任何原生命令或另一条脚本命令重名。函数可以闭包引用本次配置的局部变量；重载后不保留旧运行时状态。

命令从 `M-x` 或自定义键位调用；读取活动 region，不获得 buffer、view、point 的可变引用，也不能直接调用原生命令或发起 I/O。必须返回字符串，不能返回 Promise。错误只显示诊断，不提交部分替换。

### `semacs.registerWorkflowCommand(name, { modes?, run })`

声明同步领域工作流，不改变文本变换接口。命名、重名检查、`M-x`、键位及 worker 与文本变换共用；不能覆盖原生命令或随包声明。`modes` 缺省适用全部正文 mode，提供时必须是非空、无重复的已知 mode 列表；局部键位、候选和直接名称执行一致检查适用性，通用键位不会绕过 mode 限制。

`run(context)` 收到调用开始时深度冻结的值快照，不含路径、Git 参数、可变核心对象或可伪造目标句柄：

- `mode`：`"fundamental" | "magit-diff" | "magit-status" | "git-commit" | "magit-process"`。
- `status`：非 status 为 `null`，否则为 `{ selection }`。
- `selection`：无可见文件 / hunk 目标时为 `null`，否则含 `group: "untracked" | "unstaged" | "staged" | "unmerged"` 及布尔值 `canStage` / `canUnstage`。能力只是提示；region、过时清单或不支持的目标不授予写入权，核心仍重新校验。

返回 `null` 表示无请求，或返回以下一个严格对象（未知字段拒绝）：

| 请求 | 行为 |
|---|---|
| `{ operation: "magit.status.open" }` | 沿调用目标的固定仓库或目录打开 status |
| `{ operation: "magit.status.refresh" }` | 仅刷新捕获的 status 会话；非 status 拒绝 |
| `{ operation: "magit.diff.open", comparison: "staged" | "unstaged" }` | 打开整个比较，不按 point 过滤 |
| `{ operation: "magit.stage" }` / `{ operation: "magit.unstage" }` | 仅操作调用时捕获的一个 status 文件或 hunk，见 [F4](emacs-fidelity.md#阶段-f4status-内嵌-diff-与单-hunk-index-操作) |

不接受数组、任意命令名 / payload / Git args、Promise、执行期 host-call 或完成回调。JS 异常、超时、非法返回或 pending jobs 均不启动核心操作。返回不等于 Git 完成；接受前重新验证原 view、输入会话、配置代次及目标，不在返回时重选 point。接受后的写入由核心持有，跨 reload / kill / 另存继续，终态和独立刷新结果记录在 `*Messages*`；不确定结果不自动重试。

随包 JS 声明 `magit-stage` / `magit-unstage`，status 默认 `s` / `u`；空配置、配置失败回退也构建真实 handler。可重绑或用局部 `null` 禁用，不能重名覆盖。JS 故障时写入不可用，不改走原生写入；原生 status / diff / 刷新仍可用。

**文件级 stage 使用执行时磁盘内容；hunk stage / unstage 应用捕获的 patch，不读取未保存 buffer。写入可能丢失仅存于 index 的中间版本，没有自动 undo / 回滚。** 支持矩阵与退出规则见 [F2](emacs-fidelity.md#阶段-f2status-与单文件-index-操作) / [F4](emacs-fidelity.md#阶段-f4status-内嵌-diff-与单-hunk-index-操作) 契约。

```js
semacs.registerWorkflowCommand("status-or-refresh", {
    run: context => context.status
        ? { operation: "magit.status.refresh" }
        : { operation: "magit.status.open" },
});
semacs.bindKey("ctrl-c g", "status-or-refresh");
```

### `semacs.bindKey(keys, commandName | null, options?)`

绑定或取消一个正文键序列；可多次绑定不同键到同一命令。不传 `options` 保持正文通用绑定；传 `{ mode: "magit-diff" }`、`{ mode: "magit-status" }`、`{ mode: "git-commit" }`、`{ mode: "magit-process" }` 或 `{ mode: "fundamental" }` 仅配置对应 mode 的局部层。未知 mode / 选项拒绝整份配置，当前没有 JS mode 注册 API。局部层优先于通用层；局部 `null` 屏蔽该 mode 内的低层回落，删除声明恢复默认。跨层前缀与上下文规则见 [F1 契约](emacs-fidelity.md#mode-与键位)。

```js
semacs.bindKey("tab", "section-toggle", { mode: "magit-diff" });
semacs.bindKey("alt-right", null, { mode: "magit-diff" });
```

命令名可以是本份配置注册的命令或可通过 `M-x` 使用的原生命令，不要求先注册后引用。

键格式采用显式修饰符和键名，空白分隔连续按键：

| 配置写法 | 显示 |
|---|---|
| `ctrl-f` | `C-f` |
| `alt-x` 或 `escape x` | `M-x` / `ESC x`（同一绑定） |
| `ctrl-c u` | `C-c u` |
| `ctrl-c U` | `C-c S-u` |
| `enter`、`tab`、`backspace` | `RET`、`<tab>`、`DEL` |
| `left`、`right`、`up`、`down` | 方向键，可加 `ctrl-` 等修饰符 |
| `pageup`、`pagedown`、`home`、`end` | 翻页键、行首尾键；行为由绑定的命令决定 |

组合修饰符如 `ctrl-alt-_`；也支持 `shift-`、`super-` 和 `fn-`。不支持平台相关的 `secondary-` 别名，不把文字标签 `C-f` 当作配置语法。系统保留键仍由平台处理。

`null` 取消该序列的绑定，不会删除命令。相同自定义序列不能重复声明；例如不能同时声明 `alt-x` 和 `escape x`。取消前缀不能隐式删除仍存在的子绑定；把旧叶子命令改成前缀时，必须明确取消旧叶子绑定：

```js
semacs.bindKey("ctrl-f", null);
semacs.bindKey("ctrl-f a", "forward-char");
```

这会改变常用 `C-f`，仅用于展示规则，不是推荐默认键位。

所有宿主配置 API 仅在加载 `init.js` 时可调用，命令执行期间调用会报错。

## 类型提示与 TypeScript

[types/semacs.d.ts](../types/semacs.d.ts) 描述全局宿主 API，可在编辑器的 JS / TS 项目中引用。仓库示例包含相对引用；复制到配置目录后，应调整声明文件路径。

rquickjs 只执行 JavaScript。不内置 TypeScript 转译器或类型检查器；可以自行将 TS 编译成单一普通脚本 `init.js`，不要输出依赖 Node / 模块加载器的代码。类型提示不是运行时校验的替代品。

## 资源边界

当前实现限制（不是新增性能承诺）：

| 项目 | 限额 |
|---|---|
| `init.js` 源码 | UTF-8，最多 1 MiB |
| 配置执行 / 校验 | 500 ms 执行期限 |
| 单次脚本命令 | 100 ms 执行期限 |
| QuickJS 内存 / 栈 | 每个运行时 32 MiB / 256 KiB，不等于整个进程 RSS 上限 |
| region 输入 / 返回文本 | 各最多 256 KiB UTF-8 字节 |
| 脚本命令 / 自定义键位 | 文本变换、随包及用户 workflow 合计最多 256 条；用户自定义键位最多 1024 条 |
| 自定义主题 / 样式覆盖调用 | 每份配置最多 32 个 / 128 次；每个主题最多 30 个命名样式 |
| 正文请求 | 每个窗口最多一个未完成的脚本命令及一个配置重载；重复请求立即报告忙，不累计 |
| worker 请求队列 | 容量 1，发送不等待空位 |

执行期限由 QuickJS 中断和返回时检查实现，不保证任意原生内建操作都能在精确时刻被抢占；文件读取也不是受该 JS 期限约束的磁盘 I/O 承诺。不要运行不可信脚本。配置超时回退默认值，重载失败保留旧配置；命令超时 / 内存或栈异常不会提交正文修改。

Promise / async 返回值被拒绝，排队的 JS jobs 不会执行；出现待处理 jobs 后，该运行时拒绝后续脚本命令，需要显式重载。原生编辑仍不依赖脚本运行时。
