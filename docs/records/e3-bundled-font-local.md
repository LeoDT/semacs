# E3 随包 Lilex 默认字体：本地修复验证

## 身份、批准范围与来源

接续 [首批宿主字体失败](e3-host-manual-01.md)、[离线 fallback 实验](e3-font-fallback-local.md) 与 [只读系统字体解析](e3-font-resolution-host-01.md)。HEAD 仍为 `138995b31a615833e04a729e8d619c829b9667c4`，工作区包含未提交 E3 实现及本次修复；没有提交或重置既有改动。

用户已确认采用与 Zed 相同的随包 Lilex 默认字体策略，提供 Regular / Bold / Italic / BoldItalic 四个原始变体，不增加通用族名解析、不修改 GPUI 源码 / 版本或引入合成斜体。四份字体及原始 OFL 1.1 许可从本地 Zed 快照 `ba63a022c9a22ca66d68ef2a8f0bb9b5b0a9984d` 的 `assets/fonts/lilex/` 原样复制，逐字节核对一致；它们现在属于 semacs 自身资源，构建不依赖本地 Zed 路径。

| 文件（`assets/fonts/lilex/`） | SHA-256 |
|---|---|
| `Lilex-Regular.ttf` | `8a467ad5a1941dc7c9cebaee34e3ed9364a73b48393034e363e4b528bd711d04` |
| `Lilex-Bold.ttf` | `3369214e2ecddb45ae5024b11d45dd637e99e9d2127ab729817d41d72b8a9c68` |
| `Lilex-Italic.ttf` | `a6e52046b91ed12a6af50bb74e70e27dea4a54eab2bed8947db4c1128ba83d45` |
| `Lilex-BoldItalic.ttf` | `9629bd65ee7376879414560e37281653ca82dcf33e143d7473341b2f88206985` |
| `OFL.txt` | `a356708a399e3f54b937ec7d9b722c26dabba21365b60f95c56eb7ebca0dee40` |

归属与二进制分发时保留字体版权 / OFL 的义务已登记在 `THIRD_PARTY.md`。

## 修复范围

- 默认族名改为具体的 `Lilex`，维持 16 px / 1.5 倍行高。公共应用初始化入口在组件初始化和首个视图排版前，通过同一 GPUI TextSystem 加载内嵌字体，不进行系统字体安装。
- JS 配置省略字体族时统一使用 `FontSettings` 默认值，消除另一个硬编码 `monospace`；只设置字号、多次部分声明和显式自定义族名继续按既有合并规则工作。
- 当前示例、人工 fixture 生成器和性能驱动继承或引用产品默认字体，不再主动切回不支持的通用族名。配置只去除显式 family，保留原字号与其余行为；后续宿主批次必须使用新目录，不能把旧失败 fixture 改成新样本。
- 为无系统字体的正式回归，Linux 测试显式引用已经存在的 `gpui-pre-wgpu 0.3.10`；只增加 dev-dependency 边，不改变任何锁定包的名称、版本或来源，也不更换生产后端。

## 新增确定性回归

本轮新增 3 项 Rust 测试，并更新默认字体的既有集成断言：

1. 使用 `CosmicTextSystem::new_without_system_fonts` 的空字体数据库，经实际 `app::init` 加载资源后，四个 descriptor 直接命中四个不同 FontId，face 实际字重 / 样式分别为 400 Normal、700 Normal、400 Italic、700 Italic；每个变体的 `i` / `W` advance 相等。实际 shaping 的 ASCII glyph runs 保持对应 FontId，而不是四种请求全部变成普通 fallback。
2. 同一无系统字体后端运行生产视图的普通 / 粗体 / 斜体 / 粗斜体范围样式，核对实际 glyph runs、软换行宽度、命中测试和输入坐标，并验证合成的预编辑操作后坐标失效与恢复。这里是 headless 输入，不是真实 IME。
3. JS 空配置、空字体声明、仅字号及多次部分声明均正确使用 / 保留默认或自定义字体，不再回到另一个硬编码通用族名。

另增 1 项 Python fixture 回归，确认当前字体相关 profile 使用空配置或只声明字号，不覆盖产品默认字体。

## 最终检查

- `cargo test --offline --all-targets`：531 项通过，无忽略。
- `cargo test --offline --all-targets --features native-profiler`：571 项通过，无忽略。
- 两套对应 Clippy（`-D warnings`）通过。
- Python：`scripts/test_*.py` 54 项、`tests/test_*.py` 68 项通过；仅使用离线 fixture / mock，不启动真实桌面程序。
- `cargo fmt --check`、`git diff --check`、fish 语法检查及示例 JS 的严格 TypeScript 检查通过。
- 首批失败目录中的 `body.rs`、`body.md`、隔离 `init.js` 再次核对，均保持原始 seed 哈希；未修改原始失败观察或冻结记录。

日志位于 `.pi/tmp/e3-bundled-font-*.log`。E3 已做过一次集中 reviewer，本修复由主代理自检，没有追加审查。

## 结论与未测边界

随包默认字体修复已在本地通过：不依赖系统字体，即可加载四个真实变体并保持排版 / 输入几何身份一致。没有启动新的系统字体探针、原生窗口、用户 JS 配置、人工验收或性能矩阵；不据此宣称屏幕粗斜体、中文 / emoji 回退、真实 IME 或原生性能通过，性能预算也未调整。

自定义缺失族名的 GPUI fallback 丢失强调、通用族名未解析仍是已知边界，保留在计划，不因默认路径修复而关闭。后续宿主定向复核须另行取得同意，使用新目录验证随包字体，原批失败证据保持不变。E3 尚未验收收尾，不进入 F。
