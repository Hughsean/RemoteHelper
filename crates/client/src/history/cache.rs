use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, Weak},
};

use ::history::{DayHeader, FileKind, PACKET_HEADER_LEN, packet_len};
use common::history_api::{DayEntry, PacketPage, TailCursor};

use super::{
    decode::{Decoder, Playback, page_bytes},
    *,
};

const MANIFEST_LIMIT: u64 = 8 * MIB;
const MAX_DIRECTORIES: usize = 8;
const MAX_PARTITION_FILES: usize = 32_768;
const MAX_TAIL_SNAPSHOTS: usize = 2;
const MAX_WARNINGS: usize = 32;

type DayKey = (i32, Id);
type StateRegistry = Mutex<HashMap<PathBuf, Weak<Mutex<State>>>>;
static PARTITIONS: OnceLock<StateRegistry> = OnceLock::new();

#[derive(Clone, Debug)]
pub struct SavedHistory {
    pub address: String,
    pub fingerprint: String,
    pub store_id: Id,
    pub current_pin: bool,
    pub last_sync_ms: Option<i64>,
}

#[derive(Clone)]
pub struct HistoryCache {
    pub(super) store_id: Id,
    state: Arc<Mutex<State>>,
}

struct State {
    root: Option<PathBuf>,
    policy: CachePolicy,
    meta: Meta,
    prefixes: BTreeMap<DayKey, Prefix>,
    tails: VecDeque<Tail>,
    committed_tail: Option<Id>,
    memory_bytes: u64,
    memory_lru: VecDeque<DayKey>,
    warnings: VecDeque<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Meta {
    version: u16,
    address: String,
    summary: Option<HistorySummary>,
    directories: VecDeque<DirectorySnapshot>,
    status: SyncStatus,
    last_sync_ms: Option<i64>,
}

struct Prefix {
    decoder: Decoder,
    memory: Option<Vec<u8>>,
    modified: Option<std::time::SystemTime>,
}

struct Tail {
    cursor: TailCursor,
    bytes: Vec<u8>,
}

fn hosts_path() -> HistoryResult<PathBuf> {
    let root = dirs::data_local_dir().ok_or(HistoryError::Protocol("无法定位应用数据目录"))?;
    Ok(root.join("RemoteHelper").join("hosts"))
}

fn validate_fingerprint(value: &str) -> HistoryResult<()> {
    crate::server_pins::validate_fingerprint(value).map_err(HistoryError::Connection)
}

fn hex(id: Id) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn id_from_hex(text: &str) -> Option<Id> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn day_name((day, id): DayKey) -> String {
    format!("{day}_{}.rhh", hex(id))
}

fn day_key(path: &Path) -> Option<DayKey> {
    if path.extension()?.to_str()? != "rhh" {
        return None;
    }
    let (day, id) = path.file_stem()?.to_str()?.split_once('_')?;
    let key = (day.parse().ok()?, id_from_hex(id)?);
    (path.file_name()?.to_str()? == day_name(key)).then_some(key)
}

fn read_meta(root: &Path) -> HistoryResult<Meta> {
    let mut bytes = Vec::new();
    match File::open(root.join("history.v1.json")) {
        Ok(file) => file.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes)?,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Meta::default()),
        Err(err) => return Err(err.into()),
    };
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(HistoryError::Limit("清单大小"));
    }
    let meta: Meta = serde_json::from_slice(&bytes)?;
    if meta.version != 1
        || meta.directories.len() > MAX_DIRECTORIES
        || meta
            .directories
            .iter()
            .any(|d| d.entries.len() > MAX_DIRECTORY_ENTRIES)
    {
        return Err(HistoryError::Protocol("清单版本或条目数无效"));
    }
    Ok(meta)
}

fn sync_dir(root: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}

fn save_meta(root: &Path, meta: &Meta) -> HistoryResult<()> {
    let bytes = serde_json::to_vec(meta)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(HistoryError::Limit("清单大小"));
    }
    atomic_json(root, "history.v1.json", &bytes)
}

fn config_path() -> HistoryResult<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or(HistoryError::Protocol("无法定位客户端设置目录"))?
        .join("remotehelper"))
}

