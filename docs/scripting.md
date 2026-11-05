# JS 配置

行为契约见[阶段 D](emacs-fidelity.md#阶段-djs-配置与文本变换)。此处说明配置入口和 API，不记录实现 / 验收进度。

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
| `diff.file` / `diff.hunk` / `diff.added` / `diff.removed` | 前景 / 背景及全部文字属性，diff heading 与增删行；heading 默认加粗 |
| `section.current` | 仅 `background`，当前 section 整行背景；该反馈独立于 region |

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

### `semacs.bindKey(keys, commandName | null, options?)`

绑定或取消一个正文键序列；可多次绑定不同键到同一命令。不传 `options` 保持正文通用绑定；传 `{ mode: "magit-diff" }` 或 `{ mode: "fundamental" }` 仅配置对应 mode 的局部层。未知 mode / 选项拒绝整份配置，当前没有 JS mode 注册 API。局部层优先于通用层；局部 `null` 屏蔽该 mode 内的低层回落，删除声明恢复默认。跨层前缀与上下文规则见 [F1 契约](emacs-fidelity.md#mode-与键位)。

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
| 脚本命令 / 自定义键位 | 每份配置最多 256 / 1024 条 |
| 自定义主题 / 样式覆盖调用 | 每份配置最多 32 个 / 128 次；每个主题最多 30 个命名样式 |
| 正文请求 | 每个窗口最多一个未完成的脚本命令及一个配置重载；重复请求立即报告忙，不累计 |
| worker 请求队列 | 容量 1，发送不等待空位 |

执行期限由 QuickJS 中断和返回时检查实现，不保证任意原生内建操作都能在精确时刻被抢占；文件读取也不是受该 JS 期限约束的磁盘 I/O 承诺。不要运行不可信脚本。配置超时回退默认值，重载失败保留旧配置；命令超时 / 内存或栈异常不会提交正文修改。

Promise / async 返回值被拒绝，排队的 JS jobs 不会执行；出现待处理 jobs 后，该运行时拒绝后续脚本命令，需要显式重载。原生编辑仍不依赖脚本运行时。
