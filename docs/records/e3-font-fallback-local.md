# E3 字体失败：无系统字体的 fallback 定向诊断

## 身份与范围

本轮接续 [首批宿主失败](e3-host-manual-01.md)，HEAD 仍为 `138995b31a615833e04a729e8d619c829b9667c4`，工作区包含未提交 E3 实现。只执行可控字体后端实验、源码检查与既有 headless 样式测试；未修改产品代码、依赖或失败 fixture，未追加 reviewer。

使用项目锁定的 `gpui-kit 0.8.1` / `gpui-pre 0.3.10`。临时诊断 crate 复制项目 lockfile 后离线构建，原有依赖未换版本；文件与日志位于 `.pi/tmp/e3-font-fallback/`。未读取系统字体、用户配置或启动原生窗口。

## 实验与确定事实

以 `src/test_support.rs::StyledTextSystem` 为基础，包装一个只接受 `DejaVu Sans` 的可控 `PlatformTextSystem`。这里的族名仅用于命中 GPUI 固定 fallback 列表，不读取同名字体文件。后端为普通、粗体、斜体、粗斜体分别返回 FontId 1、2、3、4，并采用不同字宽。

输入使用项目的 `FontSettings::font` 和 `FontEmphasis::font` 构造四种 descriptor，核对族名、weight / style 后交给 GPUI `TextSystem` 与 `WindowTextSystem::shape_line`；记录实际后端请求、FontRun、ShapedRun 及字宽。结果：

| 请求族名 | 解析顺序 | 四种样式的 FontId / glyph FontId | `abcd` 总宽 |
|---|---|---|---|
| 可用的 `DejaVu Sans` | 普通 → 粗体 → 斜体 → 粗斜体 | 1、2、3、4 | 52 px |
| 不可用的 `monospace` | 普通 → 粗体 → 斜体 → 粗斜体 | 全为 1 | 40 px |
| 不可用的 `Nonexistent Probe Family` | 粗斜体 → 斜体 → 粗体 → 普通 | 全为 1 | 40 px |

缺失族名时，原始请求确实带有对应的 weight / style，但 fallback 命中的 `DejaVu Sans` descriptor 始终是 weight=400、style=Normal；后端有其他变体也没有机会被请求。重复解析不再调用后端，折叠后的身份被缓存。三个诊断测试全部通过，表示复现了当前行为，不表示产品问题已修复。

## 源码交叉核对

- semacs 的 `FontEmphasis::font` 保留基础字体族并叠加字重 / 斜体；`Typography::runs` 生成对应 TextRun，`FontMeasure` 和 `ShapedRow::shape` 将它们交给 GPUI。没有发现这条路径主动清除强调。
- 锁定 GPUI 的 `text_system.rs::resolve_font_uncached` 在原 descriptor 加载失败后，直接遍历预建的普通字体 fallback descriptor，没有叠加请求的 weight / style，再将同一个 fallback FontId 缓存到各原始请求。
- Linux 使用 `gpui_wgpu::CosmicTextSystem`。其 `load_family` 经 `font_name_with_fallbacks` 后按数据库中的具体 family 名相等匹配；该别名映射没有处理 `monospace`。因此 fontconfig 能解析通用族名，并不证明 GPUI 的这一入口也能解析。

以上确认了**缺失字体族时强调丢失的依赖边界行为**；尚无宿主实际命中 FontId / face 的证据，不能写成此次宿主失败已确认的唯一根因，也不能排除斜体变体缺失。

## 既有回归与检查

- `cargo test --lib style_tests -- --nocapture`：12 项通过，覆盖 descriptor 继承、Markdown 强调、带样式测量 / 换行、几何 / 输入坐标和布局复用等既有路径。
- 诊断：`CARGO_TARGET_DIR="$PWD/target" cargo test --manifest-path .pi/tmp/e3-font-fallback/Cargo.toml --offline --locked --test fallback -- --nocapture --test-threads=1`：3 项通过。
- `git diff --check` 通过。未重跑全目标 / profiler / 性能矩阵；本轮不修改性能预算。

## 下一步与修复候选（均未授权实施）

先取得用户同意，运行一次独立、只读的同版本 Linux 字体解析诊断：只加载系统字体并请求 `monospace` 的四种 descriptor，记录原始后端请求是否成功、GPUI fallback 请求、最终 FontId 与命中的具体 family / weight / style。不启动窗口、不注键、不截图、不加载用户 JS 配置、不改字体 / 配置 / 失败样本，不运行人工验收或性能。若独立诊断与产品路径有差别，再另行明确窗口 trace 的范围，不自动升级测试权限。

若证实同一路径，最小结构性修复应位于统一的字体解析边界，而非在测量与绘制各自补丁：区分通用族名解析与缺失族 fallback，后者保留请求的 weight / style；不以改 fixture 字体、硬编码宿主字体或对每个字形手动倾斜来绕过。涉及字体策略、依赖补丁 / 升级的具体方案须先确认，当前不改 registry 或切换后端。可用字体本身缺少斜体时是否合成仍是另一个策略问题，不能混为本次 fallback 修复。

获准修复后，将可用族四变体、缺失族保留强调、解析顺序 / 缓存一致性纳入正式回归，并继续核对测量、wrap、glyph runs 与输入几何使用同一字体身份；宿主定向复核另行授权，不自动重跑原整批验收。
