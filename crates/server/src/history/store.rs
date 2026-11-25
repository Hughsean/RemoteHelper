use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use ::history::{
    Block, Catalog, DAY_HEADER_LEN, DayHeader, Error, Event, FileContext, FileKind, Frame, Gap,
    GapReason, Id, MAX_BLOCK_POINTS, Marker, MarkerKind, PUBLISH_BOUNDARY_LEN, Packet,
    PublishBoundary, Session, decode_packet, packet_len,
};
use time::{Date, Duration as Days, Month};

use super::{Loss, RecorderOptions, RetentionSnapshot, Sample, clock};

const HEADER: u64 = DAY_HEADER_LEN as u64;
const DAY_MS: i64 = 86_400_000;
const WAL_BLOCK_LIMIT: usize = 112 * 1024;
const PAGE_LIMIT: usize = 512 * 1024;
const MAX_PENDING_WAL: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum StorageState {
    Starting,
    Recording,
    Disabled,
    Unavailable(String),
}

#[derive(Clone, Debug)]
pub(crate) struct DaySnapshot {
    pub utc_day: i32,
    pub day_id: Id,
    pub published_through: u64,
    pub data_range: Option<(i64, i64)>,
    pub gap_range: Option<(i64, i64)>,
    pub blocks: Arc<Vec<BlockIndex>>,
    pub gaps: Arc<Vec<GapIndex>>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // 保留包偏移和段号，供区间索引使用。
pub(crate) struct BlockIndex {
    pub offset: u64,
    pub through: u64,
    pub from_ms: i64,
    pub to_ms: i64,
    pub segment: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct GapIndex {
    #[allow(dead_code)] // 保留缺口包偏移，供区间索引使用。
    pub offset: u64,
    pub from_ms: i64,
    pub to_ms: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct HistorySnapshot {
    pub store_id: Option<Id>,
    pub days: Arc<BTreeMap<i32, DaySnapshot>>,
    pub unavailable_days: Arc<BTreeMap<i32, QueryError>>,
    pub disk_bytes: u64,
    pub latest: Option<(Id, u64)>,
    pub latest_wall_ms: Option<i64>,
    pub storage: StorageState,
    pub retention: RetentionSnapshot,
}

impl HistorySnapshot {
    /// 同时按实际缺口区间和样本所属 UTC 日匹配。
    pub fn days_intersecting(&self, from: i64, to: i64) -> impl Iterator<Item = &DaySnapshot> {
        self.days.values().filter(move |day| {
            from < to
                && (day
                    .blocks
                    .iter()
                    .any(|block| from < block.to_ms && block.from_ms < to)
                    || day
                        .gaps
                        .iter()
                        .any(|gap| from < gap.to_ms && gap.from_ms < to))
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TailCursor {
    pub wal_generation: Id,
    pub offset: u64,
    pub through: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QueryError {
    MissingDay,
    DayChanged,
    CorruptDay,
    UnsupportedVersion,
    InvalidCursor,
    TailRestartRequired,
    BudgetTooSmall,
    Io,
}

#[derive(Debug)]
pub(crate) struct Page {
    /// 从偏移 0 开始时包含文件头，其后仅有完整的数据包。
    pub bytes: Vec<u8>,
    pub next_offset: u64,
    pub through: u64,
    /// 尾部分页游标携带首次读取时固定的上界。
    pub tail_cursor: Option<TailCursor>,
}

pub(super) struct ReadState {
    dir: PathBuf,
    snapshot: Arc<HistorySnapshot>,
    day_boundaries: BTreeMap<i32, Arc<BTreeSet<u64>>>,
    tail: Tail,
}

struct Tail {
    generation: Id,
    bytes: Vec<u8>,
    boundaries: BTreeSet<u64>,
}

impl Tail {
    fn empty() -> Self {
        Self {
            generation: rand::random(),
            bytes: Vec::new(),
            boundaries: BTreeSet::from([0]),
        }
    }

    fn append(&mut self, bytes: &[u8], ends: impl IntoIterator<Item = u64>) {
        self.bytes.extend_from_slice(bytes);
        self.boundaries.extend(ends);
    }
}

impl ReadState {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            snapshot: Arc::new(HistorySnapshot {
                store_id: None,
                days: Arc::default(),
                unavailable_days: Arc::default(),
                disk_bytes: 0,
                latest: None,
                latest_wall_ms: None,
                storage: StorageState::Starting,
                retention: RetentionSnapshot::default(),
            }),
            day_boundaries: BTreeMap::new(),
            tail: Tail::empty(),
        }
    }

    pub fn snapshot(&self) -> Arc<HistorySnapshot> {
        self.snapshot.clone()
    }

    pub fn set_storage(&mut self, state: StorageState) {
        Arc::make_mut(&mut self.snapshot).storage = state;
    }

    pub fn set_retention(&mut self, state: RetentionSnapshot) {
        Arc::make_mut(&mut self.snapshot).retention = state;
    }

    pub fn day_page(
        &self,
        day: i32,
        day_id: Id,
        after: u64,
        through: u64,
        budget: usize,
    ) -> Result<Page, QueryError> {
        if let Some(status) = self.snapshot.unavailable_days.get(&day) {
            return Err(status.clone());
        }
        let meta = self.snapshot.days.get(&day).ok_or(QueryError::MissingDay)?;
        if meta.day_id != day_id {
            return Err(QueryError::DayChanged);
        }
        let (next_offset, size) = page_range(
            self.day_boundaries
                .get(&day)
                .ok_or(QueryError::MissingDay)?,
            after,
            through,
            meta.published_through,
            budget,
        )?;
        let mut file = File::open(day_path(&self.dir, day, "rhh").map_err(|_| QueryError::Io)?)
            .map_err(|_| QueryError::Io)?;
        let header = read_header(&mut file).map_err(|e| query_error(&e))?;
        if header.day_id != day_id || header.utc_day != day {
            return Err(QueryError::DayChanged);
        }
        let mut bytes = vec![0; size];
        file.seek(SeekFrom::Start(after))
            .and_then(|_| file.read_exact(&mut bytes))
            .map_err(|error| query_error(&error))?;
        let mut remaining = if after == 0 && !bytes.is_empty() {
            &bytes[DAY_HEADER_LEN..]
        } else {
            &bytes[..]
        };
        while !remaining.is_empty() {
            let (_, count) =
                decode_packet(remaining).map_err(|error| query_error(&format_error(error)))?;
            remaining = &remaining[count..];
        }
        Ok(Page {
            bytes,
            next_offset,
            through,
            tail_cursor: None,
        })
    }

    pub fn tail_page(&self, cursor: Option<TailCursor>, budget: usize) -> Result<Page, QueryError> {
        let tail = &self.tail;
        let len = tail.bytes.len() as u64;
        let cursor = cursor.unwrap_or(TailCursor {
            wal_generation: tail.generation,
            offset: 0,
            through: len,
        });
        if cursor.wal_generation != tail.generation {
            return Err(QueryError::TailRestartRequired);
        }
        let (next_offset, _) =
            page_range(&tail.boundaries, cursor.offset, cursor.through, len, budget)?;
        Ok(Page {
            bytes: tail.bytes[cursor.offset as usize..next_offset as usize].to_vec(),
            next_offset,
            through: cursor.through,
            tail_cursor: Some(TailCursor {
                offset: next_offset,
                ..cursor
            }),
        })
    }
}

fn page_range(
    boundaries: &BTreeSet<u64>,
    after: u64,
    through: u64,
    published: u64,
    budget: usize,
) -> Result<(u64, usize), QueryError> {
    if after > through
        || through > published
        || !boundaries.contains(&after)
        || !boundaries.contains(&through)
    {
        return Err(QueryError::InvalidCursor);
    }
    let high = through.min(after.saturating_add(budget.min(PAGE_LIMIT) as u64));
    let end = *boundaries.range(after..=high).next_back().unwrap();
    if end == after && end < through {
        return Err(QueryError::BudgetTooSmall);
    }
    Ok((end, (end - after) as usize))
}

#[derive(Clone)]
pub(super) struct Activity {
    pub sample: Sample,
    segment: u64,
    state: Option<MarkerKind>,
}

impl Activity {
    fn key(&self) -> (u64, u64) {
        (self.segment, self.sample.frame.sample_seq)
    }
}

struct Day {
    snapshot: DaySnapshot,
    boundaries: Arc<BTreeSet<u64>>,
    sessions: HashMap<Id, (u64, Session)>,
    session_offsets: HashMap<u64, Session>,
    catalogs: HashMap<u32, Catalog>,
    samples: HashMap<Id, Ranges>,
    events: HashSet<Id>,
}

/// 通常每天每会话只有一个连续序号区间；缺口和重新打开日文件会分割区间。
#[derive(Default)]
struct Ranges(BTreeMap<u64, u64>);

impl Ranges {
    fn contains(&self, seq: u64) -> bool {
        self.0
            .range(..=seq)
            .next_back()
            .is_some_and(|(_, end)| seq <= *end)
    }

    fn insert(&mut self, seq: u64) {
        let mut from = seq;
        let mut to = seq;
        if let Some((&a, &b)) = self.0.range(..=seq).next_back() {
            if seq <= b {
                return;
            }
            if seq == b.saturating_add(1) {
                from = a;
                self.0.remove(&a);
            }
        }
        if let Some((&a, &b)) = self.0.range(seq..).next() {
            if a == seq.saturating_add(1) {
                to = b;
                self.0.remove(&a);
            }
        }
        self.0.insert(from, to);
    }
}

impl Day {
    fn new(header: &DayHeader) -> Self {
        Self {
            snapshot: DaySnapshot {
                utc_day: header.utc_day,
                day_id: header.day_id,
                published_through: HEADER,
                data_range: None,
                gap_range: None,
                blocks: Arc::default(),
                gaps: Arc::default(),
            },
            boundaries: Arc::new(BTreeSet::from([0, HEADER])),
            sessions: HashMap::new(),
            session_offsets: HashMap::new(),
            catalogs: HashMap::new(),
            samples: HashMap::new(),
            events: HashSet::new(),
        }
    }

    fn contains(&self, session: Id, seq: u64) -> bool {
        self.samples
            .get(&session)
            .is_some_and(|ranges| ranges.contains(seq))
    }

    fn index(
        &mut self,
        packet: &Packet,
        offset: u64,
        end: u64,
        latest: &mut Option<Activity>,
    ) -> io::Result<()> {
        let day = self.snapshot.utc_day;
        match packet {
            Packet::Catalog(catalog) => {
                self.catalogs.insert(catalog.version, catalog.clone());
            }
            Packet::Marker(Marker::Session(session)) => {
                self.session_offsets.insert(offset, *session);
                if let Some((_, old)) = self.sessions.insert(session.session_id, (offset, *session))
                {
                    if old.boot_id != session.boot_id {
                        return Err(invalid("conflicting session definitions"));
                    }
                }
            }
            Packet::Marker(Marker::Event(event)) => {
                let session = self.session_at(event.session_ref)?;
                if !self.events.insert(event.event_id) {
                    return Err(invalid("duplicate published event"));
                }
                if let (Some(seq), Some(wall)) = (event.sample_seq, event.clock.wall_ms) {
                    if let Some(p) = latest.as_mut().filter(|p| {
                        p.key() == (event.segment, seq)
                            && p.sample.session.session_id == session.session_id
                    }) {
                        p.state = Some(event.kind);
                    } else if matches!(
                        event.kind,
                        MarkerKind::Exit | MarkerKind::Disabled | MarkerKind::Resumed
                    ) {
                        update_latest(
                            latest,
                            Activity {
                                sample: Sample {
                                    session,
                                    frame: Frame {
                                        sample_seq: seq,
                                        time_ms: wall,
                                        period_ms: 0,
                                        clock: event.clock,
                                        values: Vec::new(),
                                    },
                                },
                                segment: event.segment,
                                state: Some(event.kind),
                            },
                        );
                    }
                }
            }
            Packet::Gap(gap) => {
                if !self.events.insert(gap.event_id) {
                    return Err(invalid("duplicate published GAP"));
                }
                union(&mut self.snapshot.gap_range, gap.from_ms, gap.to_ms);
                Arc::make_mut(&mut self.snapshot.gaps).push(GapIndex {
                    offset,
                    from_ms: gap.from_ms,
                    to_ms: gap.to_ms,
                });
            }
            Packet::Block(block) => {
                let session = self.session_at(block.session_ref)?;
                let ranges = self.samples.entry(session.session_id).or_default();
                let mut from = i64::MAX;
                let mut to = i64::MIN;
                for frame in &block.frames {
                    if utc_day(frame.time_ms) != Some(day) || ranges.contains(frame.sample_seq) {
                        return Err(invalid("invalid or duplicate published sample"));
                    }
                    ranges.insert(frame.sample_seq);
                    from = from.min(frame.time_ms);
                    to = to.max(frame.time_ms.saturating_add(1));
                }
                union(&mut self.snapshot.data_range, from, to);
                Arc::make_mut(&mut self.snapshot.blocks).push(BlockIndex {
                    offset,
                    through: end,
                    from_ms: from,
                    to_ms: to,
                    segment: block.segment,
                });
                if let Some(frame) = block.frames.last() {
                    update_latest(
                        latest,
                        Activity {
                            sample: Sample {
                                session,
                                frame: frame.clone(),
                            },
                            segment: block.segment,
                            state: None,
                        },
                    );
                }
            }
            Packet::Roll(_) => {}
            Packet::Frame(_) => return Err(invalid("FRAME in published file")),
        }
        self.snapshot.published_through = end;
        Arc::make_mut(&mut self.boundaries).insert(end);
        Ok(())
    }

    fn session_at(&self, offset: u64) -> io::Result<Session> {
        self.session_offsets
            .get(&offset)
            .copied()
            .ok_or_else(|| invalid("missing published session"))
    }
}

fn union(range: &mut Option<(i64, i64)>, from: i64, to: i64) {
    *range = Some(range.map_or((from, to), |(a, b)| (a.min(from), b.max(to))));
}

fn update_latest(latest: &mut Option<Activity>, activity: Activity) {
    if latest
        .as_ref()
        .is_none_or(|previous| activity.key() >= previous.key())
    {
        *latest = Some(activity);
    }
}

#[derive(Clone)]
enum PendingRecord {
    Frame {
        frame: Frame,
        session: Session,
        catalog: Catalog,
        segment: u64,
    },
    Event {
        event: Event,
        session: Session,
    },
    Gap(Gap),
}

struct Wal {
    header: DayHeader,
    file: File,
    length: u64,
    started: Option<Instant>,
    records: Vec<PendingRecord>,
    points: usize,
    synced_records: usize,
    active_session: Option<(u64, Session)>,
    active_catalog: Option<u32>,
}

pub(super) struct Store {
    dir: PathBuf,
    read: Arc<RwLock<ReadState>>,
    _lock: File,
    store_id: Id,
    new_store: bool,
    days: BTreeMap<i32, Day>,
    bad_days: BTreeMap<i32, QueryError>,
    bad_wals: BTreeSet<i32>,
    hidden: BTreeSet<i32>,
    wal: Option<Wal>,
    latest: Option<Activity>,
    segment: u64,
    disk_bytes: u64,
}

impl Store {
    pub fn open(dir: PathBuf, read: Arc<RwLock<ReadState>>) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("writer.lock"))?;
        lock.try_lock().map_err(io::Error::from)?;
        let (store_id, new_store) = match read_store_id(&dir) {
            Ok(id) => (id, false),
            Err(e)
                if e.kind() == io::ErrorKind::NotFound
                    && list_days(&dir)?.is_empty()
                    && !dir.join("retention.json").try_exists()? =>
            {
                let id: Id = rand::random();
                atomic_write(&dir.join("store_id"), &id)?;
                (id, true)
            }
            Err(e) => return Err(e),
        };
        let mut store = Self {
            disk_bytes: directory_bytes(&dir)?,
            dir,
            read,
            _lock: lock,
            store_id,
            new_store,
            days: BTreeMap::new(),
            bad_days: BTreeMap::new(),
            bad_wals: BTreeSet::new(),
            hidden: BTreeSet::new(),
            wal: None,
            latest: None,
            segment: 0,
        };
        let candidates = list_days(&store.dir)?;
        for (&day, &files) in &candidates {
            match scan_day(&store.dir, day, files) {
                Ok((meta, latest)) => {
                    if let Some(latest) = latest {
                        update_latest(&mut store.latest, latest);
                    }
                    store.days.insert(day, meta);
                }
                Err(error) => {
                    tracing::warn!(day, %error, "published history day unavailable");
                    store.bad_days.insert(day, query_error(&error));
                }
            }
        }
        store.segment = store.latest.as_ref().map_or(0, |a| a.segment);
        // 长缺口的归属日可能不在缺口范围内；先建立所有日的索引，
        // 再对照索引回放仍保留的 WAL。
        for (&day, &files) in &candidates {
            if !store.days.contains_key(&day) {
                continue;
            }
            match store.recover_day(day, files) {
                Ok(()) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof
                    ) =>
                {
                    tracing::warn!(day, %error, "history WAL unavailable; retained for inspection");
                    store.bad_wals.insert(day);
                    store.wal = None;
                }
                Err(error) => return Err(error),
            }
        }
        store.disk_bytes = store.space_used()?;
        {
            let mut read = store.read.write().unwrap();
            read.tail = Tail::empty();
            store.install_snapshot(&mut read);
        }
        Ok(store)
    }

    pub fn inspect_read_only(dir: &Path, read: &RwLock<ReadState>) {
        let (Ok(id), Ok(files)) = (read_store_id(dir), list_days(dir)) else {
            return;
        };
        let mut days = BTreeMap::new();
        let mut boundaries = BTreeMap::new();
        let mut unavailable = BTreeMap::new();
        let mut latest = None;
        for (day, files) in files {
            match scan_day(dir, day, files) {
                Ok((meta, activity)) => {
                    days.insert(day, meta.snapshot);
                    boundaries.insert(day, meta.boundaries);
                    if let Some(activity) = activity {
                        update_latest(&mut latest, activity);
                    }
                }
                Err(error) => {
                    unavailable.insert(day, query_error(&error));
                }
            }
        }
        let mut read = read.write().unwrap();
        read.day_boundaries = boundaries;
        read.tail = Tail::empty();
        let snap = Arc::make_mut(&mut read.snapshot);
        snap.store_id = Some(id);
        snap.days = Arc::new(days);
        snap.unavailable_days = Arc::new(unavailable);
        snap.disk_bytes = directory_bytes(dir).unwrap_or(0);
        snap.latest_wall_ms = latest.as_ref().map(|a| a.sample.frame.time_ms);
        snap.latest = latest.map(|a| (a.sample.session.session_id, a.sample.frame.sample_seq));
    }

    pub fn store_id(&self) -> Id {
        self.store_id
    }
    pub fn new_store(&self) -> bool {
        self.new_store
    }
    pub fn active_day(&self) -> Option<i32> {
        self.wal.as_ref().map(|w| w.header.utc_day)
    }
    pub fn latest(&self) -> Option<&Activity> {
        self.latest.as_ref()
    }
    pub fn space_used(&self) -> io::Result<u64> {
        directory_bytes(&self.dir)
    }
    pub fn near_capacity(&self, limit: u64) -> bool {
        limit != 0 && self.disk_bytes.saturating_add(1024 * 1024) >= limit
    }

    pub fn day_snapshots(&self) -> Vec<DaySnapshot> {
        self.days
            .iter()
            .filter(|(day, _)| !self.bad_wals.contains(day))
            .map(|(_, d)| d.snapshot.clone())
            .collect()
    }

    pub fn hide_day(&mut self, day: i32, day_id: Id) -> bool {
        if self.active_day() == Some(day)
            || self
                .days
                .get(&day)
                .is_none_or(|d| d.snapshot.day_id != day_id)
        {
            return false;
        }
        self.hidden.insert(day);
        self.refresh_snapshot();
        true
    }

    pub fn show_day(&mut self, day: i32, day_id: Id) {
        if self
            .days
            .get(&day)
            .is_some_and(|d| d.snapshot.day_id == day_id)
        {
            self.hidden.remove(&day);
            self.refresh_snapshot();
        }
    }

    pub fn delete_day(&mut self, day: i32, day_id: Id) -> io::Result<()> {
        if self.active_day() == Some(day) || self.bad_wals.contains(&day) {
            return Err(io::Error::other(
                "cannot delete active or unrecovered history day",
            ));
        }
        if self
            .days
            .get(&day)
            .is_some_and(|d| d.snapshot.day_id != day_id)
        {
            return Ok(());
        }
        // 重启时待结算的清理事务可能已删掉部分文件；
        // 逐个检查剩余文件的日代号后再删除。
        for ext in ["rhh", "wal", "pub"] {
            let path = day_path(&self.dir, day, ext)?;
            let id = match File::open(&path) {
                Ok(mut f) if ext == "pub" => read_boundary_from(&mut f)?.day_id,
                Ok(mut f) => read_header(&mut f)?.day_id,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if id == day_id {
                fs::remove_file(path)?;
            }
        }
        sync_dir(&self.dir)?;
        self.days.remove(&day);
        self.bad_days.remove(&day);
        self.hidden.remove(&day);
        self.disk_bytes = self.space_used()?;
        self.refresh_snapshot();
        Ok(())
    }

    pub fn refresh_snapshot(&self) {
        self.install_snapshot(&mut self.read.write().unwrap());
    }

    fn install_snapshot(&self, read: &mut ReadState) {
        let snap = Arc::make_mut(&mut read.snapshot);
        snap.store_id = Some(self.store_id);
        snap.disk_bytes = self.disk_bytes;
        snap.latest = self
            .latest
            .as_ref()
            .map(|a| (a.sample.session.session_id, a.sample.frame.sample_seq));
        snap.latest_wall_ms = self.latest.as_ref().map(|a| a.sample.frame.time_ms);
        snap.days = Arc::new(
            self.days
                .iter()
                .filter(|(d, _)| !self.hidden.contains(d) && !self.bad_days.contains_key(d))
                .map(|(&d, m)| (d, m.snapshot.clone()))
                .collect(),
        );
        snap.unavailable_days = Arc::new(self.bad_days.clone());
        read.day_boundaries = self
            .days
            .iter()
            .filter(|(d, _)| !self.hidden.contains(d))
            .map(|(&d, m)| (d, m.boundaries.clone()))
            .collect();
    }

    fn recover_day(&mut self, day: i32, files: u8) -> io::Result<()> {
        let meta = &self.days[&day];
        let header = DayHeader {
            utc_day: day,
            day_id: meta.snapshot.day_id,
        };
        let rhh = day_path(&self.dir, day, "rhh")?;
        let file = OpenOptions::new().write(true).open(rhh)?;
        if file.metadata()?.len() > meta.snapshot.published_through {
            file.set_len(meta.snapshot.published_through)?;
            file.sync_all()?;
        }
        // 只有文件头且没有附属文件，说明新日初始化可能被中断；
        // 写入器在发布初始边界后才会创建 WAL，因此可以恢复。
        if files & PUB == 0 {
            self.write_boundary(&header, HEADER)?;
        }
        if files & WAL == 0 {
            return Ok(());
        }
        let mut wal = read_wal(&self.dir, header, meta, self.segment)?;
        if wal.records.is_empty() {
            let header = wal.header;
            if wal.length > HEADER {
                drop(wal.file);
                self.replace_wal(&header)?;
            }
            return Ok(());
        }
        let prior = self.latest.clone();
        let prior_segment = self.segment;
        // 按 WAL 中的顺序读取标记和样本证据；
        // 跨日顺序由段号和段内的样本序号确定，不依赖文件名。
        for entry in &wal.records {
            match entry {
                PendingRecord::Frame {
                    frame,
                    session,
                    segment,
                    ..
                } => {
                    self.segment = self.segment.max(*segment);
                    update_latest(
                        &mut self.latest,
                        Activity {
                            sample: Sample {
                                session: *session,
                                frame: frame.clone(),
                            },
                            segment: *segment,
                            state: None,
                        },
                    );
                }
                PendingRecord::Event { event, session } => {
                    self.segment = self.segment.max(event.segment);
                    if let Some(p) = self.latest.as_mut().filter(|p| {
                        p.key() == (event.segment, event.sample_seq.unwrap_or(u64::MAX))
                            && p.sample.session.session_id == session.session_id
                    }) {
                        p.state = Some(event.kind);
                    } else if let (Some(seq), Some(time_ms)) =
                        (event.sample_seq, event.clock.wall_ms)
                        && matches!(
                            event.kind,
                            MarkerKind::Disabled | MarkerKind::Resumed | MarkerKind::Exit
                        )
                    {
                        update_latest(
                            &mut self.latest,
                            Activity {
                                sample: Sample {
                                    session: *session,
                                    frame: Frame {
                                        sample_seq: seq,
                                        time_ms,
                                        period_ms: 0,
                                        clock: event.clock,
                                        values: Vec::new(),
                                    },
                                },
                                segment: event.segment,
                                state: Some(event.kind),
                            },
                        );
                    }
                }
                PendingRecord::Gap(_) => {}
            }
        }
        wal.started = Some(Instant::now());
        self.wal = Some(wal);
        if let Err(error) = self.seal_inner(3, false) {
            self.latest = prior;
            self.segment = prior_segment;
            return Err(error);
        }
        self.wal = None;
        Ok(())
    }

    fn write_boundary(&self, header: &DayHeader, offset: u64) -> io::Result<()> {
        atomic_write(
            &day_path(&self.dir, header.utc_day, "pub")?,
            &PublishBoundary {
                day_id: header.day_id,
                offset,
            }
            .encode(),
        )
    }

    fn replace_wal(&self, header: &DayHeader) -> io::Result<()> {
        atomic_write(
            &day_path(&self.dir, header.utc_day, "wal")?,
            &header.encode(),
        )
    }

    pub fn sync_wal(&mut self) -> io::Result<()> {
        if let Some(wal) = &mut self.wal {
            wal.file.sync_all()?;
            wal.synced_records = wal.records.len();
        }
        Ok(())
    }

    pub fn unsynced_samples(&self) -> Option<(Sample, Sample)> {
        let wal = self.wal.as_ref()?;
        let mut samples =
            wal.records[wal.synced_records..]
                .iter()
                .filter_map(|record| match record {
                    PendingRecord::Frame { frame, session, .. } => Some(Sample {
                        frame: frame.clone(),
                        session: *session,
                    }),
                    _ => None,
                });
        let first = samples.next()?;
        let last = samples.next_back().unwrap_or_else(|| first.clone());
        Some((first, last))
    }

    pub fn seal_due(&self, interval: Duration) -> bool {
        self.wal
            .as_ref()
            .and_then(|w| w.started)
            .is_some_and(|start| start.elapsed() >= interval)
    }

    pub fn seal(&mut self, compression: i32) -> io::Result<()> {
        self.seal_inner(compression, true)
    }

    fn seal_inner(&mut self, compression: i32, publish_view: bool) -> io::Result<()> {
        if self.wal.as_ref().is_none_or(|wal| wal.records.is_empty()) {
            return Ok(());
        }
        self.sync_wal()?;
        let wal = self.wal.as_ref().unwrap();
        let header = wal.header;
        let day = header.utc_day;
        let meta = &self.days[&day];
        let previous = meta.snapshot.published_through;
        let mut pos = previous;
        let mut sessions = meta.sessions.clone();
        let mut catalogs = meta.catalogs.clone();
        let mut staged = Vec::<(Packet, Vec<u8>)>::new();
        let mut i = 0;
        while i < wal.records.len() {
            match &wal.records[i] {
                PendingRecord::Frame {
                    session,
                    catalog,
                    segment,
                    ..
                } => {
                    let session_ref =
                        ensure_published_session(session, &mut sessions, &mut pos, &mut staged)?;
                    if let Some(old) = catalogs.get(&catalog.version) {
                        if old != catalog {
                            return Err(invalid("conflicting catalog version"));
                        }
                    } else {
                        catalogs.insert(catalog.version, catalog.clone());
                        stage(&mut staged, &mut pos, Packet::Catalog(catalog.clone()))?;
                    }
                    let mut frames = Vec::new();
                    while let Some(PendingRecord::Frame {
                        frame,
                        session: s,
                        catalog: c,
                        segment: g,
                    }) = wal.records.get(i)
                    {
                        if s != session
                            || c != catalog
                            || g != segment
                            || frames.len() == MAX_BLOCK_POINTS
                        {
                            break;
                        }
                        frames.push(frame.clone());
                        i += 1;
                    }
                    stage_blocks(
                        &mut staged,
                        &mut pos,
                        frames,
                        session_ref,
                        *segment,
                        catalog,
                        compression,
                    )?;
                }
                PendingRecord::Event { event, session } => {
                    if event.kind != MarkerKind::Continuation {
                        let session_ref = ensure_published_session(
                            session,
                            &mut sessions,
                            &mut pos,
                            &mut staged,
                        )?;
                        stage(
                            &mut staged,
                            &mut pos,
                            Packet::Marker(Marker::Event(Event {
                                session_ref,
                                ..*event
                            })),
                        )?;
                    }
                    i += 1;
                }
                PendingRecord::Gap(gap) => {
                    stage(&mut staged, &mut pos, Packet::Gap(*gap))?;
                    i += 1;
                }
            }
        }
        let mut rhh = open_wal(&day_path(&self.dir, day, "rhh")?)?;
        if rhh.metadata()?.len() != previous {
            return Err(invalid("published day changed during recording"));
        }
        for (_, encoded) in &staged {
            rhh.write_all(encoded)?;
        }
        rhh.sync_all()?;
        // 在同一个临界区持久化边界并切换已发布视图和尾部视图。
        // WAL 替换在提交后执行，失败可在恢复时重试。
        let state = self.read.clone();
        let mut read = publish_view.then(|| state.write().unwrap());
        self.write_boundary(&header, pos)?;
        let meta = self.days.get_mut(&day).unwrap();
        let mut at = previous;
        let mut activity = None;
        for (packet, encoded) in staged {
            let end = at + encoded.len() as u64;
            meta.index(&packet, at, end, &mut activity)?;
            at = end;
        }
        self.disk_bytes = self.disk_bytes.saturating_add(pos - previous);
        if let Some(read) = &mut read {
            read.tail = Tail::empty();
            read.tail.append(&header.encode(), [HEADER]);
            self.install_snapshot(read);
        }
        drop(read);
        let wal = self.wal.take().unwrap();
        let length = wal.length;
        drop(wal);
        self.replace_wal(&header)?;
        let file = open_wal(&day_path(&self.dir, day, "wal")?)?;
        self.wal = Some(Wal {
            header,
            file,
            length: HEADER,
            started: None,
            records: Vec::new(),
            points: 0,
            synced_records: 0,
            active_session: None,
            active_catalog: None,
        });
        self.disk_bytes = self.disk_bytes.saturating_sub(length - HEADER);
        if publish_view {
            self.refresh_snapshot();
        }
        Ok(())
    }

    pub fn record(
        &mut self,
        sample: &Sample,
        losses: &[(i64, GapReason)],
        options: &RecorderOptions,
    ) -> io::Result<()> {
        let day = sample_day(sample).ok_or_else(|| invalid("invalid sample wall time"))?;
        if self.latest.as_ref().is_some_and(|p| {
            p.sample.session.session_id == sample.session.session_id
                && p.sample.frame.sample_seq >= sample.frame.sample_seq
        }) {
            return Ok(());
        }
        let mut reason = self.latest.as_ref().and_then(|prev| {
            clock::classify(
                &prev.sample.session,
                &prev.sample.frame,
                prev.state == Some(MarkerKind::Exit),
                &sample.session,
                &sample.frame,
            )
        });
        // 小于证据容差的墙钟回拨也不能写入要求时间单调的块。
        if reason.is_none()
            && self
                .latest
                .as_ref()
                .is_some_and(|prev| prev.sample.frame.time_ms > sample.frame.time_ms)
        {
            reason = Some(GapReason::ClockJump);
        }
        let disabled = self
            .latest
            .as_ref()
            .is_some_and(|p| p.state == Some(MarkerKind::Disabled));
        let started = self
            .latest
            .as_ref()
            .is_none_or(|p| p.sample.session.session_id != sample.session.session_id);
        let clock_jump = reason == Some(GapReason::ClockJump);
        let rotated = self.active_day() != Some(day);
        let segment_changed = clock_jump || started || disabled;
        if segment_changed && !rotated {
            self.seal(options.compression_level)?;
        }
        if self
            .wal
            .as_ref()
            .is_some_and(|w| w.points == MAX_BLOCK_POINTS || w.length as usize >= WAL_BLOCK_LIMIT)
        {
            self.seal(options.compression_level)?;
        }
        if self
            .wal
            .as_ref()
            .is_some_and(|wal| wal.header.utc_day != day)
        {
            self.seal(options.compression_level)?;
            self.wal = None;
            self.read.write().unwrap().tail = Tail::empty();
        }
        // 为这批 WAL 的追加和后续发布预留空间，
        // 避免达到容量上限后无法封存。
        if options.retention_bytes != 0 {
            let pending = self
                .wal
                .as_ref()
                .map_or(0, |w| w.length.saturating_sub(HEADER));
            let reserve = 4096u64.saturating_add(pending);
            if self.disk_bytes.saturating_add(reserve) > options.retention_bytes {
                return Err(io::Error::other("history retention capacity reached"));
            }
        }
        self.select_day(day, options.compression_level)?;
        let needs_wal_segment = self
            .wal
            .as_ref()
            .is_some_and(|w| w.active_session.is_none());
        if segment_changed || self.segment == 0 {
            self.segment = self
                .segment
                .checked_add(1)
                .ok_or_else(|| invalid("history segment exhausted"))?;
        }
        let segment = self.segment;
        let mut batch = Vec::new();
        if started {
            batch.push(event(sample, MarkerKind::Start, segment));
        }
        if clock_jump {
            batch.push(event(sample, MarkerKind::ClockSegment, segment));
        }
        if disabled {
            batch.push(event(sample, MarkerKind::Resumed, segment));
        }
        if needs_wal_segment && !started && !clock_jump && !disabled {
            batch.push(event(sample, MarkerKind::Continuation, segment));
        }
        let end = sample.frame.time_ms;
        let previous_end = self.latest.as_ref().map(|a| covered_until(&a.sample.frame));
        if let Some(begin) = previous_end {
            if disabled {
                push_gap(&mut batch, begin, end, GapReason::Disabled, segment);
            }
            if let Some(reason) = reason {
                push_gap(&mut batch, begin, end, reason, segment);
            }
            // 详细队列溢出证据没有跨崩溃保存时，仍可以从样本序号发现跳过；
            // 如已有明确原因，则以其为准。
            if !disabled
                && losses.is_empty()
                && !started
                && self.latest.as_ref().is_some_and(|a| {
                    a.sample.frame.sample_seq.saturating_add(1) < sample.frame.sample_seq
                })
            {
                push_gap(&mut batch, begin, end, GapReason::RecorderOverrun, segment);
            }
        }
        for &(from, reason) in losses {
            // 以最近样本的覆盖终点与首个丢失时间构建故障区间。
            // 当墙钟逆转，保留丢失记录的时刻，不使用相反方向的区间。
            let begin = previous_end.map_or(from, |previous| {
                if previous < end {
                    previous.min(from)
                } else {
                    from
                }
            });
            push_gap(&mut batch, begin, end, reason, segment);
        }
        batch.push(PendingRecord::Frame {
            frame: sample.frame.clone(),
            session: sample.session,
            catalog: Catalog::standard(1),
            segment,
        });
        self.append(batch)?;
        self.latest = Some(Activity {
            sample: sample.clone(),
            segment,
            state: None,
        });
        let mut read = self.read.write().unwrap();
        let snapshot = Arc::make_mut(&mut read.snapshot);
        snapshot.latest = Some((sample.session.session_id, sample.frame.sample_seq));
        snapshot.latest_wall_ms = Some(sample.frame.time_ms);
        snapshot.disk_bytes = self.disk_bytes;
        Ok(())
    }

    pub fn stop_recording(&mut self, compression: i32) -> io::Result<()> {
        if let Some(latest) = self.latest.clone() {
            if latest.state == Some(MarkerKind::Disabled) {
                return Ok(());
            }
            self.select_day(
                sample_day(&latest.sample).ok_or_else(|| invalid("missing last wall time"))?,
                compression,
            )?;
            self.append(vec![event(
                &latest.sample,
                MarkerKind::Disabled,
                latest.segment,
            )])?;
            self.latest.as_mut().unwrap().state = Some(MarkerKind::Disabled);
            self.seal(compression)?;
        }
        Ok(())
    }

    pub fn ensure_disabled(&mut self, sample: &Sample, compression: i32) -> io::Result<()> {
        if self.latest.is_some() {
            return self.stop_recording(compression);
        }
        let Some(day) = sample_day(sample) else {
            return Ok(());
        };
        self.select_day(day, compression)?;
        self.segment += 1;
        self.append(vec![event(sample, MarkerKind::Disabled, self.segment)])?;
        let mut sample = sample.clone();
        sample.frame.period_ms = 0;
        self.latest = Some(Activity {
            sample,
            segment: self.segment,
            state: Some(MarkerKind::Disabled),
        });
        self.seal(compression)
    }

    pub fn flush_losses(
        &mut self,
        compression: i32,
        overrun: Option<&Loss>,
        failed: Option<&Loss>,
    ) -> io::Result<()> {
        let latest = self.latest.clone();
        let mut losses = Vec::new();
        let mut last = None;
        for (loss, reason) in [
            (overrun, GapReason::RecorderOverrun),
            (failed, GapReason::Storage),
        ] {
            let Some(loss) = loss else { continue };
            let Some(from) = loss.start_after(latest.as_ref()) else {
                continue;
            };
            let Some(day) = sample_day(&loss.last) else {
                continue;
            };
            push_gap(
                &mut losses,
                from,
                covered_until(&loss.last.frame),
                reason,
                self.segment,
            );
            if last.is_none_or(|old| day > old) {
                last = Some(day);
            }
        }
        if let Some(day) = last.filter(|_| !losses.is_empty()) {
            self.select_day(day, compression)?;
            self.append(losses)?;
            self.seal(compression)?;
        }
        Ok(())
    }

    pub fn finish(
        &mut self,
        compression: i32,
        overrun: Option<&Loss>,
        failed: Option<&Loss>,
    ) -> io::Result<()> {
        self.flush_losses(compression, overrun, failed)?;
        if let Some(latest) = self
            .latest
            .clone()
            .filter(|latest| latest.state != Some(MarkerKind::Disabled))
        {
            self.select_day(
                sample_day(&latest.sample).ok_or_else(|| invalid("missing last wall time"))?,
                compression,
            )?;
            self.append(vec![event(
                &latest.sample,
                MarkerKind::Exit,
                latest.segment,
            )])?;
        }
        self.seal(compression)
    }

    fn append(&mut self, records: Vec<PendingRecord>) -> io::Result<()> {
        let wal = self
            .wal
            .as_mut()
            .ok_or_else(|| invalid("no open history WAL"))?;
        let mut staged = Vec::new();
        let mut offset = wal.length;
        let mut session = wal.active_session;
        let mut catalog_version = wal.active_catalog;
        let mut points = 0;
        for record in &records {
            match record {
                PendingRecord::Frame {
                    frame,
                    session: s,
                    catalog,
                    ..
                } => {
                    ensure_wal_session(s, &mut session, &mut offset, &mut staged)?;
                    if catalog_version != Some(catalog.version) {
                        stage(&mut staged, &mut offset, Packet::Catalog(catalog.clone()))?;
                        catalog_version = Some(catalog.version);
                    }
                    let packet = Packet::Frame(frame.clone());
                    let encoded = packet.encode(Some(catalog)).map_err(format_error)?;
                    offset += encoded.len() as u64;
                    staged.push((packet, encoded));
                    points += 1;
                }
                PendingRecord::Event { event, session: s } => {
                    let session_ref =
                        ensure_wal_session(s, &mut session, &mut offset, &mut staged)?;
                    stage(
                        &mut staged,
                        &mut offset,
                        Packet::Marker(Marker::Event(Event {
                            session_ref,
                            ..*event
                        })),
                    )?;
                }
                PendingRecord::Gap(gap) => stage(&mut staged, &mut offset, Packet::Gap(*gap))?,
            }
        }
        let mut bytes = Vec::with_capacity((offset - wal.length) as usize);
        let mut ends = Vec::with_capacity(staged.len());
        for (_, encoded) in staged {
            bytes.extend_from_slice(&encoded);
            ends.push(wal.length + bytes.len() as u64);
        }
        wal.file.write_all(&bytes)?;
        self.disk_bytes = self.disk_bytes.saturating_add(offset - wal.length);
        wal.length = offset;
        wal.points += points;
        wal.active_session = session;
        wal.active_catalog = catalog_version;
        wal.started.get_or_insert_with(Instant::now);
        wal.records.extend(records);
        let mut read = self.read.write().unwrap();
        read.tail.append(&bytes, ends);
        Ok(())
    }

    fn select_day(&mut self, day: i32, compression: i32) -> io::Result<bool> {
        if self.bad_days.contains_key(&day)
            || self.bad_wals.contains(&day)
            || self.hidden.contains(&day)
        {
            return Err(invalid("history day is unavailable"));
        }
        if self.active_day() == Some(day) {
            return Ok(false);
        }
        self.seal(compression)?;
        self.wal = None;
        let header = if let Some(meta) = self.days.get(&day) {
            DayHeader {
                utc_day: day,
                day_id: meta.snapshot.day_id,
            }
        } else {
            let header = DayHeader {
                utc_day: day,
                day_id: rand::random(),
            };
            let path = day_path(&self.dir, day, "rhh")?;
            // 原子写入文件头；缺少 .pub 时，
            // 只有空日且无 WAL 才视为可恢复初始化。
            if path.try_exists()? {
                return Err(invalid("unexpected day file"));
            }
            atomic_write(&path, &header.encode())?;
            self.write_boundary(&header, HEADER)?;
            self.days.insert(day, Day::new(&header));
            self.disk_bytes += (DAY_HEADER_LEN + PUBLISH_BOUNDARY_LEN) as u64;
            header
        };
        let path = day_path(&self.dir, day, "wal")?;
        if !path.try_exists()? {
            self.disk_bytes += HEADER;
        }
        self.replace_wal(&header)?;
        let file = open_wal(&path)?;
        let mut read = self.read.write().unwrap();
        read.tail = Tail::empty();
        read.tail.append(&header.encode(), [HEADER]);
        self.install_snapshot(&mut read);
        drop(read);
        self.wal = Some(Wal {
            header,
            file,
            length: HEADER,
            started: None,
            records: Vec::new(),
            points: 0,
            synced_records: 0,
            active_session: None,
            active_catalog: None,
        });
        Ok(true)
    }
}

const RHH: u8 = 1;
const PUB: u8 = 2;
const WAL: u8 = 4;

fn list_days(dir: &Path) -> io::Result<BTreeMap<i32, u8>> {
    let mut days = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let bit = match path.extension().and_then(|ext| ext.to_str()) {
            Some("rhh") => RHH,
            Some("pub") => PUB,
            Some("wal") => WAL,
            _ => continue,
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Some(day) = parse_day(stem) {
            *days.entry(day).or_default() |= bit;
        }
    }
    Ok(days)
}

fn scan_day(dir: &Path, day: i32, files: u8) -> io::Result<(Day, Option<Activity>)> {
    if files & RHH == 0 {
        return Err(invalid("missing published history file"));
    }
    let mut file = File::open(day_path(dir, day, "rhh")?)?;
    let header = read_header(&mut file)?;
    if header.utc_day != day {
        return Err(invalid("day header does not match filename"));
    }
    let len = file.metadata()?.len();
    let through = if files & PUB != 0 {
        let boundary = read_boundary_from(&mut File::open(day_path(dir, day, "pub")?)?)?;
        if boundary.day_id != header.day_id || boundary.offset > len {
            return Err(invalid("invalid publication boundary"));
        }
        boundary.offset
    } else if len == HEADER && files & WAL == 0 {
        HEADER
    } else {
        return Err(invalid("missing publication boundary"));
    };
    let mut meta = Day::new(&header);
    let mut latest = None;
    let mut offset = HEADER;
    let mut context = FileContext::new(FileKind::Day);
    while offset < through {
        let Some(bytes) = read_packet(&mut file, through - offset)? else {
            return Err(invalid("published prefix ends inside a packet"));
        };
        let (raw, count) = decode_packet(&bytes).map_err(format_error)?;
        let packet = context.decode_at(offset, &raw).map_err(format_error)?;
        let end = offset + count as u64;
        meta.index(&packet, offset, end, &mut latest)?;
        offset = end;
    }
    Ok((meta, latest))
}

fn read_wal(dir: &Path, header: DayHeader, day: &Day, initial_segment: u64) -> io::Result<Wal> {
    let mut file =
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(day_path(dir, header.utc_day, "wal")?)?;
    if read_header(&mut file)? != header {
        return Err(invalid("WAL header does not match publication"));
    }
    let len = file.metadata()?.len();
    if len > MAX_PENDING_WAL {
        return Err(invalid("WAL exceeds recovery limit"));
    }
    let mut offset = HEADER;
    let mut context = FileContext::new(FileKind::Wal);
    let mut records = Vec::new();
    let mut points = 0;
    let mut segment = initial_segment;
    let mut events = HashMap::<Id, Packet>::new();
    let mut last_seq = HashMap::<Id, u64>::new();
    while offset < len {
        let Some(bytes) = read_packet(&mut file, len - offset)? else {
            // 仅修剪能确定不完整的包；CRC 校验失败、
            // 非法长度或完整但格式错误的负载均保留原状。
            file.set_len(offset)?;
            file.sync_all()?;
            break;
        };
        let (raw, count) = decode_packet(&bytes).map_err(format_error)?;
        let packet = context.decode_at(offset, &raw).map_err(format_error)?;
        offset += count as u64;
        match packet {
            Packet::Marker(Marker::Event(event)) => {
                let session = *context
                    .session(event.session_ref)
                    .ok_or_else(|| invalid("missing WAL session"))?;
                segment = event.segment;
                let normalized = Packet::Marker(Marker::Event(Event {
                    session_ref: 0,
                    ..event
                }));
                if event.kind != MarkerKind::Continuation
                    && insert_event(&mut events, event.event_id, normalized)?
                    && !day.events.contains(&event.event_id)
                {
                    records.push(PendingRecord::Event { event, session });
                }
            }
            Packet::Frame(frame) => {
                let (_, session) = context
                    .current_session()
                    .ok_or_else(|| invalid("missing WAL session"))?;
                if utc_day(frame.time_ms) != Some(header.utc_day) {
                    return Err(invalid("WAL sample belongs to another UTC day"));
                }
                if day.contains(session.session_id, frame.sample_seq) {
                    continue;
                }
                match last_seq.get(&session.session_id) {
                    Some(last) if *last == frame.sample_seq => continue,
                    Some(last) if *last > frame.sample_seq => {
                        return Err(invalid("WAL has non-increasing pending sample sequence"));
                    }
                    _ => {
                        last_seq.insert(session.session_id, frame.sample_seq);
                    }
                }
                let catalog = context
                    .current_catalog()
                    .ok_or_else(|| invalid("missing WAL catalog"))?
                    .clone();
                records.push(PendingRecord::Frame {
                    frame,
                    session: *session,
                    catalog,
                    segment,
                });
                points += 1;
            }
            Packet::Gap(gap) => {
                if insert_event(&mut events, gap.event_id, Packet::Gap(gap))?
                    && !day.events.contains(&gap.event_id)
                {
                    records.push(PendingRecord::Gap(gap));
                }
            }
            Packet::Catalog(_) | Packet::Marker(Marker::Session(_)) => {}
            _ => return Err(invalid("unexpected WAL packet type")),
        }
    }
    Ok(Wal {
        header,
        file,
        length: offset,
        started: None,
        records,
        points,
        synced_records: 0,
        active_session: context.current_session().map(|(at, s)| (at, *s)),
        active_catalog: context.current_catalog().map(|c| c.version),
    })
}

fn insert_event(events: &mut HashMap<Id, Packet>, id: Id, record: Packet) -> io::Result<bool> {
    use std::collections::hash_map::Entry;
    match events.entry(id) {
        Entry::Occupied(entry) if entry.get() == &record => Ok(false),
        Entry::Occupied(_) => Err(invalid("conflicting event identity in WAL")),
        Entry::Vacant(entry) => {
            entry.insert(record);
            Ok(true)
        }
    }
}

fn read_packet(file: &mut File, remaining: u64) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0; 5];
    if remaining < header.len() as u64 {
        return Ok(None);
    }
    file.read_exact(&mut header)?;
    let len = packet_len(&header).map_err(format_error)?;
    if len as u64 > remaining {
        return Ok(None);
    }
    let mut bytes = vec![0; len];
    bytes[..header.len()].copy_from_slice(&header);
    file.read_exact(&mut bytes[header.len()..])?;
    Ok(Some(bytes))
}

fn read_header(file: &mut File) -> io::Result<DayHeader> {
    let mut bytes = [0; DAY_HEADER_LEN];
    file.read_exact(&mut bytes)?;
    DayHeader::decode(&bytes).map_err(format_error)
}

fn read_boundary_from(file: &mut File) -> io::Result<PublishBoundary> {
    let mut bytes = [0; PUBLISH_BOUNDARY_LEN];
    file.read_exact(&mut bytes)?;
    if file.read(&mut [0; 1])? != 0 {
        return Err(invalid("trailing publication bytes"));
    }
    PublishBoundary::decode(&bytes).map_err(format_error)
}

fn read_store_id(dir: &Path) -> io::Result<Id> {
    let mut file = File::open(dir.join("store_id"))?;
    let mut id = [0; 16];
    file.read_exact(&mut id)?;
    if file.read(&mut [0; 1])? != 0 || id == [0; 16] {
        return Err(invalid("invalid history store ID"));
    }
    Ok(id)
}

fn open_wal(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).append(true).open(path)
}

fn covered_until(frame: &Frame) -> i64 {
    frame.time_ms.saturating_add(i64::from(frame.period_ms))
}

fn event(sample: &Sample, kind: MarkerKind, segment: u64) -> PendingRecord {
    PendingRecord::Event {
        session: sample.session,
        event: Event {
            kind,
            event_id: rand::random(),
            segment,
            session_ref: 0,
            sample_seq: Some(sample.frame.sample_seq),
            clock: sample.frame.clock,
        },
    }
}

fn push_gap(
    batch: &mut Vec<PendingRecord>,
    from_ms: i64,
    to_ms: i64,
    reason: GapReason,
    segment: u64,
) {
    if from_ms < to_ms {
        batch.push(PendingRecord::Gap(Gap {
            from_ms,
            to_ms,
            reason,
            segment,
            event_id: rand::random(),
        }));
    }
}

fn ensure_wal_session(
    session: &Session,
    current: &mut Option<(u64, Session)>,
    pos: &mut u64,
    staged: &mut Vec<(Packet, Vec<u8>)>,
) -> io::Result<u64> {
    if let Some((offset, active)) = current
        && session == active
    {
        return Ok(*offset);
    }
    let offset = *pos;
    stage(staged, pos, Packet::Marker(Marker::Session(*session)))?;
    *current = Some((offset, *session));
    Ok(offset)
}

fn ensure_published_session(
    session: &Session,
    sessions: &mut HashMap<Id, (u64, Session)>,
    pos: &mut u64,
    staged: &mut Vec<(Packet, Vec<u8>)>,
) -> io::Result<u64> {
    if let Some((offset, old)) = sessions.get(&session.session_id) {
        if old != session {
            return Err(invalid("conflicting published session definition"));
        }
        return Ok(*offset);
    }
    let offset = *pos;
    stage(staged, pos, Packet::Marker(Marker::Session(*session)))?;
    sessions.insert(session.session_id, (offset, *session));
    Ok(offset)
}

fn stage(staged: &mut Vec<(Packet, Vec<u8>)>, pos: &mut u64, packet: Packet) -> io::Result<()> {
    let encoded = packet.encode(None).map_err(format_error)?;
    *pos += encoded.len() as u64;
    staged.push((packet, encoded));
    Ok(())
}

fn stage_blocks(
    staged: &mut Vec<(Packet, Vec<u8>)>,
    pos: &mut u64,
    mut frames: Vec<Frame>,
    session_ref: u64,
    segment: u64,
    catalog: &Catalog,
    compression: i32,
) -> io::Result<()> {
    let block = Block {
        catalog_version: catalog.version,
        session_ref,
        segment,
        frames,
    };
    match block.encode_packet(catalog, compression) {
        Ok(encoded) => {
            *pos += encoded.len() as u64;
            staged.push((Packet::Block(block), encoded));
            Ok(())
        }
        Err(Error::Limit(_)) if block.frames.len() > 1 => {
            frames = block.frames;
            let right = frames.split_off(frames.len() / 2);
            stage_blocks(
                staged,
                pos,
                frames,
                session_ref,
                segment,
                catalog,
                compression,
            )?;
            stage_blocks(
                staged,
                pos,
                right,
                session_ref,
                segment,
                catalog,
                compression,
            )
        }
        Err(error) => Err(format_error(error)),
    }
}

fn format_error(error: Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
fn invalid(reason: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason)
}

fn query_error(error: &io::Error) -> QueryError {
    if matches!(
        error.get_ref().and_then(|e| e.downcast_ref::<Error>()),
        Some(Error::UnsupportedVersion(_))
    ) {
        return QueryError::UnsupportedVersion;
    }
    match error.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof | io::ErrorKind::NotFound => {
            QueryError::CorruptDay
        }
        _ => QueryError::Io,
    }
}

