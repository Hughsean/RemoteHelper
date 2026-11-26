use std::{fs, path::PathBuf};

use ::history::{
    Block, Catalog, ClockEvidence, DayHeader, Event, Frame, Gap, GapReason, Marker, MarkerKind,
    Packet, Session,
};
use base64::{Engine, prelude::BASE64_STANDARD};
use common::history_api::{DayEntry, PacketPage, StorageStatus, TailCursor};

use super::{cache::test_cache, *};

const STORE: Id = [11; 16];
const SESSION: Id = [12; 16];

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "remotehelper-client-history-{:032x}",
            rand::random::<u128>()
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn policy() -> CachePolicy {
    CachePolicy {
        retention_days: 0,
        ..CachePolicy::default()
    }
}
fn stamp(day: i32, offset: i64) -> i64 {
    i64::from(day) * DAY_MS + offset
}
fn frame(day: i32, seq: u64, offset: i64) -> Frame {
    Frame {
        sample_seq: seq,
        time_ms: stamp(day, offset),
        period_ms: 1_000,
        clock: ClockEvidence::default(),
        values: vec![None; Catalog::standard(1).channels.len()],
    }
}

fn header(day: i32, id: Id) -> (DayEntry, Vec<u8>) {
    (
        DayEntry {
            day,
            day_id: id,
            published_through: HEADER,
            data_range: None,
            gap_range: None,
        },
        DayHeader {
            utc_day: day,
            day_id: id,
        }
        .encode()
        .to_vec(),
    )
}

fn definitions(buf: &mut Vec<u8>) -> (Catalog, u64) {
    let catalog = Catalog::standard(1);
    buf.extend(Packet::Catalog(catalog.clone()).encode(None).unwrap());
    let session_ref = buf.len() as u64;
    buf.extend(
        Packet::Marker(Marker::Session(Session {
            session_id: SESSION,
            boot_id: None,
        }))
        .encode(None)
        .unwrap(),
    );
    (catalog, session_ref)
}

fn sealed(day: i32, id: Id, seq: u64) -> (DayEntry, Vec<u8>, Vec<u8>) {
    let (mut entry, mut head) = header(day, id);
    let (catalog, session_ref) = definitions(&mut head);
    let block = Packet::Block(Block {
        catalog_version: catalog.version,
        segment: 4,
        session_ref,
        frames: vec![frame(day, seq, 1000)],
    })
    .encode(Some(&catalog))
    .unwrap();
    entry.published_through = (head.len() + block.len()) as u64;
    entry.data_range = Some((stamp(day, 1000), stamp(day, 1001)).into());
    (entry, head, block)
}

fn wal(day: i32, id: Id, seqs: &[u64]) -> (TailCursor, Vec<u8>) {
    let (_, mut bytes) = header(day, id);
    let (catalog, session_ref) = definitions(&mut bytes);
    bytes.extend(
        Packet::Marker(Marker::Event(Event {
            kind: MarkerKind::Continuation,
            event_id: [99; 16],
            segment: 4,
            session_ref,
            sample_seq: None,
            clock: ClockEvidence::default(),
        }))
        .encode(None)
        .unwrap(),
    );
    for &seq in seqs {
        bytes.extend(
            Packet::Frame(frame(day, seq, seq as i64 * 1000))
                .encode(Some(&catalog))
                .unwrap(),
        );
    }
    let through = bytes.len() as u64;
    (
        TailCursor {
            wal_generation: id,
            offset: through,
            through,
        },
        bytes,
    )
}

fn page(offset: u64, through: u64, bytes: &[u8]) -> PacketPage {
    PacketPage {
        bytes: BASE64_STANDARD.encode(bytes),
        offset,
        next_offset: offset + bytes.len() as u64,
        through,
        tail_cursor: None,
    }
}

async fn put(cache: &HistoryCache, day: &DayEntry, offset: u64, bytes: &[u8]) -> HistoryResult<()> {
    cache
        .append(
            day.clone(),
            offset,
            day.published_through,
            page(offset, day.published_through, bytes),
        )
        .await
}

