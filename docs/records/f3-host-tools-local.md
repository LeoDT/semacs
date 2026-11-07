# F3 单窗口宿主入口：离线工具验证

本记录冻结 `magit-commit` 入口准备与离线自检，不是宿主通过记录。用户已同意本批最小单窗口功能范围，实际执行和回传仍待进行；授权边界与可执行流程见 [F3 定向流程](../acceptance/profiles.md#f3-最小定向验证)，当前状态见[计划](../plan.md)。

## 身份与变更

基础 HEAD 为 `24d996e49a8bcb71ffa7c20f7907b4d3724804c8`，测试对象仍是累计未提交工作区，包含新增脚本，不能只用基础 HEAD 标识。本轮工具准备没有修改 Rust 产品代码；实现与阶段集中审查修复的测试证据保留在 [F3 本地记录](f3-window-commit-local.md)，不覆盖其旧源码身份或测试数量。

工具源清单 SHA-256 为 `6cf57338bba685d68f17021d7dd1fb9d96d8274c0e6fec960acbed2e5984de88`：依次对 `scripts/acceptance.fish`、`scripts/magit_commit_manual.py`、`scripts/magit_fixture.py`、`scripts/magit_status_manual.py`、`scripts/test_magit_commit_manual.py`、`scripts/test_magit_status_manual.py` 执行 `sha256sum`，将六行标准输出按 UTF-8 / LF 拼接后再求 SHA-256。它包含直接依赖和测试替身来源，不把文档编辑混入工具身份。

## 离线检查

环境沿用上述产品本地记录中的 Linux x86_64、Git 2.56.0 与 Rust 1.99.1。全部 fixture 都在被忽略的 `.pi/tmp/`，窗口由 mock child 替代，没有启动桌面，也没有访问日常 Git 配置、hooks 或密钥。

- `python3 -B -m unittest discover -s scripts -p 'test_magit*.py' -v`：40 项通过，其中 F3 工具 13、F2 工具 17、F1 fixture 10。
- `fish --no-execute scripts/acceptance.fish`、`cargo fmt --check`、`git diff --check`：通过。
- JS 示例 / 类型声明的严格 `tsc --allowJs --checkJs --noEmit --strict --target es2020 --lib es2020`：通过。
- 本地最终日志为 `.pi/tmp/f3-host-tools-final.log`，SHA-256 `939933a4dae6f4b8ef25aa651034113de52667119eb28ec669134b2e2be80160`；不将完整日志复制进文档。

自检覆盖取消 → 活动但未保存 → 成功提交的独立检查点，完整 index / tree 和提交 parent / 消息，额外 refs、错误 staged 内容、配置 / 正文 / manifest / symlink / lock 变化的拒绝；模拟一窗口完整推进、人工 no / EOF、进程异常、二进制变化、退出异常和超时清理，并检查 fish 只构建产品且传播失败。校验只读并保留失败数据，不自动修复或重试。

helper 目录生命周期在工具自检中由目录替身模拟；真实 Git fixture 操作不等于真实 GUI / helper 集成，本轮没有补记新的产品或宿主结果。fish 的实际 release 构建、二进制 / 完整源码差异与环境采集将在用户启动本批入口时执行。

## 待取得证据

用户在正常 Wayland 宿主终端运行本批，按四组提示观察并回传整个输出目录；成功与失败均须离线核对。尚未取得真实分屏 / 焦点 / OS 关闭观察，不宣称阶段宿主验证完成。原生性能、IME、密码读入、日常 hooks / 签名工具、GPU 与屏幕呈现均不在本批范围。
