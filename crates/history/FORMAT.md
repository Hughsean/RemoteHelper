# History format v1

The storage lifecycle and clock semantics are specified in [design §§5–7](../../.doc/design/history-persistence.md). This file specifies the bytes chosen for §§7.1–7.2. Fixed-width fields are little-endian. `ID` is 16 bytes in UUID string order (without separators). All CRCs are CRC32C of all preceding bytes of the header, sidecar, or packet itself, stored as `u32`.

| Structure | Fields in byte order |
| --- | --- |
| `.rhh` or `.wal` day header (30 bytes) | `RHH\0`, `u16 version=1`, `i32 UTC day` (days since 1970-01-01), `ID day_id`, CRC |
| `.pub` sidecar (34 bytes, exactly) | `RHP\0`, `u16 version=1`, `ID day_id`, `u64 published_offset` (exclusive file boundary), CRC |
| Packet (at most 128 KiB, including overhead) | `u8 tag`, `u32 payload_len`, payload bytes, CRC |

Packet tags are `1 CATALOG`, `2 FRAME`, `3 MARKER`, `4 BLOCK`, `5 GAP`, `6 ROLL` (reserved opaque payload). A presence byte is `0` or `1`; bitmaps number bits least-significant first and zero unused high bits. Clock evidence encodes a flags byte (bits 0–2 signify wall/tick/awake, other bits zero), then present fields in bit order (`i64 wall_ms`, `u64 tick_ms`, `u64 awake_ms`).

- **CATALOG**: `u32 version`, `u16 number_of_columns` (1–64), then each column's `u8 encoding`, `u8 key_len`, key bytes, `u8 unit_len`, UTF-8 unit bytes. Text fields are at most 96 bytes each; keys are unique ASCII alphanumeric/dot/underscore/dash strings. Unit may be empty. Encodings: 1 percentage (`u16` at 100 steps per percentage point, maximum 10000), 2 Celsius (`u16` at 100 steps per degree after adding 100), 3 watts (`u16` at 10 steps per watt), 4 unsigned integer (`u64`), 5 unsigned rate (`u64`). Units are labels.
- **FRAME** (WAL only): `u64 sample_seq`, `i64 time_ms`, `u32 period_ms` (positive), clock evidence, column validity bitmap, and present values in catalog order at their native integer widths. The latest preceding catalog and session definitions apply.
- **MARKER(Session)**: `u8 kind=0`, `ID session_id`, boot ID presence byte, and optional `ID boot_id`. Nil boot IDs are encoded as absent.
- **MARKER(Event)**: `u8 kind` (1 start, 2 exit, 3 disabled, 4 resumed, 5 clock segment, 6 continuation), `ID event_id`, `u64 segment`, `u64 session_ref`, sequence presence byte and optional `u64 sample_seq`, clock evidence. Continuation is WAL-only and carries the existing segment into a replacement WAL without reporting a new event.
- **GAP**: `i64 from_ms`, `i64 to_ms` (exclusive and greater than `from_ms`), `u8 reason` (1 disabled, 2 storage, 3 recorder overrun, 4 rebooted, 5 stopped, 6 interrupted, 7 sleep, 8 clock jump, 9 stalled, 10 unknown), `u64 segment`, `ID event_id`.
- **BLOCK** (day log only): `u32 catalog_version`, `u64 segment`, `u64 session_ref`, `u64 first_seq`, `u64 last_seq`, `i64 first_time_ms`, `i64 last_time_ms` (inclusive sample bounds), `u16 count` (1–2,048), `u32 uncompressed_payload_len` (at most 512 KiB), clock evidence of the final sample, a single standard zstd frame. The zstd window is limited to 512 KiB; skippable frames, concatenation, and trailing bytes are not allowed.

A session reference is the start offset of a preceding MARKER(Session) packet in the same file, not a global ID or a stable sample identity. A BLOCK refers to a preceding CATALOG of its stated version; redefining the same version to different columns in a file is invalid. The caller restricts reads to the committed boundary and validates that it is a packet boundary. `FileContext` validates preceding definitions and allowed packet types separately from raw framing.

## Uncompressed BLOCK payload

Time and sequence arrays are stored as interleaved triples of canonical unsigned LEB128 values: `(time_ms - first_time_ms, sample_seq - previous_sample_seq, period_ms)` per sample. The first offsets are zero; times are nondecreasing and sequence numbers strictly increase. Then, for each catalog column: validity bitmap across the block, followed by only the present values in sample order. The first present value of each column and every present rate are unsigned canonical LEB128. Subsequent non-rate values are zigzag-encoded differences against the preceding **present** value, computed in `i128` to cover the full unsigned domain; the zigzag representation is also canonical unsigned LEB128. No padding or trailing payload bytes follow the last column. Encoders split over-limit batches at sample boundaries, retaining each sub-block's own final clock evidence.
