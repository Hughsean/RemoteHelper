use std::{collections::HashSet, sync::Mutex, time::Duration};

use common::{
    Request, Response,
    history_api::{
        DayEntry, DirectoryCursor, HISTORY_FRAME_LIMIT, HistoryRequest, HistoryResponse,
        PacketPage, TailCursor,
    },
    server_auth::{HISTORY_VERSION, ServerIdentity},
};
use ed25519_dalek::SigningKey;
use tokio::{
    sync::watch,
    time::{sleep, timeout},
};

use super::{decode::page_bytes, *};
use crate::EncryptedConnection;

const IO_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESTARTS: usize = 4;
const DIRECTORY_PAGE: usize = 128;
const ADJACENT_MS: i64 = 60_000;

/// 生命周期绑定到单个登录目标。主机切换时取消运行任务并丢弃此对象。
pub struct HistorySync {
    address: String,
    fingerprint: String,
    instance_id: Option<Id>,
    signing_key: SigningKey,
    public_key: String,
    policy: CachePolicy,
    active: Mutex<Option<HistoryCache>>,
    serial: tokio::sync::Mutex<()>,
    settings: Arc<tokio::sync::Mutex<Option<SettingsLink>>>,
    updates: watch::Sender<u64>,
}

struct SettingsLink {
    link: Link,
    deadline: tokio::time::Instant,
}

struct Link {
    transport: Option<(EncryptedConnection, Vec<u8>)>,
}

impl HistorySync {
    /// 使用当前已持久固定的指纹建立独立同步连接，不接触实时连接。
    pub fn new(
        address: String,
        fingerprint: String,
        signing_key: SigningKey,
        public_key: String,
        policy: CachePolicy,
    ) -> HistoryResult<Self> {
        let address =
            crate::server_pins::canonical_address(&address).map_err(HistoryError::Connection)?;
        crate::server_pins::validate_fingerprint(&fingerprint).map_err(HistoryError::Connection)?;
        policy.validate()?;
        Ok(Self {
            address,
            fingerprint,
            instance_id: None,
            signing_key,
            public_key,
            policy,
            active: Mutex::new(None),
            serial: tokio::sync::Mutex::new(()),
            settings: Arc::new(tokio::sync::Mutex::new(None)),
            updates: watch::channel(0).0,
        })
    }

    pub fn from_verified(
        address: String,
        identity: &ServerIdentity,
        signing_key: SigningKey,
        public_key: String,
        policy: CachePolicy,
    ) -> HistoryResult<Self> {
        if identity.history_version != HISTORY_VERSION {
            return Err(HistoryError::Protocol("未支持的历史协商版本"));
        }
        let mut sync = Self::new(
            address,
            identity.fingerprint.clone(),
            signing_key,
            public_key,
            policy,
        )?;
        sync.instance_id = Some(identity.instance_id);
        Ok(sync)
    }

    pub fn active_cache(&self) -> Option<HistoryCache> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub async fn synchronize(&self, range: TimeRange) -> HistoryResult<HistoryQuery> {
        self.refresh(Some(range)).await?.query(range).await
    }

    /// 使用最新摘要同步尾部及发布数据，先处理可见范围，再按需预取。
    pub async fn refresh(&self, visible: Option<TimeRange>) -> HistoryResult<HistoryCache> {
        if let Some(range) = visible {
            check_range(range)?;
        }
        let _serial = self.serial.lock().await;
        for attempt in 0..=MAX_RESTARTS {
            match self.refresh_once(visible).await {
                Ok(cache) => {
                    self.notify_update();
                    return Ok(cache);
                }
                Err(e) if attempt < MAX_RESTARTS && retryable(&e) => {
                    // 新连接继续读取已经同步到磁盘的前缀；繁忙时让实时连接优先。
                    sleep(Duration::from_millis(200 * (1_u64 << attempt))).await;
                }
                Err(error) => {
                    if let Some(cache) = self.active_cache() {
                        let _ = cache
                            .status(SyncStatus::Failed {
                                at_ms: now_ms(),
                                reason: error.to_string().chars().take(512).collect(),
                            })
                            .await;
                    }
                    self.notify_update();
                    return Err(error);
                }
            }
        }
        unreachable!()
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.updates.subscribe()
    }
    fn notify_update(&self) {
        self.updates.send_modify(|v| *v = v.wrapping_add(1));
    }