pub(super) fn load_policy() -> HistoryResult<CachePolicy> {
    let file = match File::open(config_path()?.join("settings.json")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(CachePolicy::default()),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(16 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 {
        return Err(HistoryError::Limit("客户端设置大小"));
    }
    let policy: CachePolicy = serde_json::from_slice(&bytes)?;
    policy.validate()?;
    Ok(policy)
}

pub(super) fn save_policy(policy: CachePolicy) -> HistoryResult<()> {
    let root = config_path()?;
    fs::create_dir_all(&root)?;
    atomic_json(&root, "settings.json", &serde_json::to_vec_pretty(&policy)?)
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
    let c_path = |path: &Path| -> io::Result<Vec<u16>> {
        let mut wide: Vec<_> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "路径含空字符"));
        }
        wide.push(0);
        Ok(wide)
    };
    let from = c_path(from)?;
    let to = c_path(to)?;
    // 两个以空字符结尾的 UTF-16 路径在调用返回前保持有效。
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), REPLACE_EXISTING | WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn atomic_json(root: &Path, name: &str, bytes: &[u8]) -> HistoryResult<()> {
    let temp = root.join(format!("history-{:032x}.tmp", rand::random::<u128>()));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temp, &root.join(name))?;
        sync_dir(root)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    Ok(result?)
}

impl HistoryCache {
    /// 查询当前固定地址下的现有分区，不会创建新的缓存目录。
    pub async fn open_pinned(
        address: &str,
        fingerprint: &str,
        store_id: Id,
        policy: CachePolicy,
    ) -> HistoryResult<Self> {
        let address = address.to_owned();
        let fingerprint = fingerprint.to_owned();
        tokio::task::spawn_blocking(move || {
            if crate::server_pins::load_server_pin(&address)
                .map_err(HistoryError::Connection)?
                .as_deref()
                != Some(&fingerprint)
            {
                return Err(HistoryError::Connection(
                    "离线分区不属于该地址当前固定的指纹".into(),
                ));
            }
            Self::open(Some(address), fingerprint, store_id, policy, false)
        })
        .await?
    }

    /// 打开曾经由已验证连接写入的分区；重新绑定后旧分区仍可读。
    pub async fn open_saved(
        fingerprint: &str,
        store_id: Id,
        policy: CachePolicy,
    ) -> HistoryResult<Self> {
        let fingerprint = fingerprint.to_owned();
        tokio::task::spawn_blocking(move || Self::open(None, fingerprint, store_id, policy, false))
            .await?
    }

    pub async fn list_pinned() -> HistoryResult<Vec<SavedHistory>> {
        tokio::task::spawn_blocking(|| {
            let pins = crate::server_pins::list_server_pins().map_err(HistoryError::Connection)?;
            let root = hosts_path()?;
            let hosts = match fs::read_dir(root) {
                Ok(v) => v,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(e) => return Err(e.into()),
            };
            let mut saved = Vec::new();
            for host in hosts.take(MAX_PARTITION_FILES) {
                let host = host?;
                if !host.file_type()?.is_dir() {
                    continue;
                }
                let fingerprint = host.file_name().to_string_lossy().into_owned();
                if validate_fingerprint(&fingerprint).is_err() {
                    continue;
                }
                let pinned_address = pins
                    .iter()
                    .find(|(_, pin)| **pin == fingerprint)
                    .map(|(addr, _)| addr.as_str());
                for store in fs::read_dir(host.path())?.take(MAX_PARTITION_FILES) {
                    let store = store?;
                    if !store.file_type()?.is_dir() {
                        continue;
                    }
                    let Some(store_id) = store.file_name().to_str().and_then(id_from_hex) else {
                        continue;
                    };
                    let meta = read_meta(&store.path()).unwrap_or_default();
                    if meta.address.is_empty() && pinned_address.is_none() {
                        continue;
                    }
                    let address = if meta.address.is_empty() {
                        pinned_address.unwrap().to_owned()
                    } else {
                        meta.address
                    };
                    let current_pin = pins.get(&address).is_some_and(|pin| pin == &fingerprint);
                    saved.push(SavedHistory {
                        address,
                        fingerprint: fingerprint.clone(),
                        store_id,
                        current_pin,
                        last_sync_ms: meta.last_sync_ms,
                    });
                    if saved.len() >= MAX_PARTITION_FILES {
                        return Err(HistoryError::Limit("离线分区数量"));
                    }
                }
            }
            saved.sort_by_key(|item| {
                (
                    std::cmp::Reverse(item.last_sync_ms),
                    item.address.clone(),
                    item.fingerprint.clone(),
                    item.store_id,
                )
            });
            Ok(saved)
        })
        .await?
    }

    pub(super) async fn open_online(
        address: String,
        fingerprint: String,
        store_id: Id,
        policy: CachePolicy,
    ) -> HistoryResult<Self> {
        tokio::task::spawn_blocking(move || {
            Self::open(Some(address), fingerprint, store_id, policy, true)
        })
        .await?
    }

