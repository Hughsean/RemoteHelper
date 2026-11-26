# 历史格式 v1

> [English](FORMAT.md) | 简体中文

存储生命周期和时钟语义见[设计文档 §§5–7](../../.doc/design/history-persistence.md)。本文件规定 §§7.1–7.2 所选定的字节布局。定宽字段均为小端序。`ID` 为 16 字节，按 UUID 字符串顺序排列（不含分隔符）。所有 CRC 均为 CRC32C，校验范围为对应日文件头、伴随文件或包中位于 CRC 之前的全部字节，并以 `u32` 存储。

| 结构 | 按字节顺序排列的字段 |
| --- | --- |
| `.rhh` 或 `.wal` 日文件头（30 字节） | `RHH\0`、`u16 version=1`、`i32 UTC day`（自 1970-01-01 起的天数）、`ID day_id`、CRC |
| `.pub` 伴随文件（sidecar，恰好 34 字节） | `RHP\0`、`u16 version=1`、`ID day_id`、`u64 published_offset`（已发布文件边界，不含该偏移本身）、CRC |
| 包（最多 128 KiB，含包头与 CRC 等开销） | `u8 tag`、`u32 payload_len`、载荷字节、CRC |

包标签为 `1 CATALOG`、`2 FRAME`、`3 MARKER`、`4 BLOCK`、`5 GAP`、`6 ROLL`（预留的不透明载荷）。存在标志字节取值为 `0` 或 `1`；位图从最低有效位开始编号，未使用的高位必须为零。时钟证据先编码一个标志字节（第 0–2 位分别表示 wall/tick/awake，其余位为零），再按位顺序编码存在的字段（`i64 wall_ms`、`u64 tick_ms`、`u64 awake_ms`）。

- **CATALOG**：`u32 version`、`u16 number_of_columns`（1–64），随后是每一列的 `u8 encoding`、`u8 key_len`、键字节、`u8 unit_len`、UTF-8 单位字节。每个文本字段最多 96 字节；键必须是唯一的 ASCII 字母、数字、点、下划线或连字符字符串。单位可以为空。编码：1 percentage 百分比（`u16`，每个百分点 100 步，最大 10000）、2 Celsius 摄氏度（先加 100，再以每摄氏度 100 步表示，`u16`）、3 watts 瓦特（`u16`，每瓦 10 步）、4 unsigned integer 无符号整数（`u64`）、5 unsigned rate 无符号速率（`u64`）。单位只是标签。
- **FRAME**（仅用于 WAL）：`u64 sample_seq`、`i64 time_ms`、`u32 period_ms`（必须为正）、时钟证据、列有效位图，以及按目录顺序、以各列原生整数宽度存储的存在值。适用此前最近一次的目录定义和会话定义。
- **MARKER(Session)**：`u8 kind=0`、`ID session_id`、开机标识存在标志字节，以及可选的 `ID boot_id`。空（nil）开机标识编码为缺失。
- **MARKER(Event)**：`u8 kind`（1 start 启动、2 exit 退出、3 disabled 停录、4 resumed 恢复、5 clock segment 时钟分段、6 continuation 接续）、`ID event_id`、`u64 segment`、`u64 session_ref`、序号存在标志字节和可选的 `u64 sample_seq`、时钟证据。接续仅用于 WAL：它把现有段带入替换后的 WAL，但不报告新事件。
- **GAP**：`i64 from_ms`、`i64 to_ms`（右端不含，且大于 `from_ms`）、`u8 reason`（1 disabled 记录已关闭、2 storage 存储、3 recorder overrun 记录队列溢出、4 rebooted 重启、5 stopped 服务已退出、6 interrupted 服务中断、7 sleep 睡眠或休眠、8 clock jump 时钟跳变、9 stalled 采样停顿、10 unknown 未知）、`u64 segment`、`ID event_id`。
- **BLOCK**（仅用于日文件）：`u32 catalog_version`、`u64 segment`、`u64 session_ref`、`u64 first_seq`、`u64 last_seq`、`i64 first_time_ms`、`i64 last_time_ms`（包含首尾的样本范围）、`u16 count`（1–2,048）、`u32 uncompressed_payload_len`（最多 512 KiB）、最后一个样本的时钟证据、单个标准 zstd 帧。zstd 窗口限制为 512 KiB；不允许 skippable frame（可跳过帧）、拼接和尾随字节。

会话引用是同一文件中位于其前面的 MARKER(Session) 包的起始偏移，不是全局 ID，也不是稳定的样本身份。BLOCK 引用此前声明的同一版本的 CATALOG；在同一文件中将相同版本重定义为不同的列是无效的。调用方将读取限制在已提交（committed）边界内，并校验该边界是包边界。`FileContext` 在原始帧解析之外，单独校验先前定义和允许的包类型。

## 未压缩的 BLOCK 载荷

时间和序号数组以交织的三元组形式存储，每个样本为 `(time_ms - first_time_ms, sample_seq - previous_sample_seq, period_ms)`，均使用规范化的无符号 LEB128。首个偏移量为零；时间非递减，样本序号严格递增。随后按目录中的每一列，先写入覆盖整个块的有效位图，再按样本顺序只写入存在的值。每列的第一个存在值以及所有存在的速率值均为规范化的无符号 LEB128。后续非速率值是与前一个**存在**值的差值，使用 `i128` 计算以覆盖完整的无符号值域，再进行 zigzag 编码；zigzag 表示同样为规范化的无符号 LEB128。最后一列之后没有填充或尾随载荷字节。编码器在样本边界处拆分超限批次，并保留每个子块自身最后一个样本的时钟证据。