    pub async fn clear_cache(&self) -> HistoryResult<()> {
        let _serial = self.serial.lock().await;
        if let Some(cache) = self.active_cache() {
            cache.clear().await?;
        }
        self.notify_update();
        Ok(())
    }

    pub async fn clear_cache_partition(&self, cache: HistoryCache) -> HistoryResult<()> {
        let _serial = self.serial.lock().await;
        cache.clear().await?;
        self.notify_update();
        Ok(())
    }

    /// 调用方拥有这个 future；取消、关闭可见区间通道或更换主机即停止网络同步。
    pub async fn run(&self, mut visible: watch::Receiver<Option<TimeRange>>) {
        let mut delay = Duration::ZERO;
        loop {
            let range = *visible.borrow_and_update();
            let refresh = async {
                sleep(delay).await;
                self.refresh(range).await
            };
            tokio::select! {
                changed = visible.changed() => {
                    if changed.is_err() { return; }
                    delay = Duration::from_millis(150);
                }
                result = refresh => {
                    if let Err(error) = result { log::debug!("历史刷新失败：{error}"); }
                    delay = self.policy.refresh_interval;
                }
            }
        }
    }

    /// 管理操作不自动重放；写入超时后应先重新读取设置或时钟状态。
    pub async fn request_settings(
        &self,
        request: HistoryRequest,
    ) -> HistoryResult<HistoryResponse> {
        if !matches!(
            request,
            HistoryRequest::GetSettings
                | HistoryRequest::SetSettings { .. }
                | HistoryRequest::AckClock { .. }
        ) {
            return Err(HistoryError::Protocol("不是历史管理请求"));
        }
        let mut slot = self.settings.lock().await;
        let address = self.address.clone();
        let current_pin = tokio::task::spawn_blocking(move || crate::load_server_pin(&address))
            .await?
            .map_err(HistoryError::Connection)?;
        if current_pin.as_deref() != Some(&self.fingerprint) {
            *slot = None;
            return Err(HistoryError::Connection("服务器固定指纹已改变".into()));
        }
        let now = tokio::time::Instant::now();
        let mut link = match slot.take().filter(|old| old.deadline > now) {
            Some(old) => old.link,
            None if matches!(request, HistoryRequest::AckClock { .. }) => {
                return Err(HistoryError::Remote {
                    code: HistoryErrorCode::ConfirmationExpired,
                    detail: "请重新读取时钟状态".into(),
                });
            }
            None => self.connect().await?,
        };
        let response = link.request(request.clone()).await;
        // 有效的错误响应不会破坏通道；超时、取消或非法帧不归还连接。
        if matches!(response, Ok(_) | Err(HistoryError::Remote { .. })) && link.transport.is_some()
        {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
            *slot = Some(SettingsLink { link, deadline });
            let weak = Arc::downgrade(&self.settings);
            tokio::spawn(async move {
                tokio::time::sleep_until(deadline).await;
                if let Some(settings) = weak.upgrade() {
                    let mut settings = settings.lock().await;
                    if settings
                        .as_ref()
                        .is_some_and(|link| link.deadline <= tokio::time::Instant::now())
                    {
                        *settings = None;
                    }
                }
            });
        }
        let response = response?;
        match (&request, &response) {
            (HistoryRequest::GetSettings, HistoryResponse::Settings(_))
            | (
                HistoryRequest::SetSettings { .. },
                HistoryResponse::Settings(_) | HistoryResponse::SettingsConflict(_),
            )
            | (HistoryRequest::AckClock { .. }, HistoryResponse::Clock(_)) => Ok(response),
            _ => Err(HistoryError::Protocol("历史管理响应类型与请求不符")),
        }
    }