    fn open(
        address: Option<String>,
        fingerprint: String,
        store_id: Id,
        mut policy: CachePolicy,
        online: bool,
    ) -> HistoryResult<Self> {
        validate_fingerprint(&fingerprint)?;
        policy.validate()?;
        // 离线读取已有文件，不借用在线写入选项关闭磁盘分区。
        if !online {
            policy.enabled = true;
        }
        let root = if policy.enabled {
            Some(hosts_path()?.join(fingerprint).join(hex(store_id)))
        } else {
            None
        };
        Self::with_root(root, address, store_id, policy, online)
    }

    fn with_root(
        root: Option<PathBuf>,
        address: Option<String>,
        store_id: Id,
        policy: CachePolicy,
        online: bool,
    ) -> HistoryResult<Self> {
        let mut registry = PARTITIONS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        registry.retain(|_, state| state.strong_count() != 0);
        if let Some(state) = root
            .as_ref()
            .and_then(|path| registry.get(path))
            .and_then(Weak::upgrade)
        {
            if online {
                let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
                guard.policy = policy;
                if let Some(addr) = address {
                    guard.meta.address = addr;
                }
            }
            return Ok(Self { store_id, state });
        }
        let mut state = State {
            root: root.clone(),
            policy,
            meta: Meta {
                version: 1,
                ..Meta::default()
            },
            prefixes: BTreeMap::new(),
            tails: VecDeque::new(),
            committed_tail: None,
            memory_bytes: 0,
            memory_lru: VecDeque::new(),
            warnings: VecDeque::new(),
        };
        if let Some(root) = &root {
            if online {
                fs::create_dir_all(root)?;
            }
            let lock = if online {
                Some(lock_partition(root)?)
            } else {
                reader_lock(root)?
            };
            let queue = fs::read_dir(root)?;
            match read_meta(root) {
                Ok(meta) if meta.version == 1 => state.meta = meta,
                Ok(_) => {}
                Err(e) => state.warn(format!("无法加载历史清单，将从日文件重建：{e}")),
            }
            if matches!(state.meta.status, SyncStatus::InProgress) {
                state.meta.status = SyncStatus::Failed {
                    at_ms: now_ms(),
                    reason: "上一轮历史同步未完成".into(),
                };
            }
            if state
                .meta
                .summary
                .as_ref()
                .is_some_and(|s| s.store_id != Some(store_id))
            {
                state.meta = Meta {
                    version: 1,
                    ..Meta::default()
                };
                state.warn("清单与分区标识不符".into());
            }
            for (i, entry) in queue.enumerate() {
                if i >= MAX_PARTITION_FILES {
                    return Err(HistoryError::Limit("离线分区文件数"));
                }
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let Some(key) = day_key(&entry.path()) else {
                    continue;
                };
                state.reload(key, online)?;
            }
            if let Some(addr) = address {
                state.meta.address = addr;
            }
            if online {
                state.save()?;
            }
            drop(lock);
        } else if let Some(addr) = address {
            state.meta.address = addr;
        }
        let state = Arc::new(Mutex::new(state));
        if let Some(root) = root {
            registry.insert(root, Arc::downgrade(&state));
        }
        Ok(Self { store_id, state })
    }

    pub async fn query(&self, range: TimeRange) -> HistoryResult<HistoryQuery> {
        check_range(range)?;
        self.run(move |state| state.query(range, DAY_MS)).await
    }

    /// 供总览分段读取，保留跨边界的邻点，同时跳过与短区间无关的相邻日文件。
    pub async fn query_overview(&self, range: TimeRange) -> HistoryResult<HistoryQuery> {
        check_range(range)?;
        self.run(move |state| state.query(range, 180_000)).await
    }

    pub async fn extent(&self) -> HistoryResult<Option<TimeRange>> {
        self.run(|state| {
            let mut extent = state.local_extent();
            if let Some(range) = state
                .meta
                .summary
                .as_ref()
                .and_then(|summary| summary.range)
            {
                extend_range(&mut extent, range.from, range.to);
            }
            Ok(extent)
        })
        .await
    }

    pub async fn latest_wall_ms(&self) -> HistoryResult<Option<i64>> {
        self.run(|state| {
            Ok(state
                .meta
                .summary
                .as_ref()
                .and_then(|summary| summary.latest_wall_ms))
        })
        .await
    }

    /// 返回有效日文件前缀的字节数与状态，不包括清单和暂存的尾部。
    pub async fn usage(&self) -> HistoryResult<(u64, SyncStatus)> {
        self.run(|state| {
            Ok((
                state.prefixes.values().map(|p| p.decoder.offset).sum(),
                state.meta.status.clone(),
            ))
        })
        .await
    }

