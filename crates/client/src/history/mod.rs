//! 已验证的远程历史同步、分区缓存与离线查询。

mod cache;
mod decode;
mod sync;
#[cfg(test)]
mod tests;

use std::{io, sync::Arc, time::Duration};

use ::history::{Catalog, Event, Frame, Gap};
use common::{
    SampleIdentity,
    history_api::{DirectoryEntry, HistoryErrorCode, HistorySummary, Id, TimeRange},
};
use serde::{Deserialize, Serialize};

pub use cache::{HistoryCache, SavedHistory};
pub use sync::HistorySync;

const DAY_MS: i64 = 86_400_000;
const HEADER: u64 = ::history::DAY_HEADER_LEN as u64;
const MIB: u64 = 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 32_768;
const MAX_TAIL_BYTES: u64 = 16 * MIB;

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("历史缓存 I/O：{0}")]
    Io(#[from] io::Error),
    #[error("历史编码：{0}")]
    Format(#[from] ::history::Error),
    #[error("历史缓存清单：{0}")]
    Manifest(#[from] serde_json::Error),
    #[error("历史连接：{0}")]
    Connection(String),
    #[error("历史服务端 ({code:?})：{detail}")]
    Remote {
        code: HistoryErrorCode,
        detail: String,
    },
    #[error("无效的历史响应：{0}")]
    Protocol(&'static str),
    #[error("历史缓存超出限制：{0}")]
    Limit(&'static str),
    #[error("历史工作线程未能完成：{0}")]
    Worker(#[from] tokio::task::JoinError),
}

pub type HistoryResult<T> = Result<T, HistoryError>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct CachePolicy {
    pub enabled: bool,
    /// 0 表示不按年龄清理。
    pub retention_days: u32,
    /// 0 表示不按磁盘大小清理。
    pub retention_bytes: u64,
    pub memory_bytes: u64,
    /// 0 表示不自动预取；可见区间仍会同步。
    pub prefetch_days: u32,
    pub refresh_interval: Duration,
    pub trend_window_ms: u64,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
            retention_bytes: 512 * MIB,
            memory_bytes: 32 * MIB,
            prefetch_days: 7,
            refresh_interval: Duration::from_secs(60),
            trend_window_ms: 300_000,
        }
    }
}

impl CachePolicy {
    fn validate(self) -> HistoryResult<()> {
        if self.memory_bytes < ::history::MAX_PACKET_LEN as u64 + HEADER
            || self.memory_bytes > 128 * MIB
            || self.refresh_interval.is_zero()
            || !(10_000..=DAY_MS as u64).contains(&self.trend_window_ms)
            || (self.enabled && self.retention_days == 0 && self.retention_bytes == 0)
        {
            return Err(HistoryError::Limit("缓存、保留或刷新配置"));
        }
        Ok(())
    }

    pub async fn load() -> HistoryResult<Self> {
        tokio::task::spawn_blocking(cache::load_policy).await?
    }

    pub async fn save(self) -> HistoryResult<()> {
        self.validate()?;
        tokio::task::spawn_blocking(move || cache::save_policy(self)).await?
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistorySource {
    Sealed { day: i32, day_id: Id, current: bool },
    Tail { generation: Id },
}

#[derive(Clone, Debug)]
pub struct HistorySample {
    pub identity: SampleIdentity,
    pub frame: Frame,
    pub segment: u64,
    pub catalog: Arc<Catalog>,
    pub source: HistorySource,
}

#[derive(Clone, Debug)]
pub struct HistoryGap {
    pub gap: Gap,
    pub source: HistorySource,
}

#[derive(Clone, Debug)]
pub struct HistoryEvent {
    pub event: Event,
    pub session_id: Id,
    pub source: HistorySource,
}

#[derive(Clone, Debug)]
pub struct LocalCoverage {
    pub day: i32,
    pub day_id: Id,
    pub cached_through: u64,
    pub published_through: Option<u64>,
    pub data_range: Option<TimeRange>,
    pub gap_range: Option<TimeRange>,
    pub durable: bool,
    pub current: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum SyncStatus {
    #[default]
    Never,
    InProgress,
    Synced {
        at_ms: i64,
    },
    Failed {
        at_ms: i64,
        reason: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectorySnapshot {
    pub range: TimeRange,
    pub entries: Vec<DirectoryEntry>,
    pub age_cutoff_day: Option<i32>,
}

#[derive(Debug, Default)]
pub struct HistoryQuery {
    pub samples: Vec<HistorySample>,
    pub gaps: Vec<HistoryGap>,
    pub events: Vec<HistoryEvent>,
    pub coverage: Vec<LocalCoverage>,
    pub summary: Option<HistorySummary>,
    pub directories: Vec<DirectorySnapshot>,
    pub status: SyncStatus,
    pub tail_through: Option<common::history_api::TailCursor>,
    pub local_extent: Option<TimeRange>,
    pub cache_bytes: u64,
    pub warnings: Vec<String>,
    pub truncated: bool,
}

fn now_ms() -> i64 {
    let now = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    now.clamp(i64::MIN.into(), i64::MAX.into()) as i64
}

fn check_range(range: TimeRange) -> HistoryResult<()> {
    if range.from >= range.to {
        return Err(HistoryError::Protocol("查询范围必须是非空半开区间"));
    }
    Ok(())
}

fn day_of(ms: i64) -> Option<i32> {
    ms.div_euclid(DAY_MS).try_into().ok()
}

fn extend_range(slot: &mut Option<TimeRange>, from: i64, to: i64) {
    *slot = Some(match *slot {
        Some(r) => TimeRange {
            from: r.from.min(from),
            to: r.to.max(to),
        },
        None => TimeRange { from, to },
    });
}

fn entry_day(entry: &DirectoryEntry) -> i32 {
    match entry {
        DirectoryEntry::Available(e) => e.day,
        DirectoryEntry::Pending(e) | DirectoryEntry::Evicted(e) => e.day,
        DirectoryEntry::Unavailable { day, .. } => *day,
    }
}