fn epoch() -> Date {
    Date::from_calendar_date(1970, Month::January, 1).unwrap()
}

pub(super) fn utc_day(ms: i64) -> Option<i32> {
    let day = i32::try_from(ms.div_euclid(DAY_MS)).ok()?;
    epoch().checked_add(Days::days(i64::from(day)))?;
    Some(day)
}

pub(super) fn sample_day(sample: &Sample) -> Option<i32> {
    let wall = sample.frame.clock.wall_ms?;
    (wall == sample.frame.time_ms)
        .then(|| utc_day(wall))
        .flatten()
}

fn parse_day(stem: &str) -> Option<i32> {
    let (ym, d) = stem.rsplit_once('-')?;
    let (y, m) = ym.rsplit_once('-')?;
    let year: i32 = y.parse().ok()?;
    let day = Date::from_calendar_date(
        year,
        Month::try_from(m.parse::<u8>().ok()?).ok()?,
        d.parse().ok()?,
    )
    .ok()?;
    if stem != day.to_string() {
        return None;
    }
    i32::try_from((day - epoch()).whole_days()).ok()
}

fn day_path(dir: &Path, day: i32, ext: &str) -> io::Result<PathBuf> {
    let date = epoch()
        .checked_add(Days::days(i64::from(day)))
        .ok_or_else(|| invalid("UTC day out of range"))?;
    Ok(dir.join(format!("{date}.{ext}")))
}