    async fn connect(&self) -> HistoryResult<Link> {
        let conn = timeout(
            IO_TIMEOUT,
            crate::connect_verified(
                &self.address,
                &self.fingerprint,
                &self.signing_key,
                &self.public_key,
            ),
        )
        .await
        .map_err(|_| HistoryError::Connection("历史登录超时".into()))?
        .map_err(HistoryError::Connection)?;
        let identity = conn
            .server_identity()
            .ok_or(HistoryError::Protocol("历史连接未协商身份"))?;
        if identity.fingerprint != self.fingerprint
            || identity.history_version != HISTORY_VERSION
            || self
                .instance_id
                .is_some_and(|id| id != identity.instance_id)
        {
            return Err(HistoryError::Protocol("历史连接与主会话的身份不符"));
        }
        Ok(Link {
            transport: Some((conn, vec![0; HISTORY_FRAME_LIMIT])),
        })
    }

    async fn refresh_once(&self, visible: Option<TimeRange>) -> HistoryResult<HistoryCache> {
        let mut link = self.connect().await?;
        let summary = link.summary().await?;
        if let Some(range) = summary.range {
            check_range(range)?;
        }
        let Some(store_id) = summary.store_id else {
            return Err(HistoryError::Remote {
                code: HistoryErrorCode::Starting,
                detail: format!("没有可用的历史存储标识：{:?}", summary.storage),
            });
        };
        let cache = if let Some(cache) = self
            .active_cache()
            .filter(|cache| cache.store_id() == store_id)
        {
            cache
        } else {
            let cache = HistoryCache::open_online(
                self.address.clone(),
                self.fingerprint.clone(),
                store_id,
                self.policy,
            )
            .await?;
            *self.active.lock().unwrap_or_else(|e| e.into_inner()) = Some(cache.clone());
            cache
        };
        cache.summary(summary).await?;
        let prior_tail_days = cache.tail_days().await?;
        // 必须先打开尾部，再读目录。否则封存或换日可能在两者之间移走样本。
        let (tail, buffer, tail_day) = pull_tail(&mut link).await?;
        cache.publish_tail(tail, buffer).await?;
        let now = now_ms();
        let prefetch = (self.policy.prefetch_days != 0).then_some(TimeRange {
            from: now.saturating_sub(i64::from(self.policy.prefetch_days) * DAY_MS),
            to: now.saturating_add(1),
        });
        let ranges = planned_ranges(visible, prefetch, prior_tail_days, tail_day);
        let mut done = HashSet::new();
        let mut limited = false;
        for range in ranges {
            let snapshot = pull_directory(&mut link, range).await?;
            let mut days: Vec<_> = snapshot
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    DirectoryEntry::Available(e) => Some(e.clone()),
                    _ => None,
                })
                .collect();
            cache.directory(snapshot).await?;
            // 最近的日期先完成；不同 UTC 日的游标始终独立。
            days.sort_by_key(|e| std::cmp::Reverse(e.day));
            for day in days {
                if done.insert((day.day, day.day_id, day.published_through)) {
                    match pull_day(&mut link, &cache, day).await {
                        Err(HistoryError::Limit("单个日文件超过内存缓存容量")) => {
                            limited = true
                        }
                        result => result?,
                    }
                }
            }
        }
        // 以已完成游标探测读视图是否仍有效，不重新打开就无法检测此轮中途封存。
        let probe = link.tail(Some(tail)).await?;
        let probed = validate_tail_cursor(&probe, Some(tail))?;
        if probed != tail || !probe.bytes.is_empty() {
            return Err(HistoryError::Protocol("尾部高水位校验返回了额外数据"));
        }
        drop(link);
        cache.commit_tail(tail).await?;
        cache.prune().await?;
        if limited {
            cache
                .status(SyncStatus::Failed {
                    at_ms: now_ms(),
                    reason: "部分日文件超过内存缓存容量，已保留可用前缀".into(),
                })
                .await?;
        } else {
            cache.status(SyncStatus::Synced { at_ms: now_ms() }).await?;
        }
        Ok(cache)
    }
}

