# E3 只读系统字体解析诊断：monospace 回退为普通字体

## 授权、身份与范围

用户在 [离线 fallback 诊断](e3-font-fallback-local.md) 后授权一次只读系统字体解析。HEAD 仍为 `138995b31a615833e04a729e8d619c829b9667c4`，工作区为未提交 E3 实现；本轮未修改产品或项目 Cargo 文件。

独立诊断使用与产品相同的锁定依赖 `gpui-kit 0.8.1` / `gpui-pre 0.3.10` / `gpui-pre-wgpu 0.3.10`，按 Linux 平台源码构造 `CosmicTextSystem::new("IBM Plex Sans")`。仅加载本机字体数据库并请求默认 `monospace` 的普通、粗体、斜体、粗斜体 descriptor；未启动 Application / 原生窗口、注键、截图或绘制字形，未加载 semacs 用户 JS 配置，未改字体、配置、registry 或首批失败样本，未执行性能或人工验收。

临时 manifest、源码、构建日志与运行日志位于 `.pi/tmp/e3-font-fallback/`。独立程序只运行一次，退出码 0；这不是窗口验收通过证据。

| 产物 | SHA-256 |
|---|---|
| `system_font_trace.rs` | `ef885f8c107f5ebfc7e1488be0a1651faed130503916d27fb2c02b925a7c11bd` |
| `target/debug/system-font-trace` | `5849f595d108a79f07f4bb847b59a8aef89c54d5c4077d9f643bede196389fd7` |
| `system-font-trace.log` | `1d4e73af6ffebc8d24ab546b81169c71a206fbbb7b413a31f99ef769b4147b5a` |

## 运行结果

- 字体数据库没有字面族名 `monospace`。直接调用 CosmicTextSystem 解析四种 descriptor 均失败，错误为 `requested font family contains no font matching the other parameters: no font found`。
- 经 GPUI TextSystem 解析普通请求时，`.ZedMono`、`.ZedSans`、`Helvetica`、`Segoe UI`、`Ubuntu` 依次失败；`Adwaita Sans` 成功。以上 fallback 请求全部为 weight=400、style=Normal。
- 后续粗体、斜体、粗斜体的 `monospace` 请求同样失败，并复用缓存的同一个普通 fallback FontId。

| 原请求 weight / style | 最终 FontId | 命中 family | face 实际 weight / style |
|---|---|---|---|
| 400 / Normal | 1 | Adwaita Sans | 400 / Normal |
| 700 / Normal | 1 | Adwaita Sans | 400 / Normal |
| 400 / Italic | 1 | Adwaita Sans | 400 / Normal |
| 700 / Italic | 1 | Adwaita Sans | 400 / Normal |

family 来自成功的后端 descriptor；该具体名称在 CosmicTextSystem 的数据库 family 相等匹配中命中，并非用 fc-match 推断。face 的实际字重 / 样式通过 `CosmicTextSystem::font_weight_and_style` 读取。FontId 只在此次进程内有意义。

## 结论与边界

已在本机同版本的真实字体解析链路确认：`monospace` 未作为通用族名解析，触发固定 fallback；fallback 又丢失原请求字重 / 样式，使四种请求全部变成 Adwaita Sans Regular。此结论与无系统字体的确定性实验一致，足以说明当前默认字体路径存在必须修复的问题，不能再将粗体不可见归因于宿主缺少 Bold 字体。

本轮不是首批产品窗口的运行时 trace，没有重放原失败二进制或检查最终 glyph / 屏幕呈现；独立解析结果不能冒充宿主样式、IME 或性能复测通过。修复通用族名和 fallback 属性丢失，也不自动保证系统提供真实斜体变体。

## 建议修复边界（待用户确认）

在统一字体解析层处理两件事：通用族名不能按具体 family 字面匹配后直接掉入普通 UI 字体；缺失族 fallback 应保留请求的 weight / style，并以完整请求身份缓存。测量、wrap 与绘制仍复用同一个 TextSystem，不在 semacs 各条调用路径分别补偿。

推荐对锁定 GPUI 采取显式、可追溯的依赖修复，而非更换 fixture、硬编码本机字体、静默升级 / 切后端或修改 registry 缓存；具体依赖来源与通用族名策略须先确认。只映射 `fontdb::Family::Monospace` 也不能先宣称等同完整 fontconfig 选择，需验证候选族存在及缺失时的行为。不将合成斜体加入本次修复。

回归应覆盖可用族四变体、缺失族强调保留、通用族名不落普通 UI fallback、请求顺序与缓存一致性，以及测量 / wrap / shaping / 输入几何身份一致。当前停在方案确认，不自动复跑诊断、窗口验收或性能。