#[tokio::test]
async fn overview_can_read_cached_days_outside_the_detail_window() {
    let temp = Temp::new();
    let cache = test_cache(Some(temp.0.clone()), STORE, policy(), true).unwrap();
    for day in [1, 3] {
        let (entry, head, block) = sealed(day, [day as u8; 16], day as u64);
        put(&cache, &entry, 0, &head).await.unwrap();
        put(&cache, &entry, head.len() as u64, &block)
            .await
            .unwrap();
    }
    let detail = cache
        .query((stamp(3, 0), stamp(3, 10_000)).into())
        .await
        .unwrap();
    assert!(
        detail
            .samples
            .iter()
            .all(|sample| sample.frame.time_ms >= stamp(3, 0))
    );
    let old = cache
        .query_overview((stamp(1, 0), stamp(2, 0)).into())
        .await
        .unwrap();
    assert_eq!(old.samples[0].frame.time_ms, stamp(1, 1000));
    assert_eq!(old.coverage.len(), 2);
}

#[tokio::test]
async fn each_day_resumes_its_own_prefix_and_recovers_interrupted_packet() {
    let temp = Temp::new();
    let cache = test_cache(Some(temp.0.clone()), STORE, policy(), true).unwrap();
    let (old, old_head, old_block) = sealed(1, [1; 16], 1);
    let (new, new_head, new_block) = sealed(2, [2; 16], 2);
    put(&cache, &old, 0, &old_head).await.unwrap();
    put(&cache, &new, 0, &new_head).await.unwrap();
    put(&cache, &new, new_head.len() as u64, &new_block)
        .await
        .unwrap();
    assert_eq!(
        cache.offset(old.clone()).await.unwrap(),
        old_head.len() as u64
    );
    assert_eq!(
        cache.offset(new.clone()).await.unwrap(),
        new.published_through
    );
    let old_path = temp.0.join(format!("1_{}.rhh", "01".repeat(16)));
    drop(cache);
    let mut interrupted = old_head.clone();
    interrupted.extend_from_slice(&old_block[..old_block.len() / 2]);
    fs::write(&old_path, interrupted).unwrap();
    let reopened = test_cache(Some(temp.0.clone()), STORE, policy(), true).unwrap();
    assert_eq!(
        reopened.offset(old.clone()).await.unwrap(),
        old_head.len() as u64
    );
    assert_eq!(
        fs::metadata(&old_path).unwrap().len(),
        old_head.len() as u64
    );
    put(&reopened, &old, old_head.len() as u64, &old_block)
        .await
        .unwrap();
    drop(reopened);
    let offline = test_cache(Some(temp.0.clone()), STORE, policy(), false).unwrap();
    let query = offline
        .query((stamp(1, 0), stamp(3, 0)).into())
        .await
        .unwrap();
    assert_eq!(
        query
            .samples
            .iter()
            .map(|s| s.identity.sample_seq)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(fs::read(old_path).unwrap(), [old_head, old_block].concat());
}

#[tokio::test]
async fn repeated_pages_must_match_and_bad_payload_never_advances() {
    let temp = Temp::new();
    let cache = test_cache(Some(temp.0.clone()), STORE, policy(), true).unwrap();
    let (day, head, block) = sealed(1, [1; 16], 1);
    put(&cache, &day, 0, &head).await.unwrap();
    put(&cache, &day, 0, &head).await.unwrap();
    let mut bad = block.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(matches!(
        put(&cache, &day, head.len() as u64, &bad).await,
        Err(HistoryError::Format(::history::Error::Checksum))
    ));
    assert_eq!(cache.offset(day.clone()).await.unwrap(), head.len() as u64);
    put(&cache, &day, head.len() as u64, &block).await.unwrap();
    let (_, other, _) = sealed(1, [3; 16], 1);
    assert!(put(&cache, &day, 0, &other).await.is_err());
    assert_eq!(cache.offset(day).await.unwrap(), 0);
    assert!(fs::read_dir(&temp.0).unwrap().any(|e| {
        e.unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "bad")
    }));
}

