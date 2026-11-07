# F3 单窗口宿主功能验证：host-01

本记录冻结用户获准运行的一窗口 F3 定向结果，不随以后代码更新。执行范围和停止规则见 [F3 定向流程](../acceptance/profiles.md#f3-最小定向验证)，产品行为见 [F3 契约](../emacs-fidelity.md#阶段-f3临时-window-与普通-commit)，交付结论见 [阶段归档](../archive/f3-window-commit.md)。

## 批次与身份

- 用户回传目录：`/home/leodt/src/semacs/.pi/tmp/f3-host-01`；模式为 `magit-commit`。本批授权限于单窗口、独立空配置与可丢弃仓库，人工英文输入，无注键、输入法变动、截图或日常 hooks / 签名密钥。
- 基础 HEAD：`24d996e49a8bcb71ffa7c20f7907b4d3724804c8`。受测对象是累计未提交 F3 工作区；`source.patch` 包含 tracked 差异和 untracked 文件，SHA-256 `8bda1441c5706f46369580a770494232329f91a300d40370e11668c71a41bd32`，不能只用基础 HEAD 标识源码。
- release `semacs` 二进制 SHA-256：`31a80bf4defd840082e9d14565eeb600c7551010ee5e9460ba9e277c9dd5c343`。`cargo build --release --bin semacs` 成功；入口的 `binary.sha256`、报告及离线重读的实际二进制一致。
- `commit/fixture.json` SHA-256：`3f6098a9ef5a6b94d7e732c07caead275bebc4ad00a9cb2283647eaf9975e92f`；`commit/result.json` SHA-256：`efa5ac3b998b2a5636232070b322f2e200b9a41f8fb867c66e5c19a152811fe1`。
- 离线在独立忽略目录从基础 HEAD 加回传 patch 还原源码，358 个文件与收尾前工作区逐字节一致，Git status 也与批次快照一致；随后只做收尾文档 / 本地清单更新。没有重放 GUI 操作或修改回传 fixture。

## 环境与观察来源

Linux `7.2.11-arch1-1` x86_64，Hyprland / Wayland（`wayland-1`），Git 2.56.0、rustc 1.99.1，宿主 LANG 为 `en_US.UTF-8`；测试子进程显式使用 `LC_ALL=C`，独立 `XDG_CONFIG_HOME` 和 helper `TMPDIR`，隔离系统 / 全局 Git 配置与继承的 Git 覆盖。

用户记录：`size: 1187,1048, 其他都没问题`。尺寸按用户提供解释，不冒充独立几何诊断。环境 `fc-match monospace` / Bold 返回 Noto Sans CJK SC，只是系统 fontconfig 信息，不证明产品实际采用的字体或完整 fallback 路径。

四组人工观察均为 `yes`：

1. status / unstaged diff 的下方临时显示、局部 `q`、返回原文件与焦点。
2. 消息与 staged companion、进程 buffer 查看 / 返回、编辑取消和 status 保留。
3. 活动消息的 OS 关闭请求、依次拒绝保存 / 确认仍有修改 / 拒绝结束活动进程后继续编辑。
4. 保存完成、Git 成功终态、status 自动刷新和进程历史。

这些视觉 / 焦点 / OS 关闭结论来自用户观察；助手没有独立操作或截图复现。报告 `completed=true`、`returncode=0`、`errors=[]`、`forced_cleanup=false`；窗口 stdout / stderr 均为空，存在 `COMPLETED.txt`，没有 `STOP.txt`。

## Git / 字节证据

驱动在启动前、临时显示后、取消后、拒绝 OS 关闭后、提交成功后及正常关闭后共保留六个检查点。各点完整 index / tree、worktree / 配置和私有 editor 目录状态均匹配；成功与关闭后结果相同，不能用最终状态替代中间检查点。

- 初始 HEAD 为 `88406c569fb335828124ddd2fd9f84bdcb026885`；临时显示、取消和拒绝关闭后保持不变。
- 取消后消息正文为 `F3 host canceled`，editor bridge 已回收；不是丢弃消息文件或回滚 index。
- 拒绝 OS 关闭后仍有一个所属 editor bridge，Git 消息文件尚未保存用户草稿，HEAD / index 不变；继续编辑本身由用户观察确认。
- 成功 HEAD 为 `3fedae29d604f1ed3b8c1a398d245bd804e564b4`，唯一 parent 为初始 HEAD，提交消息恰为 `F3 host saved\n`，完整 tree 等于预置 index。没有额外 refs。
- 六点完整 index 条目摘要均为 `babcf826150b9078cf0b3288d99d889ca85c348048aba8ab81d98ca75454dc9a`；mode / path / blob / stage 从已知字节独立核对。正常 Git index 缓存字节、对象和 reflog 不要求不变。
- 四个 worktree 文件及路径、空 init.js、Git 配置、symbolic HEAD 均保持预期。成功及关闭后无 editor bridge，消息文件 SHA-256 均为 `f75ecff1c332763296bd49b00ea38614560cd9ec122ff73a0d24036a169e827e`。

助手离线只读重核了最终 fixture、完整 index / tree、parent / refs / 消息及实际二进制，并核对六点序列和报告一致性。中间消息原件已被正常后续编辑取代，其中间状态依据驱动执行时的验证结果与摘要，不冒称离线重新执行了历史操作。本地核对摘要留在 `.pi/tmp/f3-host-01-offline-check.json`。

## 结论与边界

本批批准的单窗口功能观察与独立 Git / 字节检查通过，足以结合 [本地实现验证](f3-window-commit-local.md)完成 F3 已确认范围。没有已知阻塞性遗留项，不追加额外宿主批次或第二次集中审查。

未测试原生 CPU 性能、真实 IME、GPU / 屏幕呈现、外部签名工具、密码读入、任意用户 hooks、多 OS frame 或其他平台。OS 关闭覆盖的是拒绝退出并继续编辑，不外推为本批真实发送了 SIGHUP、测试了忽略信号或两级强杀；这些生命周期的隔离证据见本地记录。密码 / passphrase 读入不属于 F3；历史性能观察和原预算保持不变，本批不宣称全部性能预算通过。