impl Link {
    async fn request(&mut self, request: HistoryRequest) -> HistoryResult<HistoryResponse> {
        // 请求超时或取消会令调用方丢弃整个连接，避免把上一请求的响应当作下一页。
        let response = timeout(IO_TIMEOUT, async {
            let request = Request::History(request);
            if serde_json::to_vec(&request)?.len() + 16 > 10 * 1024 {
                return Err(HistoryError::Limit("历史请求帧"));
            }
            let (mut conn, buffer) = self
                .transport
                .take()
                .ok_or_else(|| HistoryError::Connection("历史连接已关闭".into()))?;
            conn.send(&request)
                .await
                .map_err(HistoryError::Connection)?;
            let (conn, buffer, response) = conn
                .recv_history(buffer)
                .await
                .map_err(HistoryError::Connection)?;
            self.transport = Some((conn, buffer));
            Ok(response)
        })
        .await
        .map_err(|_| HistoryError::Connection("等待历史响应超时".into()))??;
        match response {
            Response::History(HistoryResponse::Error(error)) => Err(HistoryError::Remote {
                code: error.code,
                detail: error.detail.chars().take(1024).collect(),
            }),
            Response::History(response) => Ok(response),
            Response::Error(detail) => Err(HistoryError::Connection(
                detail.chars().take(1024).collect(),
            )),
            _ => Err(HistoryError::Protocol("服务器未返回历史响应")),
        }
    }

    async fn summary(&mut self) -> HistoryResult<HistorySummary> {
        match self.request(HistoryRequest::Summary).await? {
            HistoryResponse::Summary(value) => Ok(value),
            _ => Err(HistoryError::Protocol("摘要响应类型不符")),
        }
    }

    async fn tail(&mut self, cursor: Option<TailCursor>) -> HistoryResult<PacketPage> {
        match self.request(HistoryRequest::Tail { cursor }).await? {
            HistoryResponse::Tail(value) => Ok(value),
            _ => Err(HistoryError::Protocol("尾部响应类型不符")),
        }
    }
}

fn planned_ranges(
    visible: Option<TimeRange>,
    prefetch: Option<TimeRange>,
    prior_days: Vec<i32>,
    tail_day: Option<i32>,
) -> Vec<TimeRange> {
    let mut ranges: Vec<TimeRange> = Vec::new();
    for range in visible
        .map(|v| TimeRange {
            from: v.from.saturating_sub(ADJACENT_MS),
            to: v.to.saturating_add(ADJACENT_MS),
        })
        .into_iter()
        .chain(prior_days.into_iter().chain(tail_day).map(|day| {
            let from = i64::from(day) * DAY_MS;
            TimeRange {
                from,
                to: from + DAY_MS,
            }
        }))
        .chain(prefetch)
    {
        if !ranges
            .iter()
            .any(|old| old.from <= range.from && old.to >= range.to)
        {
            ranges.push(range);
        }
    }
    ranges
}

fn retryable(error: &HistoryError) -> bool {
    match error {
        HistoryError::Connection(_) => true,
        HistoryError::Remote { code, .. } => matches!(
            code,
            HistoryErrorCode::Busy
                | HistoryErrorCode::DirectoryRestartRequired
                | HistoryErrorCode::TailRestartRequired
                | HistoryErrorCode::DayChanged
                | HistoryErrorCode::DayExpired
                | HistoryErrorCode::DayPendingDeletion
                | HistoryErrorCode::MissingDay
                | HistoryErrorCode::InvalidCursor
        ),
        HistoryError::Protocol("重复页内容不匹配") => true,
        _ => false,
    }
}

