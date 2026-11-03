# D1 消息显示与历史：宿主定向验收

## 身份与证据

- 本轮取得用户同意后，用户运行 `fish scripts/acceptance.fish messages .pi/tmp/d1-messages-host-01 --consent`，回传完整目录。
- 基线 `1c319567976098accfa3c2c8ed3cebae0a9bf2af`，加 D1 未提交工作区；批次的 `source.patch` 包括未跟踪实现文件，核对时与全部工作区差异逐字节一致。patch SHA-256：`1d8328a55dd3d94a777712e07d8f413da3253739ee1d3c44ec018b96e4de9ebc`。
- 正式 release 二进制 SHA-256：`1e5c41a0d0c5e0b85b01421558e6a7fbe91c8cfa41c79dd850571f01c10087aa`，与批次记录及核对时的 `target/release/semacs` 一致。
- Arch Linux x86_64，Wayland / Hyprland，Rust 1.99.1，`LANG=en_US.UTF-8`。`fc-match monospace` 为 Noto Sans CJK SC Regular，不将该查询当作渲染字体实际命中证明。
- 一个正式窗口，独立 `XDG_CONFIG_HOME`，只操作本批可丢弃文件；不截图、不注键、不代改输入法、不运行性能矩阵。

## 结果

`files/result.json` 为 `completed=true`、`errors=[]`；原始 `observations.tsv` 与报告一致，五项均为 yes：

| 观察 | 范围 |
|---|---|
| message-long | 长错误自动折行、受限高度，不覆盖 mode line、不替换正文 |
| message-multiline-history | 多行错误布局，`C-h e` 不主动选择另一 pane，历史中的长 / 多行起止标记及详情完整 |
| message-minibuffer | `[No match]` 不修改读入，继续输入不吞键，约两秒清除，取消恢复正文 |
| message-layout-font | 窄窗口及按步骤放大到 32px 后的消息、prompt、输入和 mode line 目视关系 |
| message-copy-editing | 历史中复制、正文 yank / undo，以及返回正文编辑和保存 |

上述视觉、完整标记、焦点和交互结果来自用户观察，不是日志自动断言或截图复核。

独立读取最终文件确认：

- `body.txt` 为 `Xalpha beta\nsecond line\n`。
- `control.txt` 保持 `untouched\n`。
- `config/semacs/init.js` 与本批生成的隔离配置逐字节相同；全部 seed SHA-256 与 fixture manifest 一致。
- `COMPLETED.txt` 存在，未出现 STOP；fish 流程支持应用正常退出，stdout / stderr 均为空。核对过程未改写原始报告或其他批次产物。

## 结论与边界

本轮约定的 D1 宿主定向验收通过，支持长 / 多行错误重叠修复、读入提示及恢复、消息回看 / 复制的人工确认；结合[本地验证](d1-messages-local.md)，可进入阶段收尾确认。不改写 D 阶段修复前的冻结记录。

`notes.txt` 只有 `yes`，未提供正常 / 窄窗口的像素尺寸；24px / 32px 是步骤设置和用户观察，不是独立测量。没有截图或逐帧证据，不扩大为全部尺寸 / 字体矩阵。原生性能、真实 IME、GPU 执行及呈现测量仍未覆盖；不因这些边界自动安排补测。新一轮真实环境测试须重新取得同意。
