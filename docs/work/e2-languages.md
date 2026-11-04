# E2 多语言语法支持：讨论与调研工作稿

本文保存 E2 的设计依据和源码阅读结论，不是实现完成记录或已生效的行为契约。当前范围、验收目标与待决项统一见 [实施计划](../plan.md#当前阶段-e2多语言语法支持)，本文不另维护待办。

## 已确认方向与选择理由

semacs 主要面向小范围使用，增加语言后重新编译可以接受。当前选择编译期语言模块，不建设可安装语言包或通用插件系统；先完成多语言语法支持，再进入 diff → magit。

需要解耦的是“语言差异”和“通用机制”，而不是立即把 grammar 移出可执行文件：

- 语言定义提供身份、文件关联、grammar 和查询。
- Rust 核心统一拥有后台任务、增量解析、查询执行、取消、buffer 共享、版本隔离和绘制高亮。
- 普通语言接入应主要是添加依赖、声明语言、适配查询和补测试，不逐语言修改 buffer / view / 绘制路径。
- 未来若确需动态语言资源，加载器可将资源整理成同样可用的语言定义；仍需届时设计加载生命周期等机制，不承诺零修改兼容。

这与 [D26](../decisions.md#d26-rust-核心与脚本扩展) 一致：脚本可以组合宿主能力，但不承担逐次解析或绘制热路径。没有必要仅为语言注册引入 JS；静态声明也可以做到少量接入代码。

## 讨论过的分发方案

| 方案 | 增加语言需重编宿主 | 主要权衡 |
|---|---|---|
| 原生内置 grammar | 是 | 当前最简单；Cargo 构建 C parser / scanner 并链接进程序 |
| 原生动态 grammar | 否 | 需各平台产物、受控库路径、ABI / 依赖校验和库生命周期管理；加载的是进程内原生代码 |
| WASM grammar | 否 | 有利于跨平台资源分发和内存隔离，但引入运行时、模块加载成本和兼容性验证 |

Windows 原生动态加载本身可行，复杂点主要是 MSVC / MinGW 构建、架构、导出符号、运行库依赖，以及已加载 DLL 的更新。可以用预编译产物降低用户安装成本，但会增加发布矩阵。Linux / macOS 也存在架构、系统依赖及分发限制，并非无条件通用。

当前 tree-sitter 0.27.1 的 Rust binding 提供可选 `wasm` feature、`WasmStore::load_language` 和 `Parser::set_wasm_store`，使用 Wasmtime。此路径中的引擎仍在原生宿主侧，grammar 为 WASM 模块，不经过 QuickJS。此次未启用或测量该能力，也未验证各 grammar 的 WASM 兼容性。

若未来采用“语言包”，更合理的是元数据、grammar 产物、查询和可选脚本组成资源集合，而不是把所有资源塞进原生动态库；不应每种语言各带一套 Tree-sitter 引擎和调度机制。本阶段不定义这种包格式。

## 外部实现参照

### Emacs

这里指 Emacs 29 起内置的 `treesit`，不是第三方 `tree-sitter.el`。

- grammar 以原生动态库分发，按目录查找并加载约定导出函数，检查 Tree-sitter grammar ABI。
- 搜索路径包括 `treesit-extra-load-path`、用户配置下的 `tree-sitter/` 和系统库位置。
- `treesit-install-language-grammar` 可按来源配置下载、构建和安装，也可使用预编译 grammar。
- 解析与语言编辑行为分离；Elisp major mode 定义高亮查询、缩进和导航等规则，不是安装 grammar 后自动获得所有功能。

参考：[grammar 加载](https://www.gnu.org/software/emacs/manual/html_node/elisp/Language-Grammar.html)、[基于 parser 的 Font Lock](https://www.gnu.org/software/emacs/manual/html_node/elisp/Parser_002dbased-Font-Lock.html)。

### Zed

本次阅读本地 `~/ghq/github.com/zed-industries/zed`，commit `ba63a022c9`：

- `crates/grammars/src/grammars.rs` 注册原生内置 grammar，包括 `tree_sitter_rust::LANGUAGE`。
- 扩展通过仓库与固定 revision 声明 grammar；`crates/extension/src/extension_builder.rs` 将 parser 和 scanner 编译为 grammar WASM。
- `crates/language/src/language_registry.rs` 按需读取并通过 `WasmStore` 加载、缓存 grammar。
- 语言元数据和各类 `.scm` 查询描述高亮、缩进、outline、文本对象等行为；查询的执行及编辑效果由宿主实现。
- `grammars/*.wasm` 与扩展可执行逻辑 `extension.wasm` 是不同产物，不能混为一谈。

因此 Zed 是“内置原生 + 扩展 WASM”的混合模式，不是所有 grammar 都以 WASM 执行。参考：[语言扩展文档](https://zed.dev/docs/extensions/languages)。

### 对未来 LSP 的意义

LSP 通常通过 JSON-RPC 连接独立语言服务器进程，与 grammar 使用内置原生、动态原生还是 WASM 基本正交，不应据此提前选择加载格式。

按 D26，未来可由 Rust 统一提供进程生命周期、workspace / buffer 归属、文档同步、位置编码转换、请求取消、版本隔离和编辑提交；配置 / 脚本声明服务器命令、参数及必要策略。这里仅说明边界，不代表已设计或排期 LSP。

## 上游源码调查

本次只阅读用户提供的本地仓库；下面的 commit 是阅读时的源码身份，不是已选定的依赖锁定版本。未执行构建、查询编译或性能测试。

| 项目 | 本地仓库 | commit |
|---|---|---|
| Markdown | `~/ghq/github.com/tree-sitter-grammars/tree-sitter-markdown` | `41ff24a` |
| JavaScript | `~/ghq/github.com/tree-sitter/tree-sitter-javascript` | `58404d8` |
| TypeScript | `~/ghq/github.com/tree-sitter/tree-sitter-typescript` | `75b3874` |

### JavaScript / JSX

依据 `README.md`、`bindings/rust/lib.rs`、`tree-sitter.json` 和 `queries/`：

- 一个 `LANGUAGE` 同时覆盖 JavaScript 与 JSX；元数据列出 `js`、`mjs`、`cjs`、`jsx`。
- JSX 标签、属性和内部 JS 表达式在同一棵语法树中，不需要另接 HTML parser 或跨语言注入。
- 上游提供通用、JSX 和参数高亮查询；Rust binding 暴露 `HIGHLIGHT_QUERY`、`JSX_HIGHLIGHT_QUERY` 等常量，具体组合需按选定版本验证。
- JSX 使用 `@tag`、`@attribute` 等 capture，当前 semacs 的映射不会识别所有这些名称，需要适配。
- 上游 injections 还描述 tagged template、regex、JSDoc 等。这些与 JSX 基础语法不同，不因查询存在而自动成为宿主功能。

### TypeScript / TSX

依据 `README.md`、`bindings/rust/lib.rs` 和 `tree-sitter.json`：

- 提供两个 grammar：`LANGUAGE_TYPESCRIPT`、`LANGUAGE_TSX`。
- TSX 的 TypeScript 与 JSX 结构由 TSX grammar 直接覆盖，不需额外嵌入 HTML。
- 元数据将 TypeScript 查询与 JavaScript 通用查询组合，TSX 还组合 JSX 查询。仅使用 TypeScript crate 的 `HIGHLIGHTS_QUERY` 不等于完整高亮。
- 上游声明的查询顺序是验证输入，不应直接假定符合 semacs 当前“后匹配覆盖”等优先级规则。

### Markdown

依据 `README.md`、`Cargo.toml`、`bindings/rust/lib.rs`、`bindings/rust/parser.rs` 和两套 `queries/`：

- Rust crate 名为 `tree-sitter-md`；阅读版本为 0.5.4，提供块级 `LANGUAGE` 和行内 `INLINE_LANGUAGE`。
- 默认包含 GFM 的任务列表、删除线、表格等扩展，另有 YAML / TOML 元数据规则；具体构建选项仍须固定。该项目定位包含语法高亮，不保证完全准确的 Markdown 解释。
- 可选 `parser` feature 提供 `MarkdownParser` / `MarkdownTree`，协调块级树和行内树，使用 `included_ranges`，支持增量编辑及解析回调。此版本的可选 tree-sitter 依赖为 0.27，版本约束层面与 semacs 当前系列一致，尚未实际构建验证。
- 此封装不是任意嵌入语言执行器，不会自动解析代码围栏里的 JavaScript / Rust。
- 块级 injections 查询标出围栏中的 `@injection.language` 和 `@injection.content`，并声明 HTML、YAML、TOML、Markdown 行内内容等注入；行内查询另有 HTML 和 LaTeX。
- 宿主仍需执行查询、解析语言别名、选择已注册 grammar、计算子语言范围、协调结果生命周期并合并高亮。未知语言不应阻止宿主 Markdown 编辑或隐藏正文。
- 高亮查询使用 `@text.title`、`@text.emphasis`、`@text.strong`、`@text.uri`、`@none` 等。现有 capture 映射不覆盖这些约定；颜色适配与覆盖语义需要明确，不能仅导入查询就宣称完成。

README 还保留 WASM / web-tree-sitter 不能开箱即用的兼容性提示。此次没有复现或核定其对当前 Wasmtime 路径的适用性，不能把它外推为所有 WASM 接入均不可行。

### HTML / CSS 调查边界

本次用户提供并实际阅读的 grammar 仓库仅为上述三个项目。HTML / CSS 是计划目标，但其依赖版本、查询和 HTML script / style 注入尚未在本次调查中核实；后续以计划中的实现前细化项为准。

## 对当前代码结构的建议（非最终设计）

`src/syntax.rs` 当前按 `.rs` 判断是否启用，并直接绑定 Rust grammar / query。模块化应解决真实状态归属，而不是只移动常量：

- 以语言身份代替单纯 enabled 布尔值，正确处理不同语言之间的路径切换。
- 通用 worker 消费已准备好的语言定义，不直接引用具体 grammar crate。
- 按语言复用编译查询，保留必要的本地查询补充。
- Markdown 需要表达多棵语法树及其范围，不能硬套单树结果结构；采用上游 wrapper 还是统一语法分层，由实际增量更新和取消要求决定。
- 子语言高亮也要绑定同一 committed 快照与语言代次，定义覆盖规则和资源边界；上游提供注入规则不替代宿主的正确性责任。
- 保持少量职责模块，不预建动态后端 trait、稳定插件 ABI 或每语言一套框架。

## 文档收尾方式

本稿作为当期跨会话设计材料，由实施计划引用。实现期间只保留必要设计依据；确定的外部行为进入保真度契约，长期决策按需提炼到技术决策，来源许可随实际引入更新。阶段结束按 [文档维护规则](../README.md#临时工作材料) 提炼或删除工作稿，不把整份讨论搬入验收记录。