fn validate_tail_cursor(page: &PacketPage, prior: Option<TailCursor>) -> HistoryResult<TailCursor> {
    let cursor = page
        .tail_cursor
        .ok_or(HistoryError::Protocol("尾部分页缺少游标"))?;
    if cursor.offset != page.next_offset
        || cursor.through != page.through
        || cursor.offset > cursor.through
        || cursor.through > MAX_TAIL_BYTES
        || (cursor.through != 0 && cursor.through < HEADER)
        || page.offset != prior.map_or(0, |old| old.offset)
        || prior.is_some_and(|old| {
            cursor.wal_generation != old.wal_generation || cursor.through != old.through
        })
    {
        return Err(HistoryError::Protocol("尾部世代、偏移或快照终点不匹配"));
    }
    Ok(cursor)
}

async fn pull_tail(link: &mut Link) -> HistoryResult<(TailCursor, Vec<u8>, Option<i32>)> {
    let mut buffer = Vec::new();
    let mut cursor = None;
    loop {
        let page = link.tail(cursor).await?;
        let next = validate_tail_cursor(&page, cursor)?;
        // 每一页单独检查包边界；不能靠下一页补齐一个被截断的包。
        let (out, day) = tokio::task::spawn_blocking(move || -> HistoryResult<_> {
            let bytes = page_bytes(&page, cursor.map_or(0, |old| old.offset), next.through)?;
            let mut payload = bytes.as_slice();
            if cursor.is_none() && !payload.is_empty() {
                ::history::DayHeader::decode(payload)?;
                payload = &payload[HEADER as usize..];
            }
            while !payload.is_empty() {
                let (_, len) = ::history::decode_packet(payload)?;
                payload = &payload[len..];
            }
            buffer.extend(bytes);
            let day = if next.offset == next.through && !buffer.is_empty() {
                Some(::history::DayHeader::decode(&buffer)?.utc_day)
            } else {
                None
            };
            Ok((buffer, day))
        })
        .await??;
        buffer = out;
        if next.offset == next.through {
            return Ok((next, buffer, day));
        }
        cursor = Some(next);
    }
}

async fn pull_directory(link: &mut Link, range: TimeRange) -> HistoryResult<DirectorySnapshot> {
    let mut cursor: Option<DirectoryCursor> = None;
    let mut entries = Vec::new();
    let mut age_cutoff = None;
    let mut available = HashSet::new();
    loop {
        let HistoryResponse::Days(page) =
            link.request(HistoryRequest::Days { range, cursor }).await?
        else {
            return Err(HistoryError::Protocol("日目录响应类型不符"));
        };
        if page.entries.len() > DIRECTORY_PAGE
            || entries.len() + page.entries.len() > MAX_DIRECTORY_ENTRIES
        {
            return Err(HistoryError::Limit("日目录分页条目"));
        }
        if cursor.is_none() {
            age_cutoff = page.age_cutoff_day;
        }
        if age_cutoff != page.age_cutoff_day {
            return Err(HistoryError::Protocol("保留边界在目录快照中改变"));
        }
        if let Some(next) = page.next {
            if page.entries.is_empty()
                || next.offset as usize != entries.len() + page.entries.len()
                || cursor.is_some_and(|old| old.generation != next.generation)
            {
                return Err(HistoryError::Protocol("日目录游标不连续"));
            }
        }
        for entry in &page.entries {
            let (data, gap) = match entry {
                DirectoryEntry::Available(e) => {
                    if !available.insert(e.day) || e.published_through < HEADER {
                        return Err(HistoryError::Protocol("日目录的代号或发布边界无效"));
                    }
                    (e.data_range, e.gap_range)
                }
                DirectoryEntry::Pending(e) | DirectoryEntry::Evicted(e) => {
                    (e.data_range, e.gap_range)
                }
                DirectoryEntry::Unavailable { .. } => (None, None),
            };
            for coverage in [data, gap].into_iter().flatten() {
                check_range(coverage)?;
            }
        }
        entries.extend(page.entries);
        cursor = page.next;
        if cursor.is_none() {
            return Ok(DirectorySnapshot {
                range,
                entries,
                age_cutoff_day: age_cutoff,
            });
        }
    }
}

