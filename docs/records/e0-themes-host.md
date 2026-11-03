# E0 主题宿主定向验收

## 批次与身份

用户同意本轮真实窗口配色验证后，在正常 fish / Wayland 宿主终端运行：

```fish
fish scripts/acceptance.fish themes .pi/tmp/e0-themes-host-01 --consent
```

- 产物目录：`.pi/tmp/e0-themes-host-01/`；构建成功，存在 `COMPLETED.txt`，无 STOP，应用 stdout / stderr 均为空。
- 基线：`9bff7046a03caea226621b3039b21ec69faabd73` 加本轮 E0 修改。
- 本批 `source.patch` SHA-256：`0e52cafff645469f508b56b3cbbba2a682490ca8963729f77639cfec244939d2`；离线核对时与工作区按入口同样方法生成的完整补丁逐字节一致，后续仅新增本记录和更新状态文档。
- 产品 / 工具源码身份与 [本地记录](e0-themes-local.md#身份与环境) 一致：154 文件清单 SHA-256 为 `d59f5ddfb0332c723ef34d8e746277b0de4021a77ca413b962917f16df262ff3`。
- 本批重新构建的 release `semacs` SHA-256：`9fc8d95bc9cd0e86e5d70cb72d6e7ebd9541b4d86848e8fb4e466fffe9272901`，与本批报告及核对时的二进制一致。本批二进制身份单独记录，不以本地构建哈希替代。

## 环境与结果

Linux `7.2.11-arch1-1`、x86_64，rustc `1.99.1 (58cf8f9c3 2026-10-13)`；`WAYLAND_DISPLAY=wayland-1`、Wayland / Hyprland、`LANG=en_US.UTF-8`。`fc-match monospace` 返回 Noto Sans CJK SC Regular；这只是字体匹配信息，不证明逐字形实际命中的字体。

用户五组观察全部回答 `yes`：

| 项目 | 确认范围 |
|---|---|
| 启动 / 分屏 | 深色启动、黄色方块光标、region、选中 / 非选中 mode line 及分隔线 |
| 主题切换 | 内置浅深色与自定义纸色切换、用户覆盖保留、旧主题颜色不残留 |
| 取消 / 无效名称 | 取消恢复正文、无匹配保留读入且不切主题 |
| minibuffer / 消息 | prompt、候选命中 / 注释 / 选中、读入附注和历史配色及窄窗口观察 |
| 编辑恢复 | 编辑、撤销、保存及正常关闭 |

独立离线核对确认：

- seed 清单与本轮 fixture 定义一致；body 最终为原始 UTF-8 正文前增加一个 `X`。
- control 和独立 `config/semacs/init.js` 字节不变。
- 人工回答原文、`files/result.json` 与独立磁盘字节检查一致，`completed=true` 且无错误。
- 入口只对子进程设置独立配置；未截图、自动注键或脚本切换输入法。

## 结论与边界

E0 本轮定向宿主功能观察通过，与本地回归合并后可供用户确认收尾；不自动启动 E1。

尺寸 / 可读性备注只填写了 `yes`，没有具体窗口宽高；不把五项功能确认外推为已知尺寸的完整布局矩阵。没有原生性能计时、真实 IME 或 GPU / 实际呈现测量，报告均保留 `unmeasured`。旧 M2 profiler 工具的基线失败仍按 [计划](../plan.md#已知问题与未排期事项) 独立跟踪，本批没有修复或复测它。