    /// 调用前应暂停该分区的同步任务。
    pub async fn clear(&self) -> HistoryResult<()> {
        self.run(State::clear).await
    }

    pub(super) async fn summary(&self, summary: HistorySummary) -> HistoryResult<()> {
        self.run(move |state| {
            let _lock = state.writer_lock()?;
            state.meta.summary = Some(summary);
            state.meta.status = SyncStatus::InProgress;
            state.save()
        })
        .await
    }

    pub(super) async fn directory(&self, snapshot: DirectorySnapshot) -> HistoryResult<()> {
        self.run(move |state| {
            let _lock = state.writer_lock()?;
            let range = snapshot.range;
            state
                .meta
                .directories
                .retain(|old| !(range.from <= old.range.from && range.to >= old.range.to));
            state.meta.directories.push_front(snapshot);
            state.meta.directories.truncate(MAX_DIRECTORIES);
            state.save()
        })
        .await
    }

    pub(super) async fn status(&self, status: SyncStatus) -> HistoryResult<()> {
        self.run(move |state| {
            let _lock = state.writer_lock()?;
            if let SyncStatus::Synced { at_ms } = status {
                state.meta.last_sync_ms = Some(at_ms);
            }
            state.meta.status = status;
            state.save()
        })
        .await
    }

    pub(super) async fn offset(&self, day: DayEntry) -> HistoryResult<u64> {
        self.run(move |state| {
            let key = (day.day, day.day_id);
            let lock = state.writer_lock()?;
            if state.root.is_some() && state.prefixes.get(&key).is_none_or(|p| p.memory.is_none()) {
                state.reload_if_changed(key)?;
            }
            drop(lock);
            Ok(state.prefixes.get(&key).map_or(0, |p| p.decoder.offset))
        })
        .await
    }

    pub(super) async fn append(
        &self,
        day: DayEntry,
        after: u64,
        through: u64,
        page: PacketPage,
    ) -> HistoryResult<()> {
        self.run(move |state| {
            if page.tail_cursor.is_some() {
                return Err(HistoryError::Protocol("封存页带有尾部游标"));
            }
            let bytes = page_bytes(&page, after, through)?;
            state.append(day, after, bytes)
        })
        .await
    }

    pub(super) async fn publish_tail(
        &self,
        cursor: TailCursor,
        bytes: Vec<u8>,
    ) -> HistoryResult<()> {
        self.run(move |state| {
            if bytes.len() as u64 > MAX_TAIL_BYTES
                || cursor.offset != cursor.through
                || cursor.through != bytes.len() as u64
            {
                return Err(HistoryError::Limit("尾部快照大小或边界"));
            }
            if !bytes.is_empty() {
                Decoder::new(&bytes, FileKind::Wal, None)?.all(&bytes[HEADER as usize..], None)?;
            }
            let previous = state.committed_tail;
            state
                .tails
                .retain(|old| old.cursor.wal_generation != cursor.wal_generation);
            state.tails.push_front(Tail { cursor, bytes });
            state.tails.retain(|tail| {
                tail.cursor.wal_generation == cursor.wal_generation
                    || Some(tail.cursor.wal_generation) == previous
            });
            state.tails.truncate(MAX_TAIL_SNAPSHOTS);
            Ok(())
        })
        .await
    }

    pub(super) async fn commit_tail(&self, cursor: TailCursor) -> HistoryResult<()> {
        self.run(move |state| {
            if state.tails.front().is_some_and(|t| t.cursor == cursor) {
                state.committed_tail = Some(cursor.wal_generation);
                // 封存页已覆盖旧快照后丢弃残留点，以免已淘汰数据继续显示为尾部。
                state.tails.truncate(1);
            }
            Ok(())
        })
        .await
    }

    pub(super) async fn prune(&self) -> HistoryResult<()> {
        self.run(|state| state.prune()).await
    }

    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut State) -> HistoryResult<T> + Send + 'static,
    ) -> HistoryResult<T> {
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || f(&mut state.lock().unwrap_or_else(|e| e.into_inner())))
            .await?
    }
}

#[cfg(test)]
pub(super) fn test_cache(
    root: Option<PathBuf>,
    store_id: Id,
    policy: CachePolicy,
    online: bool,
) -> HistoryResult<HistoryCache> {
    HistoryCache::with_root(
        root,
        Some("history-test:1234".into()),
        store_id,
        policy,
        online,
    )
}

fn lock_partition(root: &Path) -> HistoryResult<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("writer.lock"))?;
    file.lock()?;
    Ok(file)
}