fn directory_bytes(dir: &Path) -> io::Result<u64> {
    fs::read_dir(dir)?.try_fold(0u64, |sum, item| {
        let item = item?;
        let next = if item.file_type()?.is_file() {
            item.metadata()?.len()
        } else {
            0
        };
        Ok(sum.saturating_add(next))
    })
}

/// 在同一目录中创建临时文件并替换；Windows 使用直写替换，
/// Unix 在替换后同步目录。
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| invalid("atomic replacement requires parent"))?;
    let tmp = dir.join(format!(".history-{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result
        .and_then(|()| replace_file(&tmp, path))
        .and_then(|()| sync_dir(dir));
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    const REPLACE_EXISTING: u32 = 1;
    const WRITE_THROUGH: u32 = 8;
    let from: Vec<_> = from.as_os_str().encode_wide().chain([0]).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain([0]).collect();
    if from[..from.len() - 1].contains(&0) || to[..to.len() - 1].contains(&0) {
        return Err(invalid("invalid history path"));
    }
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), REPLACE_EXISTING | WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}
#[cfg(not(unix))]
fn sync_dir(_: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::history::ClockEvidence;

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "remotehelper-history-test-{:032x}",
                rand::random::<u128>()
            )))
        }
        fn open(&self) -> (Store, Arc<RwLock<ReadState>>) {
            let state = Arc::new(RwLock::new(ReadState::new(self.0.clone())));
            (Store::open(self.0.clone(), state.clone()).unwrap(), state)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample(seq: u64, time_ms: i64) -> Sample {
        Sample {
            session: Session {
                session_id: [42; 16],
                boot_id: Some([12; 16]),
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
        }
    }

    fn options() -> RecorderOptions {
        RecorderOptions {
            enabled: true,
            seal_interval: Duration::from_secs(300),
            retention_days: 30,
            retention_bytes: 0,
            compression_level: 3,
        }
    }

    fn bytes(dir: &Path, ext: &str) -> Vec<u8> {
        fs::read(day_path(dir, 0, ext).unwrap()).unwrap()
    }

    #[test]
    fn replay_truncates_unpublished_day_and_incomplete_wal() {
        let dir = Temp::new();
        let (mut store, _) = dir.open();
        store.record(&sample(1, 1000), &[], &options()).unwrap();
        let wal_len = bytes(&dir.0, "wal").len();
        drop(store);
        let path = day_path(&dir.0, 0, "rhh").unwrap();
        open_wal(&path)
            .unwrap()
            .write_all(&[1, 2, 3, 4, 5, 6])
            .unwrap();
        let path = day_path(&dir.0, 0, "wal").unwrap();
        open_wal(&path).unwrap().write_all(&[2, 7, 0]).unwrap();
        assert!(bytes(&dir.0, "wal").len() > wal_len);
        let (store, state) = dir.open();
        let read = state.read().unwrap();
        let meta = &read.snapshot().days[&0];
        assert_eq!(meta.data_range, Some((1000, 1001)));
        assert_eq!(meta.published_through, bytes(&dir.0, "rhh").len() as u64);
        assert_eq!(bytes(&dir.0, "wal").len(), DAY_HEADER_LEN);
        assert_eq!(store.latest().unwrap().sample.frame.sample_seq, 1);
    }

    #[test]
    fn published_wal_replay_deduplicates_samples_and_events() {
        let dir = Temp::new();
        let (mut store, _) = dir.open();
        store.record(&sample(1, 1000), &[], &options()).unwrap();
        store
            .record(&sample(3, 3000), &[(2000, GapReason::Storage)], &options())
            .unwrap();
        let stale_wal = bytes(&dir.0, "wal");
        store.seal(3).unwrap();
        let published = bytes(&dir.0, "rhh");
        drop(store);
        fs::write(day_path(&dir.0, 0, "wal").unwrap(), stale_wal).unwrap();
        let (_, state) = dir.open();
        assert_eq!(bytes(&dir.0, "rhh"), published);
        assert_eq!(bytes(&dir.0, "wal").len(), DAY_HEADER_LEN);
        let read = state.read().unwrap();
        assert_eq!(read.snapshot().days[&0].gaps.len(), 1);
    }

    #[test]
    fn crc_corruption_is_preserved_and_never_republished() {
        let dir = Temp::new();
        let (mut store, _) = dir.open();
        store.record(&sample(1, 1000), &[], &options()).unwrap();
        store.seal(3).unwrap();
        store.record(&sample(2, 2000), &[], &options()).unwrap();
        drop(store);
        let mut corrupt = bytes(&dir.0, "wal");
        let last = corrupt.last_mut().unwrap();
        *last ^= 1;
        fs::write(day_path(&dir.0, 0, "wal").unwrap(), &corrupt).unwrap();
        let (mut store, state) = dir.open();
        assert_eq!(bytes(&dir.0, "wal"), corrupt);
        assert!(state.read().unwrap().snapshot().days.contains_key(&0));
        assert!(store.record(&sample(3, 3000), &[], &options()).is_err());
        let mut published = bytes(&dir.0, "rhh");
        *published.last_mut().unwrap() ^= 1;
        drop(store);
        fs::write(day_path(&dir.0, 0, "rhh").unwrap(), &published).unwrap();
        let (_, state) = dir.open();
        assert_eq!(
            state.read().unwrap().snapshot().unavailable_days[&0],
            QueryError::CorruptDay
        );
        assert_eq!(bytes(&dir.0, "rhh"), published);
        assert_eq!(bytes(&dir.0, "wal"), corrupt);
    }

    #[test]
    fn tail_append_keeps_generation_but_seal_invalidates_cursor() {
        let dir = Temp::new();
        let (mut store, state) = dir.open();
        store.record(&sample(1, 1000), &[], &options()).unwrap();
        let cursor = state
            .read()
            .unwrap()
            .tail_page(None, DAY_HEADER_LEN)
            .unwrap()
            .tail_cursor
            .unwrap();
        store.record(&sample(2, 2000), &[], &options()).unwrap();
        let page = state
            .read()
            .unwrap()
            .tail_page(Some(cursor), PAGE_LIMIT)
            .unwrap();
        assert_eq!(page.next_offset, cursor.through);
        assert!(
            state
                .read()
                .unwrap()
                .tail_page(None, PAGE_LIMIT)
                .unwrap()
                .through
                > cursor.through
        );
        store.seal(3).unwrap();
        let read = state.read().unwrap();
        assert_eq!(
            read.tail_page(Some(cursor), PAGE_LIMIT).unwrap_err(),
            QueryError::TailRestartRequired
        );
        let snapshot = read.snapshot();
        let day = &snapshot.days[&0];
        assert_eq!(
            read.day_page(0, day.day_id, 0, day.published_through, PAGE_LIMIT)
                .unwrap()
                .bytes,
            bytes(&dir.0, "rhh")
        );
        assert_eq!(
            read.day_page(0, day.day_id, 1, day.published_through, PAGE_LIMIT)
                .unwrap_err(),
            QueryError::InvalidCursor
        );
    }

    #[test]
    fn query_finds_gap_in_later_utc_day_and_old_day_can_be_revisited() {
        let dir = Temp::new();
        let (mut store, state) = dir.open();
        store.record(&sample(1, 1000), &[], &options()).unwrap();
        let first_id = state.read().unwrap().snapshot().days[&0].day_id;
        store
            .record(&sample(2, 2 * DAY_MS + 1000), &[], &options())
            .unwrap();
        store.seal(3).unwrap();
        let snap = state.read().unwrap().snapshot();
        assert_eq!(
            snap.days_intersecting(DAY_MS, DAY_MS + 1000)
                .next()
                .unwrap()
                .utc_day,
            2
        );
        let mut rolled_back = sample(3, 2000);
        rolled_back.frame.clock.tick_ms = Some(2 * DAY_MS as u64 + 2000);
        store.record(&rolled_back, &[], &options()).unwrap();
        store.seal(3).unwrap();
        assert_eq!(state.read().unwrap().snapshot().days[&0].day_id, first_id);
        drop(store);
        let (store, _) = dir.open();
        assert_eq!(store.latest().unwrap().sample.frame.sample_seq, 3);
    }

    #[test]
    fn bounded_fifo_places_overrun_before_fenced_controls() {
        use super::super::{Inbox, Message, Pending, QUEUE_LIMIT, RecorderHandle};
        use std::sync::{Condvar, Mutex};

        let dir = Temp::new();
        let handle = RecorderHandle {
            inbox: Arc::new(Inbox {
                pending: Mutex::new(Pending::default()),
                ready: Condvar::new(),
            }),
            read: Arc::new(RwLock::new(ReadState::new(dir.0.clone()))),
        };
        for seq in 1..=QUEUE_LIMIT as u64 {
            assert!(handle.record(sample(seq, seq as i64 * 1000)));
        }
        assert!(!handle.record(sample(257, 257_000)));
        assert!(!handle.record(sample(258, 258_000)));
        assert!(handle.configure(RecorderOptions {
            enabled: false,
            ..options()
        }));
        handle.close();
        assert!(!handle.record(sample(259, 259_000)));
        for seq in 1..=QUEUE_LIMIT as u64 {
            assert!(
                matches!(handle.inbox.recv(Duration::ZERO), Some(Message::Sample(s)) if s.frame.sample_seq == seq)
            );
        }
        assert!(
            matches!(handle.inbox.recv(Duration::ZERO), Some(Message::Overrun(loss)) if loss.first.frame.sample_seq == 257 && loss.last.frame.sample_seq == 258)
        );
        assert!(
            matches!(handle.inbox.recv(Duration::ZERO), Some(Message::Configure(o)) if !o.enabled)
        );
        assert!(matches!(
            handle.inbox.recv(Duration::ZERO),
            Some(Message::Close)
        ));
    }

    #[test]
    fn disabled_marker_survives_restart_and_records_the_gap() {
        let dir = Temp::new();
        let (mut store, _) = dir.open();
        store.record(&sample(1, 1000), &[], &options()).unwrap();
        let mut next = sample(3, 3000);
        next.session.session_id = [43; 16];
        // 模拟退出标记已封存而后续重新启动。
        store.stop_recording(3).unwrap();
        store.finish(3, None, None).unwrap();
        drop(store);
        let (mut store, state) = dir.open();
        assert_eq!(store.latest().unwrap().state, Some(MarkerKind::Disabled));
        store.record(&next, &[], &options()).unwrap();
        store.seal(3).unwrap();
        assert_eq!(
            state.read().unwrap().snapshot().days[&0].gap_range,
            Some((2000, 3000))
        );
    }

    #[test]
    fn recording_recovers_after_storage_becomes_writable() {
        use super::super::{Inbox, Pending, RecorderHandle, Worker};
        use std::sync::{Condvar, Mutex};

        let dir = Temp::new();
        fs::create_dir_all(&dir.0).unwrap();
        let target = dir.0.join("main");
        fs::write(&target, []).unwrap();
        let handle = RecorderHandle {
            inbox: Arc::new(Inbox {
                pending: Mutex::new(Pending::default()),
                ready: Condvar::new(),
            }),
            read: Arc::new(RwLock::new(ReadState::new(target.clone()))),
        };
        let mut writer = Worker::new(target.clone(), options(), handle.clone());
        writer.open();
        assert!(matches!(
            handle.snapshot().storage,
            StorageState::Unavailable(_)
        ));
        writer.record(sample(1, 1000));
        writer.record(sample(2, 2000));
        fs::remove_file(target).unwrap();
        writer.open();
        writer.record(sample(3, 3000));
        writer.store.as_mut().unwrap().seal(3).unwrap();
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.storage, StorageState::Recording);
        assert_eq!(snapshot.latest, Some(([42; 16], 3)));
        assert_eq!(snapshot.days[&0].gap_range, Some((1000, 3000)));
    }

    #[test]
    fn directory_lock_rejects_second_writer() {
        let dir = Temp::new();
        let (first, read) = dir.open();
        assert!(Store::open(dir.0.clone(), read).is_err());
        drop(first);
        let _ = dir.open();
    }
}