#[tokio::test]
async fn newer_generation_takes_precedence_and_tail_deduplicates_at_seal() {
    let cache = test_cache(None, STORE, policy(), true).unwrap();
    let (old, old_head, old_block) = sealed(1, [1; 16], 30);
    let (new, new_head, new_block) = sealed(1, [2; 16], 1);
    put(&cache, &old, 0, &[old_head, old_block].concat())
        .await
        .unwrap();
    put(&cache, &new, 0, &[new_head, new_block].concat())
        .await
        .unwrap();
    let range: TimeRange = (stamp(1, 0), stamp(2, 0)).into();
    cache
        .directory(DirectorySnapshot {
            range,
            entries: vec![DirectoryEntry::Available(new.clone())],
            age_cutoff_day: Some(1),
        })
        .await
        .unwrap();
    let (cursor, bytes) = wal(1, [3; 16], &[1, 2]);
    cache.publish_tail(cursor, bytes).await.unwrap();
    let result = cache.query(range).await.unwrap();
    assert_eq!(result.samples.len(), 2);
    assert_eq!(result.samples[0].identity.sample_seq, 1);
    assert_eq!(
        result.samples[0].source,
        HistorySource::Sealed {
            day: 1,
            day_id: new.day_id,
            current: true
        }
    );
    assert_eq!(
        result.samples[1].source,
        HistorySource::Tail {
            generation: [3; 16]
        }
    );
    assert_eq!(result.samples[1].segment, 4);
    assert_eq!(result.coverage.len(), 2);

    cache.commit_tail(cursor).await.unwrap();
    let next = TailCursor {
        wal_generation: [4; 16],
        offset: 0,
        through: 0,
    };
    cache.publish_tail(next, Vec::new()).await.unwrap();
    assert_eq!(cache.query(range).await.unwrap().samples.len(), 2);
    cache.commit_tail(next).await.unwrap();
    let after_handoff = cache.query(range).await.unwrap();
    assert_eq!(after_handoff.samples.len(), 1);
    assert!(matches!(
        after_handoff.samples[0].source,
        HistorySource::Sealed { .. }
    ));
}

#[tokio::test]
async fn offline_manifest_and_gap_outside_owner_day_survive_server_eviction() {
    let temp = Temp::new();
    let cache = test_cache(Some(temp.0.clone()), STORE, policy(), true).unwrap();
    let (mut day, mut bytes) = header(7, [7; 16]);
    let gap = Gap {
        from_ms: stamp(1, 1000),
        to_ms: stamp(7, 0),
        reason: GapReason::ClockJump,
        segment: 4,
        event_id: [22; 16],
    };
    bytes.extend(Packet::Gap(gap).encode(None).unwrap());
    day.published_through = bytes.len() as u64;
    put(&cache, &day, 0, &bytes).await.unwrap();
    let range: TimeRange = (stamp(1, 2000), stamp(1, 3000)).into();
    cache
        .summary(HistorySummary {
            store_id: Some(STORE),
            range: None,
            latest: None,
            latest_wall_ms: None,
            disk_bytes: 0,
            storage: StorageStatus::Recording,
            age_cutoff_day: Some(9),
        })
        .await
        .unwrap();
    cache
        .directory(DirectorySnapshot {
            range,
            entries: vec![],
            age_cutoff_day: Some(9),
        })
        .await
        .unwrap();
    cache
        .status(SyncStatus::Synced { at_ms: 456 })
        .await
        .unwrap();
    drop(cache);
    let offline = test_cache(Some(temp.0.clone()), STORE, policy(), false).unwrap();
    let query = offline.query(range).await.unwrap();
    assert_eq!(query.gaps[0].gap, gap);
    assert_eq!(query.coverage[0].day, 7);
    assert!(!query.coverage[0].current);
    assert_eq!(query.directories[0].age_cutoff_day, Some(9));
    assert!(matches!(query.status, SyncStatus::Synced { at_ms: 456 }));
    assert_eq!(
        offline.extent().await.unwrap(),
        Some((gap.from_ms, gap.to_ms).into())
    );
}

#[tokio::test]
async fn older_than_local_age_is_cached_in_memory_then_cleared() {
    let temp = Temp::new();
    let cache = test_cache(Some(temp.0.clone()), STORE, CachePolicy::default(), true).unwrap();
    let (day, head, block) = sealed(1, [1; 16], 1);
    put(&cache, &day, 0, &[head, block].concat()).await.unwrap();
    let range = (stamp(1, 0), stamp(2, 0)).into();
    assert!(!cache.query(range).await.unwrap().coverage[0].durable);
    assert!(!fs::read_dir(&temp.0).unwrap().any(|e| {
        e.unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "rhh")
    }));
    cache.clear().await.unwrap();
    assert!(cache.query(range).await.unwrap().samples.is_empty());
    assert_eq!(cache.usage().await.unwrap().0, 0);
}