fn reader_lock(root: &Path) -> HistoryResult<Option<File>> {
    let file = match File::open(root.join("writer.lock")) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    file.lock_shared()?;
    Ok(Some(file))
}

impl State {
    fn warn(&mut self, msg: String) {
        if self.warnings.len() >= MAX_WARNINGS {
            self.warnings.pop_front();
        }
        self.warnings.push_back(msg);
    }

    fn writer_lock(&self) -> HistoryResult<Option<File>> {
        self.root.as_deref().map(lock_partition).transpose()
    }

    fn save(&mut self) -> HistoryResult<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        while serde_json::to_vec(&self.meta)?.len() as u64 > MANIFEST_LIMIT
            && self.meta.directories.len() > 1
        {
            self.meta.directories.pop_back();
        }
        save_meta(root, &self.meta)
    }

    fn quarantine(&mut self, key: DayKey, why: &str, writable: bool) -> HistoryResult<()> {
        self.remove_prefix(key);
        self.warn(format!("已隔离 UTC 日 {} 的本地副本：{why}", key.0));
        if writable && let Some(root) = &self.root {
            let source = root.join(day_name(key));
            let target = root.join(format!("{}.bad", day_name(key)));
            match fs::rename(source, target) {
                Ok(()) => sync_dir(root)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn remove_prefix(&mut self, key: DayKey) {
        if let Some(prefix) = self.prefixes.remove(&key)
            && let Some(bytes) = prefix.memory
        {
            self.memory_bytes = self.memory_bytes.saturating_sub(bytes.len() as u64);
            self.memory_lru.retain(|old| *old != key);
        }
    }

    fn reload_if_changed(&mut self, key: DayKey) -> HistoryResult<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        match fs::metadata(root.join(day_name(key))) {
            Ok(meta) => {
                if self.prefixes.get(&key).is_none_or(|p| {
                    p.decoder.offset != meta.len() || p.modified != meta.modified().ok()
                }) {
                    self.reload(key, true)?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => self.remove_prefix(key),
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    fn reload(&mut self, key: DayKey, writable: bool) -> HistoryResult<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        let file = OpenOptions::new()
            .read(true)
            .write(writable)
            .open(root.join(day_name(key)))?;
        match scan(&file, key, writable) {
            Ok(Some(decoder)) => {
                self.remove_prefix(key);
                self.prefixes.insert(
                    key,
                    Prefix {
                        decoder,
                        memory: None,
                        modified: file.metadata()?.modified().ok(),
                    },
                );
            }
            Ok(None) => {
                drop(file);
                self.quarantine(key, "日文件头不完整", writable)?;
            }
            Err(
                e @ (HistoryError::Format(_) | HistoryError::Protocol(_) | HistoryError::Limit(_)),
            ) => {
                drop(file);
                self.quarantine(key, &e.to_string(), writable)?;
            }
            Err(e) => return Err(e),
        }
        Ok(())
    }

    fn disk_day(&self, day: i32) -> bool {
        let discovered = day_of(now_ms()).unwrap_or(i32::MAX);
        self.root.is_some()
            && (self.policy.retention_days == 0
                || day
                    >= discovered.saturating_sub(
                        self.policy
                            .retention_days
                            .saturating_sub(1)
                            .min(i32::MAX as u32) as i32,
                    ))
    }

    fn append(&mut self, day: DayEntry, after: u64, bytes: Vec<u8>) -> HistoryResult<()> {
        let key = (day.day, day.day_id);
        let lock = self.writer_lock()?;
        if self.root.is_some() && self.prefixes.get(&key).is_none_or(|p| p.memory.is_none()) {
            self.reload_if_changed(key)?;
        }
        let end = after
            .checked_add(bytes.len() as u64)
            .ok_or(HistoryError::Limit("日文件页终点"))?;
        let expected = DayHeader {
            utc_day: day.day,
            day_id: day.day_id,
        };
        let current = self.prefixes.get(&key).map_or(0, |p| p.decoder.offset);
        if after > current {
            return Err(HistoryError::Protocol("日文件页与本地前缀之间有空洞"));
        }
        if after < current {
            // 重复页只在已验证的包边界内且每一字节相同才能跳过。
            if end > current || !self.matches(key, after, &bytes)? {
                self.quarantine(key, "重复页与已验证字节不一致", true)?;
                return Err(HistoryError::Protocol("重复页内容不匹配"));
            }
            return Ok(());
        }
        if after == end {
            return Ok(());
        }
        let use_disk = self.prefixes.get(&key).map_or_else(
            || {
                self.disk_day(day.day)
                    && (self.policy.retention_bytes == 0
                        || day.published_through <= self.policy.retention_bytes)
            },
            |p| p.memory.is_none(),
        );
        if !use_disk && end > self.policy.memory_bytes {
            return Err(HistoryError::Limit("单个日文件超过内存缓存容量"));
        }
        let mut prefix = if let Some(prefix) = self.prefixes.remove(&key) {
            prefix
        } else {
            if after != 0 {
                return Err(HistoryError::Protocol("续传缺少有效日文件头"));
            }
            Prefix {
                decoder: Decoder::new(&bytes, FileKind::Day, Some(expected))?,
                memory: (!use_disk).then(Vec::new),
                modified: None,
            }
        };
        let payload = if after == 0 {
            &bytes[HEADER as usize..]
        } else {
            &bytes
        };
        if let Err(error) = prefix.decoder.all(payload, None) {
            // 校验失败不得推进游标；恢复原始有效字节的上下文。
            if let Some(buffer) = prefix.memory.take() {
                if !buffer.is_empty() {
                    let mut decoder = Decoder::new(&buffer, FileKind::Day, Some(expected))?;
                    decoder.all(&buffer[HEADER as usize..], None)?;
                    self.prefixes.insert(
                        key,
                        Prefix {
                            decoder,
                            memory: Some(buffer),
                            modified: None,
                        },
                    );
                }
            } else if current != 0 {
                self.reload(key, true)?;
            }
            return Err(error);
        }
        if prefix.decoder.offset != end {
            return Err(HistoryError::Protocol("页终点不是完整包边界"));
        }
        if let Some(buffer) = &mut prefix.memory {
            while self.memory_bytes.saturating_add(bytes.len() as u64) > self.policy.memory_bytes {
                let Some(old) = self.memory_lru.pop_front() else {
                    break;
                };
                if old != key {
                    self.remove_prefix(old);
                }
            }
            buffer.extend_from_slice(&bytes);
            self.memory_bytes += bytes.len() as u64;
            self.memory_lru.retain(|old| *old != key);
            self.memory_lru.push_back(key);
        } else {
            let root = self.root.as_ref().expect("磁盘缓存需要分区目录");
            let path = root.join(day_name(key));
            let write = (|| -> HistoryResult<Option<std::time::SystemTime>> {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create_new(after == 0)
                    .open(path)?;
                file.seek(SeekFrom::Start(after))?;
                if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
                    let _ = file.set_len(after).and_then(|_| file.sync_all());
                    return Err(error.into());
                }
                if after == 0 {
                    sync_dir(root)?;
                }
                Ok(file.metadata()?.modified().ok())
            })();
            prefix.modified = write?;
        }
        self.prefixes.insert(key, prefix);
        drop(lock);
        Ok(())
    }

    fn matches(&self, key: DayKey, after: u64, bytes: &[u8]) -> HistoryResult<bool> {
        let prefix = &self.prefixes[&key];
        let end = after + bytes.len() as u64;
        if let Some(buffer) = &prefix.memory {
            let mut reader = io::Cursor::new(buffer);
            return matches_prefix(&mut reader, after, end, bytes);
        }
        let mut file = File::open(self.root.as_ref().unwrap().join(day_name(key)))?;
        matches_prefix(&mut file, after, end, bytes)
    }

    fn clear(&mut self) -> HistoryResult<()> {
        let _lock = self.writer_lock()?;
        self.prefixes.clear();
        self.tails.clear();
        self.committed_tail = None;
        self.memory_lru.clear();
        self.memory_bytes = 0;
        self.warnings.clear();
        self.meta = Meta {
            version: 1,
            address: std::mem::take(&mut self.meta.address),
            ..Meta::default()
        };
        if let Some(root) = &self.root {
            for entry in fs::read_dir(root)? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if day_key(&path).is_some()
                    || (name.ends_with(".rhh.bad") && day_key(&path.with_extension("")).is_some())
                    || (name.starts_with("history-") && name.ends_with(".tmp"))
                    || name == "history.v1.json"
                {
                    fs::remove_file(path)?;
                }
            }
            sync_dir(root)?;
        }
        Ok(())
    }

    fn prune(&mut self) -> HistoryResult<()> {
        let Some(root) = self.root.clone() else {
            return Ok(());
        };
        let _lock = lock_partition(&root)?;
        let mut disk = BTreeMap::<i32, Vec<(PathBuf, Option<DayKey>, u64)>>::new();
        for (i, entry) in fs::read_dir(&root)?.enumerate() {
            if i >= MAX_PARTITION_FILES {
                return Err(HistoryError::Limit("离线分区文件数"));
            }
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            let key = day_key(&path).or_else(|| {
                (path.extension().and_then(|v| v.to_str()) == Some("bad"))
                    .then(|| day_key(&path.with_extension("")))
                    .flatten()
            });
            let Some(key) = key else {
                continue;
            };
            disk.entry(key.0).or_default().push((
                path.clone(),
                day_key(&path),
                entry.metadata()?.len(),
            ));
        }
        let mut size = disk
            .values()
            .flatten()
            .fold(0u64, |sum, (_, _, len)| sum.saturating_add(*len));
        for (day, entries) in disk {
            if self.disk_day(day)
                && (self.policy.retention_bytes == 0 || size <= self.policy.retention_bytes)
            {
                break;
            }
            for (path, key, len) in entries {
                if let Some(key) = key {
                    self.remove_prefix(key);
                }
                fs::remove_file(path)?;
                size = size.saturating_sub(len);
            }
        }
        sync_dir(&root)?;
        Ok(())
    }
}

fn load_packet(file: &mut impl Read, remaining: u64, bytes: &mut Vec<u8>) -> HistoryResult<bool> {
    if remaining < PACKET_HEADER_LEN as u64 {
        return Ok(false);
    }
    bytes.resize(PACKET_HEADER_LEN, 0);
    file.read_exact(bytes)?;
    let len = packet_len(bytes)?;
    if len as u64 > remaining {
        return Ok(false);
    }
    bytes.resize(len, 0);
    file.read_exact(&mut bytes[PACKET_HEADER_LEN..])?;
    Ok(true)
}

fn scan(mut file: &File, (day, day_id): DayKey, writable: bool) -> HistoryResult<Option<Decoder>> {
    let size = file.metadata()?.len();
    if size < HEADER {
        return Ok(None);
    }
    let mut header = [0; HEADER as usize];
    file.read_exact(&mut header)?;
    let mut decoder = Decoder::new(
        &header,
        FileKind::Day,
        Some(DayHeader {
            utc_day: day,
            day_id,
        }),
    )?;
    let mut packet = Vec::new();
    while decoder.offset < size {
        if !load_packet(&mut file, size - decoder.offset, &mut packet)? {
            // 崩溃留下的半个包不得发布，完整坏包则由调用方隔离。
            if writable {
                file.set_len(decoder.offset)?;
                file.sync_all()?;
            }
            break;
        }
        decoder.packet(&packet, None)?;
    }
    Ok(Some(decoder))
}

fn matches_prefix(
    reader: &mut (impl Read + Seek),
    after: u64,
    end: u64,
    bytes: &[u8],
) -> HistoryResult<bool> {
    let mut pos = if after == 0 { 0 } else { HEADER };
    let mut head = [0u8; PACKET_HEADER_LEN];
    while pos < after {
        reader.seek(SeekFrom::Start(pos))?;
        reader.read_exact(&mut head)?;
        pos += packet_len(&head)? as u64;
    }
    if pos != after || (after == 0 && end < HEADER) {
        return Ok(false);
    }
    let mut packets = if after == 0 {
        &bytes[HEADER as usize..]
    } else {
        bytes
    };
    while !packets.is_empty() {
        let Ok((_, count)) = ::history::decode_packet(packets) else {
            return Ok(false);
        };
        packets = &packets[count..];
    }
    reader.seek(SeekFrom::Start(after))?;
    let mut existing = vec![0; bytes.len()];
    reader.read_exact(&mut existing)?;
    Ok(bytes == existing)
}

impl State {
    fn local_extent(&self) -> Option<TimeRange> {
        let mut extent = None;
        for p in self.prefixes.values() {
            for r in [p.decoder.data_range, p.decoder.gap_range]
                .into_iter()
                .flatten()
            {
                extend_range(&mut extent, r.from, r.to);
            }
        }
        extent
    }

    fn query(&mut self, range: TimeRange, margin: i64) -> HistoryResult<HistoryQuery> {
        let _reader = self.root.as_deref().map(reader_lock).transpose()?;
        let mut result = HistoryQuery {
            summary: self.meta.summary.clone(),
            directories: self.meta.directories.iter().cloned().collect(),
            local_extent: self.local_extent(),
            status: self.meta.status.clone(),
            warnings: self.warnings.iter().cloned().collect(),
            tail_through: self.tails.front().map(|tail| tail.cursor),
            ..HistoryQuery::default()
        };
        let mut remote = BTreeMap::new();
        for directory in &self.meta.directories {
            for entry in &directory.entries {
                // 范围不同的较早目录不能覆盖较新的代号或淘汰状态。
                remote.entry(entry_day(entry)).or_insert(entry);
            }
        }
        let is_current = |&(day, day_id): &DayKey| -> bool {
            match remote.get(&day) {
                Some(DirectoryEntry::Available(entry)) => entry.day_id == day_id,
                _ => false,
            }
        };
        let mut playback = Playback::new(range);
        let mut keys: Vec<_> = self.prefixes.keys().copied().collect();
        // 先读目录确认的当前副本，再读旧代号；主机断网时仍读得到所有本地副本。
        keys.sort_by_key(|key| {
            (
                !is_current(key),
                std::cmp::Reverse(self.prefixes[key].modified),
                *key,
            )
        });
        let expanded = TimeRange {
            from: range.from.saturating_sub(margin),
            to: range.to.saturating_add(margin),
        };
        for key @ (day, day_id) in keys {
            let prefix = &self.prefixes[&key];
            let current = is_current(&key);
            let published = match remote.get(&day) {
                Some(DirectoryEntry::Available(entry)) if current => Some(entry.published_through),
                _ => None,
            };
            let offset = prefix.decoder.offset;
            let data_range = prefix.decoder.data_range;
            let gap_range = prefix.decoder.gap_range;
            result.cache_bytes = result.cache_bytes.saturating_add(offset);
            let owner = TimeRange {
                from: i64::from(day) * DAY_MS,
                to: (i64::from(day) + 1) * DAY_MS,
            };
            if !(owner.intersects(expanded)
                && (margin == DAY_MS || data_range.is_some_and(|data| data.intersects(expanded))))
                && !gap_range.is_some_and(|gap| gap.intersects(range))
            {
                result.coverage.push(LocalCoverage {
                    day,
                    day_id,
                    cached_through: offset,
                    published_through: published,
                    data_range,
                    gap_range,
                    durable: prefix.memory.is_none(),
                    current,
                });
                continue;
            }
            let coverage = LocalCoverage {
                day,
                day_id,
                cached_through: offset,
                published_through: published,
                data_range,
                gap_range,
                durable: prefix.memory.is_none(),
                current,
            };
            let source = HistorySource::Sealed {
                day,
                day_id,
                current,
            };
            let play = if let Some(bytes) = &prefix.memory {
                Decoder::new(bytes, FileKind::Day, Some(prefix.decoder.header))?
                    .all(&bytes[HEADER as usize..], Some((&mut playback, source)))
            } else {
                play_file(
                    &self.root.as_ref().unwrap().join(day_name(key)),
                    prefix.decoder.header,
                    offset,
                    &mut playback,
                    source,
                )
            };
            if let Err(error) = play {
                // 查询不会推进或修复缓存；同步时重新校验并隔离坏副本。
                if result.warnings.len() < MAX_WARNINGS {
                    result
                        .warnings
                        .push(format!("无法播放 UTC 日 {day}：{error}"));
                }
                continue;
            }
            if published.is_some_and(|through| offset >= through) {
                playback.prefer_day(day, data_range, gap_range);
            }
            result.coverage.push(coverage);
        }
        for tail in &self.tails {
            if tail.bytes.is_empty() {
                continue;
            }
            let source = HistorySource::Tail {
                generation: tail.cursor.wal_generation,
            };
            Decoder::new(&tail.bytes, FileKind::Wal, None)?.all(
                &tail.bytes[HEADER as usize..],
                Some((&mut playback, source)),
            )?;
        }
        playback.finish(&mut result);
        Ok(result)
    }
}

fn play_file(
    path: &Path,
    header: DayHeader,
    through: u64,
    playback: &mut Playback,
    source: HistorySource,
) -> HistoryResult<()> {
    let mut file = File::open(path)?;
    let mut bytes = [0; HEADER as usize];
    file.read_exact(&mut bytes)?;
    let mut decoder = Decoder::new(&bytes, FileKind::Day, Some(header))?;
    let mut packet = Vec::new();
    while decoder.offset < through {
        if !load_packet(&mut file, through - decoder.offset, &mut packet)? {
            return Err(::history::Error::Truncated.into());
        }
        decoder.packet(&packet, Some((playback, source)))?;
    }
    Ok(())
}

impl HistoryCache {
    pub fn store_id(&self) -> Id {
        self.store_id
    }

    pub(super) async fn tail_days(&self) -> HistoryResult<Vec<i32>> {
        self.run(|state| {
            state
                .tails
                .iter()
                .filter(|tail| !tail.bytes.is_empty())
                .map(|tail| Ok(DayHeader::decode(&tail.bytes)?.utc_day))
                .collect()
        })
        .await
    }
}
