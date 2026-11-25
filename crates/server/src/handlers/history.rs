use crate::history::{HistorySnapshot, Page, QueryError, StorageState};
use crate::state::AppState;
use base64::prelude::*;
use common::{SampleIdentity, history_api::*};
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use tokio::sync::Semaphore;

const DIRECTORY_PAGE: usize = 128;
const DAY_MS: i64 = 86_400_000;
static QUERY_SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(4)));

#[derive(Default)]
pub(crate) struct HistorySession {
    store_id: Option<Id>,
    directory: Option<Directory>,
    pub(super) confirmation: Option<super::settings::Confirmation>,
}

struct Directory {
    id: Id,
    snapshot: Arc<HistorySnapshot>,
    range: TimeRange,
    next: u32,
    opened: Instant,
}

pub(crate) async fn handle(
    state: &AppState,
    session: &mut HistorySession,
    public_key: Option<&str>,
    request: &HistoryRequest,
) -> HistoryResponse {
    let Some(public_key) = public_key else {
        return error(
            HistoryErrorCode::Unauthorized,
            "历史请求需要已验证的扩展登录",
        );
    };
    let Some(history) = state.history.as_ref() else {
        return error(HistoryErrorCode::Storage, "历史记录器不可用");
    };
    let current_store = history.snapshot().store_id;
    if matches!(request, HistoryRequest::Summary) {
        session.store_id = current_store;
        session.directory = None;
    } else if matches!(
        request,
        HistoryRequest::Days { .. } | HistoryRequest::Tail { .. } | HistoryRequest::Blocks { .. }
    ) {
        if session.store_id.is_some() && session.store_id != current_store {
            return error(
                HistoryErrorCode::DirectoryRestartRequired,
                "历史存储实例已改变，请重新读取摘要",
            );
        }
        session.store_id = current_store;
    }
    match request {
        HistoryRequest::Summary => HistoryResponse::Summary(summary(&history.snapshot())),
        HistoryRequest::Days { range, cursor } => {
            if range.from >= range.to {
                return error(HistoryErrorCode::InvalidRange, "历史范围必须为非空半开区间");
            }
            if cursor.is_none() {
                session.directory = Some(Directory {
                    id: rand::random(),
                    snapshot: history.snapshot(),
                    range: *range,
                    next: 0,
                    opened: Instant::now(),
                });
            }
            let Some(directory) = session.directory.as_mut().filter(|dir| {
                dir.range == *range
                    && dir.opened.elapsed().as_secs() < 120
                    && cursor.is_none_or(|cursor| {
                        cursor.generation == dir.id && cursor.offset == dir.next
                    })
            }) else {
                return error(
                    HistoryErrorCode::DirectoryRestartRequired,
                    "请重新打开日目录",
                );
            };
            let mut entries = directory_entries(&directory.snapshot, *range)
                .skip(directory.next as usize)
                .take(DIRECTORY_PAGE + 1)
                .collect::<Vec<_>>();
            let next = if entries.len() == DIRECTORY_PAGE + 1 {
                entries.pop();
                directory.next += DIRECTORY_PAGE as u32;
                Some(DirectoryCursor {
                    generation: directory.id,
                    offset: directory.next,
                })
            } else {
                None
            };
            let age_cutoff_day = directory.snapshot.retention.age_cutoff_day;
            if next.is_none() {
                session.directory = None;
            }
            HistoryResponse::Days(DayPage {
                entries,
                next,
                age_cutoff_day,
            })
        }
        HistoryRequest::Blocks {
            day,
            day_id,
            after,
            through,
        } => {
            let Ok(permit) = QUERY_SLOTS.clone().try_acquire_owned() else {
                return error(HistoryErrorCode::Busy, "历史查询繁忙，请稍后重试");
            };
            let (day, day_id, after, through) = (*day, *day_id, *after, *through);
            let history = history.clone();
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                match history.day_page(day, day_id, after, through, PACKET_PAGE_BUDGET) {
                    Ok(page) => HistoryResponse::Blocks(page_response(after, page)),
                    Err(QueryError::MissingDay) => {
                        let snapshot = history.snapshot();
                        let retention = &snapshot.retention;
                        if retention
                            .pending_deletions
                            .iter()
                            .any(|item| item.day.utc_day == day && item.day.day_id == day_id)
                        {
                            error(HistoryErrorCode::DayPendingDeletion, "此日正在清理")
                        } else if retention.age_cutoff_day.is_some_and(|cutoff| day < cutoff)
                            || retention
                                .capacity_evicted
                                .iter()
                                .any(|item| item.utc_day == day && item.day_id == day_id)
                        {
                            error(HistoryErrorCode::DayExpired, "此日已淘汰")
                        } else {
                            query_error(QueryError::MissingDay)
                        }
                    }
                    Err(err) => query_error(err),
                }
            })
            .await
            {
                Ok(response) => response,
                Err(_) => error(HistoryErrorCode::Storage, "历史读取任务失败"),
            }
        }
        HistoryRequest::Tail { cursor } => {
            let Ok(permit) = QUERY_SLOTS.clone().try_acquire_owned() else {
                return error(HistoryErrorCode::Busy, "历史查询繁忙，请稍后重试");
            };
            let offset = cursor.map_or(0, |cursor| cursor.offset);
            let cursor = cursor.map(|cursor| crate::history::TailCursor {
                wal_generation: cursor.wal_generation,
                offset: cursor.offset,
                through: cursor.through,
            });
            let history = history.clone();
            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                history
                    .tail_page(cursor, PACKET_PAGE_BUDGET)
                    .map(|page| page_response(offset, page))
            })
            .await;
            match result {
                Ok(Ok(page)) => HistoryResponse::Tail(page),
                Ok(Err(err)) => query_error(err),
                Err(_) => error(HistoryErrorCode::Storage, "历史读取任务失败"),
            }
        }
        _ => {
            super::settings::handle(
                state,
                &mut session.confirmation,
                public_key,
                history,
                request,
            )
            .await
        }
    }
}

