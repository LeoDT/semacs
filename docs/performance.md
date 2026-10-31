# 性能基线

本文件记录 M0 建立的可复现性能样本、测量方式与首轮结果。基线用于发现明显阻塞和后续
里程碑的回归，不是 GPU 实机帧率承诺。

## 1. 环境

- 日期：2026-10-31
- 机器：AMD Ryzen 7 7700 8-Core，30 GiB 内存，Radeon 610M（显示输出未参与 headless 测量）
- 系统：Arch Linux，Linux 7.2.11-arch1-1，Wayland 会话
- Rust：1.99.1
- 依赖：gpui-kit 0.8.1（crates.io，Cargo.lock 固定）
- 构建：--release

## 2. 样本

生成脚本：scripts/perf_sample.py。行为固定、无随机数；默认生成约 8 MiB、UTF-8 无 BOM、
LF、末尾有换行的文本。内容混合 ASCII、CJK、重音 / 希腊 / 俄文、emoji 与每 5000 行
一条约 19 KiB 的长行。

    python3 scripts/perf_sample.py .pi/tmp/perf-sample.txt
    python3 scripts/perf_sample.py .pi/tmp/perf-sample-crlf-bom.txt --crlf --bom

本轮样本统计由 examples/perf_load.rs 输出：

| 样本 | 文件字节 | 内部文本字节 | 行数 | 最长行字符 | ASCII 字符 | CJK 字符 | 其它非 ASCII |
|---|---:|---:|---:|---:|---:|---:|---:|
| LF / 无 BOM | 8,388,648 | 8,388,648 | 144,632 | 19,212 | 4,881,339 | 325,422 | 1,048,577 |
| CRLF / BOM | 8,388,658 | 8,246,595 | 142,061 | 19,212 | 4,801,640 | 319,635 | 1,029,935 |

行数按换行符切分计，包含末尾换行后的空段。

## 3. 文件层：读取 + 解码

命令：

    cargo build --release --example perf_load
    RUNS=3 scripts/perf_baseline.sh .pi/tmp/perf-sample.txt
    RUNS=3 scripts/perf_baseline.sh .pi/tmp/perf-sample-crlf-bom.txt

本轮 3 次结果（热页缓存）：

| 样本 | read ms | decode ms |
|---|---:|---:|
| LF / 无 BOM | 0.56 / 0.66 / 0.72 | 5.41 / 5.76 / 5.82 |
| CRLF / BOM | 0.92 / 0.93 / 0.99 | 9.87 / 10.60 / 11.47 |

结论：8 MiB 文件在文件层的读取与格式转换远低于可感知阈值；CRLF 需要额外扫描与
替换，约为 LF 的两倍，仍为毫秒级。M2 的真实保存往返使用同一文件层。

## 4. 应用层：打开、首帧与滚动

命令（release + 热页缓存）：

    SEMACS_PERF_SAMPLE=.pi/tmp/perf-sample.txt \
      cargo test --release --test perf_open -- --ignored --nocapture

该测试使用 gpui-kit 的 test-support headless 窗口，走真实 AppShell 构造、EditorState
装载与渲染路径。滚动测量 10 次向编辑区发送 20 行滚轮事件；gpui-kit 的测试滚动辅助函数
每次调用包含一次目标查找前渲染和一次事件后渲染，因此结果记为“每次滚动调用”平均耗时，
不冒充单帧时间。随后单独测量 10 次无变更 render_frame 的平均耗时。
它不使用 GPU 合成，因此只能作为回归基线，不能当作 Wayland 实机帧率。

首轮基线暴露出软换行问题：同一样本在启用 soft_wrap 时，release 首帧约 758 ms，
debug 约 6.0 s；M0 改为硬换行后，release 首帧约 50 ms，失焦后重新 focus 的一帧约 4 ms，
点击 focus 约 14–16 ms。因此 M0 使用 hard wrap，软换行行为留到 M3 在明确交互与性能
预算后重新评估。

关闭软换行后的 3 次结果：

| run | 读取 + 解码 ms | 窗口 + 首帧 ms | 点击 focus ms | 每次滚动调用平均 ms | 空帧渲染平均 ms |
|---:|---:|---:|---:|---:|---:|
| 1 | 8.711 | 51.214 | 14.164 | 11.087 | 3.835 |
| 2 | 8.632 | 52.918 | 14.169 | 11.766 | 3.970 |
| 3 | 8.426 | 52.942 | 16.191 | 11.618 | 3.845 |

失焦后重新 focus 的一帧为 4.104–4.427 ms。测试断言编辑区初始 scroll_offset.y 为 0、
滚动后下移至少 1000 px（当前为 -4000 px），并断言 Buffer 中的文本长度与解码结果一致
且超过 1 MB，避免空转或加载失败时仍产出基线。

尚未完成：真实 Wayland / GPU 下的窗口映射与滚动观感。当前开发环境没有可用的
Vulkan 设备（vulkaninfo 无法枚举 GPU），headless compositor 路径无法完成窗口映射，
因此真实首帧由用户在有 GPU 的会话中手工确认后回填；自动基线以第 3、4 节的 release
headless 数据为准。

## 5. 复测约定

- M1 增加按键到显示更新的延迟测量；
- M3 复测本文件所有基线；出现未解释的回归先定位，不修改预算掩盖；
- 样本规模、硬件、构建模式与命令必须随结果一起记录。
