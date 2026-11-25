use super::{
    RecorderOptions, Sample,
    clock::Clock,
    store::{Store, atomic_write},
};
use history::Id;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DAY_MS: i64 = 86_400_000;
const TRUST_TOLERANCE_MS: i128 = 3_000;
const LEDGER_LIMIT: u64 = 32 * 1024 * 1024;
const ACK_LIMIT: u64 = 1024;
const HEADROOM: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct RetentionSnapshot {
    pub age_cutoff_day: Option<i32>,
    pub capacity_evicted: Vec<DayEvidence>,
    pub pending_deletions: Vec<PendingDeletion>,
    pub clock_pause: Option<ClockPause>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DayEvidence {
    pub utc_day: i32,
    pub day_id: Id,
    pub data_range: Option<(i64, i64)>,
    pub gap_range: Option<(i64, i64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingDeletion {
    pub day: DayEvidence,
    pub reason: DeleteReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeleteReason {
    Age,
    Capacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ClockPause {
    pub event_id: Id,
    pub detected_wall_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Basis {
    wall_ms: i64,
    tick_ms: Option<u64>,
    boot_id: Option<Id>,
}

impl Basis {
    fn consistent_with(self, next: Self) -> Option<bool> {
        if self.boot_id.is_none() || self.boot_id != next.boot_id {
            return None;
        }
        let tick = i128::from(next.tick_ms?.checked_sub(self.tick_ms?)?);
        let wall = i128::from(next.wall_ms) - i128::from(self.wall_ms);
        Some((wall - tick).abs() <= TRUST_TOLERANCE_MS)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockLedger {
    trusted: Option<Basis>,
    paused: Option<ClockPause>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    version: u32,
    store_id: Id,
    age_cutoff_day: Option<i32>,
    capacity_evicted: Vec<DayEvidence>,
    pending_deletions: Vec<PendingDeletion>,
    pending_age_cutoff: Option<i32>,
    clock: ClockLedger,
}

impl Ledger {
    fn new(store_id: Id) -> Self {
        Self {
            version: 1,
            store_id,
            age_cutoff_day: None,
            capacity_evicted: Vec::new(),
            pending_deletions: Vec::new(),
            pending_age_cutoff: None,
            clock: ClockLedger::default(),
        }
    }

    fn valid(&self, store_id: Id) -> bool {
        self.version == 1
            && self.store_id == store_id
            && self
                .pending_deletions
                .iter()
                .filter(|item| item.reason == DeleteReason::Age)
                .all(|item| {
                    self.pending_age_cutoff
                        .is_some_and(|cutoff| item.day.utc_day < cutoff)
                })
            && self
                .capacity_evicted
                .iter()
                .chain(self.pending_deletions.iter().map(|item| &item.day))
                .all(|day| {
                    day.day_id != [0; 16]
                        && [day.data_range, day.gap_range]
                            .into_iter()
                            .flatten()
                            .all(|(from, to)| from < to)
                })
            && self
                .clock
                .paused
                .is_none_or(|pause| pause.event_id != [0; 16])
    }
}

pub(super) struct Retention {
    ledger_path: PathBuf,
    clock: Clock,
    ledger: Ledger,
    error: Option<String>,
    writable: bool,
    last_seen: Option<(Basis, u64)>,
    last_scan: Option<Instant>,
    last_options: Option<(u32, u64)>,
    last_day: Option<i32>,
    uncommitted_pause: Option<ClockPause>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ack {
    store_id: Id,
    event_id: Id,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClockAckError {
    EventChanged,
    Expired,
    ClockChanged,
    Storage,
}

impl Retention {
    pub(super) fn ack_clock(
        &mut self,
        store_id: Id,
        event_id: Id,
        displayed_wall_ms: i64,
        issued_at: Instant,
    ) -> Result<(), ClockAckError> {
        if !self.writable {
            return Err(ClockAckError::Storage);
        }
        if self.ledger.store_id != store_id
            || self.uncommitted_pause.is_some()
            || self
                .ledger
                .clock
                .paused
                .is_none_or(|pause| pause.event_id != event_id)
        {
            return Err(ClockAckError::EventChanged);
        }
        let elapsed = issued_at.elapsed();
        if elapsed >= Duration::from_secs(120) {
            return Err(ClockAckError::Expired);
        }
        let now = self.clock.read();
        let wall_ms = now.wall_ms.ok_or(ClockAckError::ClockChanged)?;
        let delta = i128::from(wall_ms) - i128::from(displayed_wall_ms);
        if (delta - elapsed.as_millis() as i128).abs() > TRUST_TOLERANCE_MS {
            return Err(ClockAckError::ClockChanged);
        }
        let basis = Basis {
            wall_ms,
            tick_ms: now.tick_ms,
            boot_id: self.clock.boot_id,
        };
        let mut next = self.ledger.clone();
        next.clock = ClockLedger {
            trusted: Some(basis),
            paused: None,
        };
        self.commit(next).map_err(|err| {
            tracing::warn!(%err, "无法持久化时钟确认，年龄清理仍暂停");
            ClockAckError::Storage
        })?;
        self.last_seen = now.tick_ms.or(now.awake_ms).map(|tick| (basis, tick));
        self.last_scan = None;
        Ok(())
    }

    pub(super) fn open(dir: &Path, store_id: Id, new_store: bool) -> io::Result<Self> {
        let path = dir.join("retention.json");
        let mut ledger = Ledger::new(store_id);
        let error = match read_json::<Ledger>(&path, LEDGER_LIMIT) {
            Ok(stored) if stored.valid(store_id) => {
                ledger = stored;
                None
            }
            Ok(_) => Some("保留台账版本、内容或存储实例不匹配".into()),
            Err(err) if err.kind() == io::ErrorKind::NotFound && new_store => {
                save(&path, &ledger).err().map(|err| err.to_string())
            }
            Err(err) => Some(err.to_string()),
        };
        if let Some(err) = &error {
            tracing::error!(%err, path = %path.display(), "历史保留台账不可用；已暂停自动删除");
        }
        let clock = Clock::new();
        if let Some(pause) = ledger.clock.paused {
            log_pause(dir, store_id, pause);
        }
        Ok(Self {
            ledger_path: path,
            clock,
            ledger,
            writable: error.is_none(),
            error,
            last_seen: None,
            last_scan: None,
            last_options: None,
            last_day: None,
            uncommitted_pause: None,
        })
    }

    pub(super) fn publish_state(&self) -> RetentionSnapshot {
        RetentionSnapshot {
            age_cutoff_day: self.ledger.age_cutoff_day,
            capacity_evicted: self.ledger.capacity_evicted.clone(),
            pending_deletions: self.ledger.pending_deletions.clone(),
            clock_pause: self.uncommitted_pause.or(self.ledger.clock.paused),
            error: self.error.clone(),
        }
    }

    pub(super) fn maintain(
        &mut self,
        store: &mut Store,
        options: &RecorderOptions,
        sample: Option<&Sample>,
    ) -> io::Result<()> {
        let result = self.maintain_inner(store, options, sample);
        if self.writable {
            self.error = result.as_ref().err().map(ToString::to_string);
        }
        result
    }

    fn maintain_inner(
        &mut self,
        store: &mut Store,
        options: &RecorderOptions,
        sample: Option<&Sample>,
    ) -> io::Result<()> {
        let pending_day = sample
            .and_then(|sample| sample.frame.clock.wall_ms)
            .and_then(utc_day);
        let limits = (options.retention_days, options.retention_bytes);
        let now = Instant::now();
        let scan = self
            .last_scan
            .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(5))
            || self.last_options != Some(limits)
            || (pending_day.is_some() && pending_day != self.last_day);
        if !scan {
            return Ok(());
        }
        self.last_scan = Some(now);
        self.last_options = Some(limits);
        self.last_day = pending_day.or(self.last_day);
        if !self.writable {
            return Ok(());
        }

        let now = self.clock.read();
        let basis = now.wall_ms.map(|wall_ms| Basis {
            wall_ms,
            tick_ms: now.tick_ms,
            boot_id: self.clock.boot_id,
        });
        let effective_day = self.update_clock(basis, now.tick_ms.or(now.awake_ms))?;
        self.finish_pending(store, self.ledger.clock.paused.is_none(), pending_day)?;
        let active = store.active_day();
        if let Some(today) = effective_day
            && options.retention_days != 0
            && self.ledger.pending_age_cutoff.is_none()
        {
            let cutoff = today.saturating_sub(options.retention_days as i32 - 1);
            let candidates = store.day_snapshots().into_iter().filter(|day| {
                day.utc_day < cutoff
                    && Some(day.utc_day) != active
                    && Some(day.utc_day) != pending_day
                    && !self.ledger.pending_deletions.iter().any(|item| {
                        item.day.utc_day == day.utc_day && item.day.day_id == day.day_id
                    })
            });
            let mut next = self.ledger.clone();
            for day in candidates {
                next.pending_deletions.push(PendingDeletion {
                    day: DayEvidence {
                        utc_day: day.utc_day,
                        day_id: day.day_id,
                        data_range: day.data_range,
                        gap_range: day.gap_range,
                    },
                    reason: DeleteReason::Age,
                });
            }
            // 被排除的活跃日期仍有数据，不能将其计入已清理范围。
            let evidence_cutoff = [active, pending_day]
                .into_iter()
                .flatten()
                .fold(cutoff, |a, b| a.min(b));
            if self
                .ledger
                .age_cutoff_day
                .is_none_or(|old| evidence_cutoff > old)
                || next.pending_deletions.len() != self.ledger.pending_deletions.len()
            {
                next.pending_age_cutoff = Some(cutoff);
            }
            if let Some(age_cutoff) = next.pending_age_cutoff {
                if next
                    .pending_deletions
                    .iter()
                    .all(|item| item.reason != DeleteReason::Age)
                {
                    next.age_cutoff_day = Some(
                        self.ledger
                            .age_cutoff_day
                            .map_or(evidence_cutoff, |old| old.max(evidence_cutoff)),
                    );
                    next.pending_age_cutoff = None;
                } else {
                    debug_assert_eq!(age_cutoff, cutoff);
                }
                let hidden: Vec<_> = next
                    .pending_deletions
                    .iter()
                    .filter(|item| item.reason == DeleteReason::Age)
                    .filter_map(|item| {
                        store
                            .hide_day(item.day.utc_day, item.day.day_id)
                            .then_some((item.day.utc_day, item.day.day_id))
                    })
                    .collect();
                if let Err(error) = self.commit(next) {
                    for (day, id) in hidden {
                        store.show_day(day, id);
                    }
                    return Err(error);
                }
                self.finish_pending(store, true, pending_day)?;
            }
        }
        if options.retention_bytes != 0 {
            let target = options.retention_bytes.saturating_sub(HEADROOM);
            let mut candidates = store.day_snapshots();
            candidates.sort_by_key(|day| day.utc_day);
            let mut used = store.space_used()?;
            for day in candidates {
                if used <= target {
                    break;
                }
                if Some(day.utc_day) == active
                    || Some(day.utc_day) == pending_day
                    || self.ledger.pending_deletions.iter().any(|item| {
                        item.day.utc_day == day.utc_day && item.day.day_id == day.day_id
                    })
                {
                    continue;
                }
                let mut next = self.ledger.clone();
                next.pending_deletions.push(PendingDeletion {
                    day: DayEvidence {
                        utc_day: day.utc_day,
                        day_id: day.day_id,
                        data_range: day.data_range,
                        gap_range: day.gap_range,
                    },
                    reason: DeleteReason::Capacity,
                });
                let hidden = store.hide_day(day.utc_day, day.day_id);
                if let Err(error) = self.commit(next) {
                    if hidden {
                        store.show_day(day.utc_day, day.day_id);
                    }
                    return Err(error);
                }
                self.finish_pending(store, false, pending_day)?;
                used = store.space_used()?;
            }
        }
        Ok(())
    }

    fn update_clock(
        &mut self,
        basis: Option<Basis>,
        elapsed_ms: Option<u64>,
    ) -> io::Result<Option<i32>> {
        let local_consistency = match (self.last_seen, basis, elapsed_ms) {
            (Some((last, tick)), Some(now), Some(next_tick)) => {
                next_tick.checked_sub(tick).map(|delta| {
                    let wall = i128::from(now.wall_ms) - i128::from(last.wall_ms);
                    (wall - i128::from(delta)).abs() <= TRUST_TOLERANCE_MS
                })
            }
            _ => None,
        };
        let mut next = self.ledger.clone();
        let mut changed = false;
        if let Some(pause) = self.uncommitted_pause {
            next.clock.paused = Some(pause);
            self.commit(next.clone())?;
            self.uncommitted_pause = None;
        }
        if let Some(pause) = next.clock.paused {
            let auto = next
                .clock
                .trusted
                .zip(basis)
                .and_then(|(last, now)| last.consistent_with(now))
                == Some(true);
            let ack_path = self.ledger_path.with_file_name("clock.ack");
            if !auto
                && (local_consistency == Some(false)
                    || (basis.is_none() && self.last_seen.is_some()))
            {
                let pause = ClockPause {
                    event_id: rand::random(),
                    detected_wall_ms: basis.map(|basis| basis.wall_ms),
                };
                log_pause(
                    self.ledger_path.parent().unwrap_or(Path::new(".")),
                    next.store_id,
                    pause,
                );
                self.uncommitted_pause = Some(pause);
                next.clock.paused = Some(pause);
                self.commit(next)?;
                self.uncommitted_pause = None;
                self.last_seen = basis.zip(elapsed_ms);
                return Ok(None);
            }
            let ack = read_json::<Ack>(&ack_path, ACK_LIMIT)
                .ok()
                .is_some_and(|ack| ack.store_id == next.store_id && ack.event_id == pause.event_id);
            if let Some(now) = basis
                && (auto || ack)
            {
                next.clock = ClockLedger {
                    trusted: Some(now),
                    paused: None,
                };
                self.commit(next.clone())?;
                if ack && let Err(err) = fs::remove_file(&ack_path) {
                    tracing::warn!(%err, "已恢复年龄清理，但无法移除本机确认文件");
                }
                tracing::info!("服务器时钟已确认，恢复历史年龄清理");
            }
        } else {
            let trusted_consistency = next
                .clock
                .trusted
                .zip(basis)
                .and_then(|(last, now)| last.consistent_with(now));
            if basis.is_none()
                || local_consistency == Some(false)
                || trusted_consistency == Some(false)
            {
                let pause = ClockPause {
                    event_id: rand::random(),
                    detected_wall_ms: basis.map(|b| b.wall_ms),
                };
                log_pause(
                    self.ledger_path.parent().unwrap_or(Path::new(".")),
                    next.store_id,
                    pause,
                );
                self.uncommitted_pause = Some(pause);
                next.clock.paused = Some(pause);
                self.commit(next.clone())?;
                self.uncommitted_pause = None;
            } else if let Some(now) = basis
                && (next.clock.trusted.is_none_or(|last| {
                    last.boot_id != now.boot_id
                        || i128::from(now.wall_ms) - i128::from(last.wall_ms) >= 30_000
                }))
            {
                next.clock.trusted = Some(now);
                changed = true;
            }
        }
        if changed {
            self.commit(next)?;
        }
        self.last_seen = basis.zip(elapsed_ms);
        if self.ledger.clock.paused.is_some() {
            return Ok(None);
        }
        Ok(basis.and_then(|basis| utc_day(basis.wall_ms)))
    }

    fn finish_pending(
        &mut self,
        store: &mut Store,
        allow_age: bool,
        pending_day: Option<i32>,
    ) -> io::Result<()> {
        for item in self.ledger.pending_deletions.clone() {
            let day = &item.day;
            if store.active_day() == Some(day.utc_day) || pending_day == Some(day.utc_day) {
                continue;
            }
            store.hide_day(day.utc_day, day.day_id);
            if item.reason == DeleteReason::Age && !allow_age {
                continue;
            }
            store.delete_day(day.utc_day, day.day_id)?;
            let mut next = self.ledger.clone();
            if item.reason == DeleteReason::Capacity && !next.capacity_evicted.contains(day) {
                next.capacity_evicted.push(day.clone());
            }
            next.pending_deletions.retain(|pending| pending != &item);
            self.commit(next)?;
        }
        if allow_age
            && self
                .ledger
                .pending_deletions
                .iter()
                .all(|item| item.reason != DeleteReason::Age)
            && let Some(cutoff) = self.ledger.pending_age_cutoff
        {
            let cutoff = store
                .active_day()
                .map_or(cutoff, |active| active.min(cutoff));
            let mut next = self.ledger.clone();
            next.pending_age_cutoff = None;
            next.age_cutoff_day = Some(next.age_cutoff_day.map_or(cutoff, |old| old.max(cutoff)));
            self.commit(next)?;
        }
        Ok(())
    }

    fn commit(&mut self, next: Ledger) -> io::Result<()> {
        save(&self.ledger_path, &next)?;
        self.ledger = next;
        Ok(())
    }
}

fn utc_day(wall_ms: i64) -> Option<i32> {
    i32::try_from(wall_ms.div_euclid(DAY_MS)).ok()
}

fn log_pause(dir: &Path, store_id: Id, pause: ClockPause) {
    tracing::warn!(path = %dir.join("clock.ack").display(), store_id = ?store_id, event_id = ?pause.event_id,
        detected_wall_ms = ?pause.detected_wall_ms, "已暂停历史年龄清理；核对 UTC 后可用这两个标识写入本机确认 JSON");
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> io::Result<T> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSON 文件超出大小限制",
        ));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn save(path: &Path, value: &Ledger) -> io::Result<()> {
    let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    if bytes.len() as u64 > LEDGER_LIMIT {
        return Err(io::Error::other("历史保留台账超出大小限制"));
    }
    atomic_write(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn open() -> Self {
            let path = std::env::temp_dir().join(format!(
                "remotehelper-retention-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn pause_survives_restart_and_stale_confirmation_cannot_clear_it() {
        let dir = TestDir::open();
        let id = [7; 16];
        let first = Basis {
            wall_ms: 100_000,
            tick_ms: Some(10_000),
            boot_id: Some([1; 16]),
        };
        let second = Basis {
            wall_ms: 180_000,
            tick_ms: Some(15_000),
            ..first
        };
        let third = Basis {
            wall_ms: 220_000,
            tick_ms: Some(20_000),
            ..first
        };
        let mut retention = Retention::open(&dir.0, id, true).unwrap();
        assert!(
            retention
                .update_clock(Some(first), first.tick_ms)
                .unwrap()
                .is_some()
        );
        assert_eq!(
            retention
                .update_clock(Some(second), second.tick_ms)
                .unwrap(),
            None
        );
        let paused = retention.publish_state().clock_pause.unwrap();
        drop(retention);
        let mut retention = Retention::open(&dir.0, id, false).unwrap();
        assert_eq!(retention.publish_state().clock_pause, Some(paused));
        assert_eq!(
            retention
                .update_clock(Some(second), second.tick_ms)
                .unwrap(),
            None
        );
        let ack = dir.0.join("clock.ack");
        fs::write(
            &ack,
            serde_json::to_vec(&serde_json::json!({"store_id": id, "event_id": paused.event_id}))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            retention.update_clock(Some(third), third.tick_ms).unwrap(),
            None
        );
        let current = retention.publish_state().clock_pause.unwrap();
        assert_ne!(paused.event_id, current.event_id);
        assert_eq!(
            retention.ack_clock(id, paused.event_id, third.wall_ms, Instant::now()),
            Err(ClockAckError::EventChanged)
        );
        fs::write(
            &ack,
            serde_json::to_vec(&serde_json::json!({"store_id": id, "event_id": current.event_id}))
                .unwrap(),
        )
        .unwrap();
        assert!(
            retention
                .update_clock(Some(third), third.tick_ms)
                .unwrap()
                .is_some()
        );
        assert!(!ack.exists());
        assert!(
            Retention::open(&dir.0, id, false)
                .unwrap()
                .publish_state()
                .clock_pause
                .is_none()
        );
    }

    #[test]
    fn completed_capacity_eviction_remains_visible_after_restart() {
        use crate::history::store::ReadState;
        use std::sync::{Arc, RwLock};
        let dir = TestDir::open();
        let read = Arc::new(RwLock::new(ReadState::new(dir.0.clone())));
        let mut store = Store::open(dir.0.clone(), read).unwrap();
        let mut retention = Retention::open(&dir.0, store.store_id(), true).unwrap();
        let options = RecorderOptions {
            enabled: true,
            seal_interval: Duration::from_secs(60),
            retention_days: 0,
            retention_bytes: 0,
            compression_level: 3,
        };
        for (seq, time_ms) in [(0, 1000), (1, DAY_MS + 1000)] {
            use history::{Catalog, ClockEvidence, Frame, Session};
            let sample = Sample {
                session: Session {
                    session_id: [1; 16],
                    boot_id: None,
                },
                frame: Frame {
                    sample_seq: seq,
                    time_ms,
                    period_ms: 1000,
                    clock: ClockEvidence {
                        wall_ms: Some(time_ms),
                        tick_ms: Some(time_ms as u64),
                        awake_ms: Some(time_ms as u64),
                    },
                    values: vec![None; Catalog::standard(1).channels.len()],
                },
            };
            store.record(&sample, &[], &options).unwrap();
        }
        let capacity = RecorderOptions {
            retention_bytes: 1,
            ..options
        };
        retention.maintain(&mut store, &capacity, None).unwrap();
        let snapshot = retention.publish_state();
        assert_eq!(snapshot.capacity_evicted.len(), 1);
        assert_eq!(snapshot.capacity_evicted[0].utc_day, 0);
        assert!(snapshot.pending_deletions.is_empty());
        assert_eq!(store.active_day(), Some(1));
        assert!(!dir.0.join("1970-01-01.rhh").exists());
        assert_eq!(
            Retention::open(&dir.0, store.store_id(), false)
                .unwrap()
                .publish_state()
                .capacity_evicted,
            snapshot.capacity_evicted
        );
    }

    #[test]
    fn corrupt_ledger_is_preserved_and_no_evictions_are_claimed() {
        let dir = TestDir::open();
        let path = dir.0.join("retention.json");
        fs::write(&path, b"unfinished").unwrap();
        let retention = Retention::open(&dir.0, [7; 16], false).unwrap();
        assert!(!retention.writable);
        assert!(retention.publish_state().error.is_some());
        assert_eq!(retention.publish_state().age_cutoff_day, None);
        assert_eq!(fs::read(path).unwrap(), b"unfinished");
    }

    #[test]
    fn only_same_boot_can_automatically_confirm_trusted_wall() {
        let last = Basis {
            wall_ms: 100_000,
            tick_ms: Some(20_000),
            boot_id: Some([1; 16]),
        };
        let next = Basis {
            wall_ms: 105_100,
            tick_ms: Some(25_000),
            boot_id: Some([1; 16]),
        };
        assert_eq!(last.consistent_with(next), Some(true));
        assert_eq!(
            last.consistent_with(Basis {
                wall_ms: 110_000,
                ..next
            }),
            Some(false)
        );
        assert_eq!(
            last.consistent_with(Basis {
                boot_id: Some([2; 16]),
                ..next
            }),
            None
        );
        assert_eq!(
            last.consistent_with(Basis {
                tick_ms: Some(3),
                ..next
            }),
            None
        );
    }
}