async fn pull_day(link: &mut Link, cache: &HistoryCache, day: DayEntry) -> HistoryResult<()> {
    let through = day.published_through;
    let mut after = cache.offset(day.clone()).await?;
    while after < through {
        let HistoryResponse::Blocks(page) = link
            .request(HistoryRequest::Blocks {
                day: day.day,
                day_id: day.day_id,
                after,
                through,
            })
            .await?
        else {
            return Err(HistoryError::Protocol("封存页响应类型不符"));
        };
        let next = page.next_offset;
        cache.append(day.clone(), after, through, page).await?;
        after = next;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, prelude::BASE64_STANDARD};
    use common::crypto::CryptoSession;
    use tokio::{
        io::AsyncWriteExt,
        net::{TcpListener, TcpStream},
    };

    async fn local_link() -> (Link, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (client, server) = tokio::join!(
            TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept()
        );
        let mut conn =
            EncryptedConnection::new(client.unwrap(), CryptoSession::new([6; 32], false));
        conn.server_identity = Some(ServerIdentity {
            fingerprint: "01".repeat(32),
            instance_id: [9; 16],
            history_version: HISTORY_VERSION,
        });
        (
            Link {
                transport: Some((conn, vec![0; HISTORY_FRAME_LIMIT])),
            },
            server.unwrap().0,
        )
    }

    #[tokio::test]
    async fn history_frame_limit_and_incomplete_response_discard_connection() {
        for (len, prefix) in [
            (HISTORY_FRAME_LIMIT as u32 + 1, &[][..]),
            (100, &[1, 2, 3][..]),
        ] {
            let (mut link, mut server) = local_link().await;
            let responder = tokio::spawn(async move {
                let mut crypto = CryptoSession::new([6; 32], true);
                let mut buffer = [0; 10240];
                let request =
                    common::transport::recv_frame::<Request>(&mut server, &mut crypto, &mut buffer)
                        .await
                        .unwrap();
                assert!(matches!(
                    request,
                    common::transport::FrameResult::Message(Request::History(
                        HistoryRequest::Summary
                    ))
                ));
                server.write_u32(len).await.unwrap();
                server.write_all(prefix).await.unwrap();
            });
            assert!(matches!(
                link.summary().await,
                Err(HistoryError::Connection(_))
            ));
            assert!(link.transport.is_none());
            responder.await.unwrap();
        }
    }

    #[test]
    fn tail_cursor_never_accepts_view_or_boundary_switch() {
        let cursor = TailCursor {
            wal_generation: [1; 16],
            offset: HEADER,
            through: 100,
        };
        let good = PacketPage {
            bytes: BASE64_STANDARD.encode([0; 70]),
            offset: HEADER,
            next_offset: 100,
            through: 100,
            tail_cursor: Some(TailCursor {
                offset: 100,
                ..cursor
            }),
        };
        assert!(validate_tail_cursor(&good, Some(cursor)).is_ok());
        let mut page = good.clone();
        page.tail_cursor.as_mut().unwrap().wal_generation = [2; 16];
        assert!(validate_tail_cursor(&page, Some(cursor)).is_err());
        page = good.clone();
        page.through += 1;
        assert!(validate_tail_cursor(&page, Some(cursor)).is_err());
        page = good;
        page.next_offset = HEADER;
        assert!(page_bytes(&page, HEADER, 100).is_err());
    }

    #[test]
    fn visible_has_precedence_and_previous_tail_day_is_refreshed_without_prefetch() {
        let visible = (1000, 2000).into();
        let ranges = planned_ranges(Some(visible), None, vec![2], Some(3));
        assert_eq!(
            ranges,
            vec![
                (-59000, 62000).into(),
                (2 * DAY_MS, 3 * DAY_MS).into(),
                (3 * DAY_MS, 4 * DAY_MS).into()
            ]
        );
    }
}
