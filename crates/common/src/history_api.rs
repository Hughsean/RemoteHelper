//! 在经过认证的加密 RPC 通道中传输的版本化历史消息。
use serde::{Deserialize, Serialize};

pub const HISTORY_FRAME_LIMIT: usize = 1024 * 1024;
pub const PACKET_PAGE_BUDGET: usize = 384 * 1024;
pub type Id = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: i64,
    pub to: i64,
}

impl TimeRange {
    pub fn intersects(self, other: Self) -> bool {
        self.from < self.to && other.from < other.to && self.from < other.to && other.from < self.to
    }
}

impl From<(i64, i64)> for TimeRange {
    fn from((from, to): (i64, i64)) -> Self {
        Self { from, to }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCursor {
    pub generation: Id,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TailCursor {
    pub wal_generation: Id,
    pub offset: u64,
    pub through: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryRequest {
    Summary,
    Days {
        range: TimeRange,
        cursor: Option<DirectoryCursor>,
    },
    Blocks {
        day: i32,
        day_id: Id,
        after: u64,
        through: u64,
    },
    Tail {
        cursor: Option<TailCursor>,
    },
    GetSettings,
    SetSettings {
        expected_revision: SettingsRevision,
        history: EditableHistory,
    },
    AckClock {
        store_id: Id,
        event_id: Id,
        confirmation: Id,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryResponse {
    Summary(HistorySummary),
    Days(DayPage),
    Blocks(PacketPage),
    Tail(PacketPage),
    Settings(SettingsSnapshot),
    SettingsConflict(SettingsSnapshot),
    Clock(Option<ClockStatus>),
    Error(HistoryError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistorySummary {
    pub store_id: Option<Id>,
    pub range: Option<TimeRange>,
    pub latest: Option<crate::SampleIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_wall_ms: Option<i64>,
    pub disk_bytes: u64,
    pub storage: StorageStatus,
    pub age_cutoff_day: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayEntry {
    pub day: i32,
    pub day_id: Id,
    pub published_through: u64,
    pub data_range: Option<TimeRange>,
    pub gap_range: Option<TimeRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayPage {
    pub entries: Vec<DirectoryEntry>,
    pub next: Option<DirectoryCursor>,
    pub age_cutoff_day: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DirectoryEntry {
    Available(DayEntry),
    Pending(Eviction),
    Evicted(Eviction),
    Unavailable { day: i32, error: HistoryErrorCode },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eviction {
    pub day: i32,
    pub day_id: Id,
    pub data_range: Option<TimeRange>,
    pub gap_range: Option<TimeRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PacketPage {
    /// 以 Base64 编码从 `offset` 开始的连续字节。
    pub bytes: String,
    pub offset: u64,
    pub next_offset: u64,
    pub through: u64,
    pub tail_cursor: Option<TailCursor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HistoryErrorCode {
    Unauthorized,
    Starting,
    Busy,
    InvalidRange,
    DirectoryRestartRequired,
    MissingDay,
    DayChanged,
    DayExpired,
    DayPendingDeletion,
    CorruptDay,
    UnsupportedVersion,
    InvalidCursor,
    TailRestartRequired,
    BudgetTooSmall,
    Storage,
    InvalidSettings,
    ClockEventChanged,
    ConfirmationExpired,
    ClockChanged,
    SaveFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryError {
    pub code: HistoryErrorCode,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageStatus {
    Starting,
    Disabled,
    Recording,
    ReadOnly(String),
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsRevision {
    pub number: u64,
    pub digest: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditableHistory {
    pub enabled: bool,
    pub idle_interval_ms: u64,
    pub min_interval_ms: u64,
    pub seal_interval_secs: u64,
    pub retention_days: u32,
    pub retention_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsSnapshot {
    pub history: EditableHistory,
    pub revision: SettingsRevision,
    pub can_manage: bool,
    pub directory: String,
    pub summary: HistorySummary,
    pub clock: Option<ClockStatus>,
    pub pending_deletions: usize,
    pub retention_error: Option<String>,
    pub external_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockStatus {
    pub store_id: Id,
    pub event_id: Id,
    pub detected_wall_ms: Option<i64>,
    pub current_wall_ms: Option<i64>,
    pub confirmation: Option<Id>,
}