fn page_response(offset: u64, page: Page) -> PacketPage {
    PacketPage {
        bytes: BASE64_STANDARD.encode(page.bytes),
        offset,
        next_offset: page.next_offset,
        through: page.through,
        tail_cursor: page.tail_cursor.map(|cursor| TailCursor {
            wal_generation: cursor.wal_generation,
            offset: cursor.offset,
            through: cursor.through,
        }),
    }
}

fn directory_entries(
    snapshot: &HistorySnapshot,
    range: TimeRange,
) -> impl Iterator<Item = DirectoryEntry> + '_ {
    let available = snapshot.days_intersecting(range.from, range.to).map(|day| {
        DirectoryEntry::Available(DayEntry {
            day: day.utc_day,
            day_id: day.day_id,
            published_through: day.published_through,
            data_range: day.data_range.map(Into::into),
            gap_range: day.gap_range.map(Into::into),
        })
    });
    let convert = |day: &crate::history::DayEvidence| Eviction {
        day: day.utc_day,
        day_id: day.day_id,
        data_range: day.data_range.map(Into::into),
        gap_range: day.gap_range.map(Into::into),
    };
    let intersects = move |item: &Eviction| {
        [item.data_range, item.gap_range]
            .into_iter()
            .flatten()
            .any(|coverage| range.intersects(coverage))
    };
    let pending = snapshot
        .retention
        .pending_deletions
        .iter()
        .map(move |item| convert(&item.day))
        .filter(intersects)
        .map(DirectoryEntry::Pending);
    let expired = snapshot
        .retention
        .capacity_evicted
        .iter()
        .map(convert)
        .filter(intersects)
        .map(DirectoryEntry::Evicted);
    let unavailable = snapshot
        .unavailable_days
        .iter()
        .filter(move |(day, _)| {
            let from = i64::from(**day) * DAY_MS;
            range.intersects(TimeRange {
                from,
                to: from + DAY_MS,
            })
        })
        .map(|(day, err)| DirectoryEntry::Unavailable {
            day: *day,
            error: code(err),
        });
    available.chain(pending).chain(expired).chain(unavailable)
}

pub(super) fn summary(snapshot: &HistorySnapshot) -> HistorySummary {
    let range = snapshot
        .days
        .values()
        .flat_map(|day| [day.data_range, day.gap_range])
        .flatten()
        .fold(None::<TimeRange>, |range, (from, to)| {
            Some(range.map_or(TimeRange { from, to }, |old| TimeRange {
                from: old.from.min(from),
                to: old.to.max(to),
            }))
        });
    let range = snapshot.latest_wall_ms.map_or(range, |at| {
        Some(range.map_or(
            TimeRange {
                from: at,
                to: at.saturating_add(1),
            },
            |range| TimeRange {
                from: range.from.min(at),
                to: range.to.max(at.saturating_add(1)),
            },
        ))
    });
    HistorySummary {
        store_id: snapshot.store_id,
        latest_wall_ms: snapshot.latest_wall_ms,
        range,
        latest: snapshot
            .latest
            .map(|(session_id, sample_seq)| SampleIdentity {
                session_id,
                sample_seq,
            }),
        disk_bytes: snapshot.disk_bytes,
        storage: match &snapshot.storage {
            StorageState::Starting => StorageStatus::Starting,
            StorageState::Recording => StorageStatus::Recording,
            StorageState::Disabled => StorageStatus::Disabled,
            StorageState::Unavailable(detail) if snapshot.store_id.is_some() => {
                StorageStatus::ReadOnly(detail.chars().take(300).collect())
            }
            StorageState::Unavailable(detail) => {
                StorageStatus::Error(detail.chars().take(300).collect())
            }
        },
        age_cutoff_day: snapshot.retention.age_cutoff_day,
    }
}

fn code(err: &QueryError) -> HistoryErrorCode {
    match err {
        QueryError::MissingDay => HistoryErrorCode::MissingDay,
        QueryError::DayChanged => HistoryErrorCode::DayChanged,
        QueryError::CorruptDay => HistoryErrorCode::CorruptDay,
        QueryError::UnsupportedVersion => HistoryErrorCode::UnsupportedVersion,
        QueryError::InvalidCursor => HistoryErrorCode::InvalidCursor,
        QueryError::TailRestartRequired => HistoryErrorCode::TailRestartRequired,
        QueryError::BudgetTooSmall => HistoryErrorCode::BudgetTooSmall,
        QueryError::Io => HistoryErrorCode::Storage,
    }
}

fn query_error(err: QueryError) -> HistoryResponse {
    error(code(&err), "无法读取指定历史页")
}

pub(super) fn error(code: HistoryErrorCode, detail: impl Into<String>) -> HistoryResponse {
    HistoryResponse::Error(HistoryError {
        code,
        detail: detail.into().chars().take(300).collect(),
    })
}
