use crate::ui::{field, panel};
use client::history::{CachePolicy, HistoryCache, HistoryError, HistorySync, SyncStatus};
use common::history_api::{
    ClockStatus, EditableHistory, HistoryRequest, HistoryResponse, Id, SettingsSnapshot,
    StorageStatus, TimeRange,
};
use gpui_kit::base::{Disableable, StyledExt};
use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::*;
use std::{
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use time::{OffsetDateTime, UtcOffset, macros::format_description};
use tokio::runtime::Handle;

const MIB: u64 = 1_048_576;
const DAY_MS: i64 = 86_400_000;
// 管理连接可在空闲满一分钟后关闭，给最终请求留出余量。
const CLOCK_REVIEW_LIMIT: Duration = Duration::from_secs(45);

#[derive(Clone, Copy)]
pub(crate) struct HistoryPreferences {
    pub(crate) cache: CachePolicy,
    pub(crate) trend_window: Duration,
}

impl Default for HistoryPreferences {
    fn default() -> Self {
        Self::from_cache(CachePolicy::default())
    }
}

impl HistoryPreferences {
    fn from_cache(cache: CachePolicy) -> Self {
        Self {
            trend_window: Duration::from_millis(cache.trend_window_ms),
            cache,
        }
    }

    pub(crate) async fn load() -> Result<Self, String> {
        CachePolicy::load()
            .await
            .map(Self::from_cache)
            .map_err(|error| error.to_string())
    }
}

pub(crate) enum HistorySettingsChange {
    Preferences(HistoryPreferences),
    ServerChanged,
    CacheCleared,
}

#[derive(Clone, Copy)]
pub(crate) enum HistorySection {
    Cache,
    Server,
}

type ChangeHandler = Rc<dyn Fn(HistorySettingsChange, &mut Window, &mut App)>;

const SERVER_FIELDS: [&str; 5] = [
    "空闲采样间隔（毫秒）",
    "最短采样间隔（毫秒）",
    "封存间隔（秒）",
    "保留天数（0 表示不限）",
    "保留上限（字节，0 表示不限）",
];
const CACHE_FIELDS: [&str; 4] = [
    "本地保留天数（0 表示不限）",
    "本地保留上限（字节，0 表示不限）",
    "预取天数（0 表示关闭自动预取）",
    "增量同步间隔（秒）",
];
const TREND_FIELD: &str = "趋势图默认窗口（秒）";

struct NumberFields<const N: usize>([Entity<InputState>; N]);

impl<const N: usize> NumberFields<N> {
    fn new(window: &mut Window, cx: &mut App) -> Self {
        Self(std::array::from_fn(|_| {
            cx.new(|cx| InputState::new(window, cx))
        }))
    }

    fn set(&self, values: [u64; N], window: &mut Window, cx: &mut App) {
        for (input, value) in self.0.iter().zip(values) {
            input.update(cx, |state, cx| {
                state.set_value(value.to_string(), window, cx)
            });
        }
    }

    fn values(&self, labels: [&str; N], cx: &App) -> Result<[u64; N], String> {
        let mut values = [0; N];
        for ((value, input), label) in values.iter_mut().zip(&self.0).zip(labels) {
            *value = input
                .read(cx)
                .value()
                .trim()
                .parse()
                .map_err(|_| format!("{label}：请输入有效的非负整数"))?;
        }
        Ok(values)
    }

    fn render(&self, labels: [&'static str; N], disabled: bool) -> impl IntoElement {
        v_flex().gap_2().children(
            labels
                .into_iter()
                .zip(&self.0)
                .map(|(label, input)| field(label, Input::new(input).disabled(disabled))),
        )
    }
}

struct ClockReview {
    clock: ClockStatus,
    client_utc: OffsetDateTime,
    started: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerOperation {
    Read,
    ReviewClock,
    Save,
    AckClock,
}

impl ServerOperation {
    fn mutates(self) -> bool {
        matches!(self, Self::Save | Self::AckClock)
    }
}

struct ServerResult {
    result: Result<HistoryResponse, HistoryError>,
    reread: Option<Result<(SettingsSnapshot, OffsetDateTime), String>>,
    client_utc: OffsetDateTime,
    received_at: Instant,
}

pub(crate) struct HistorySettings {
    runtime: Handle,
    section: HistorySection,
    public_key: String,
    sync: Option<Arc<HistorySync>>,
    offline_cache: Option<HistoryCache>,
    on_change: ChangeHandler,
    source_generation: u64,
    server_generation: u64,
    usage_generation: u64,
    snapshot: Option<SettingsSnapshot>,
    snapshot_time: Option<OffsetDateTime>,
    server_operation: Option<ServerOperation>,
    server_stale: bool,
    server_message: Option<String>,
    copied: bool,
    clock_review: Option<ClockReview>,
    server_review: Option<EditableHistory>,
    server_enabled: bool,
    server_fields: NumberFields<5>,
    preferences: HistoryPreferences,
    cache_enabled: bool,
    cache_fields: NumberFields<5>,
    cache_busy: bool,
    clear_review: Option<HistoryCache>,
    cache_message: Option<String>,
    cache_usage: Option<Result<(u64, SyncStatus), String>>,
}

impl HistorySettings {
    pub(crate) fn new(
        runtime: Handle,
        public_key: String,
        sync: Option<Arc<HistorySync>>,
        offline_cache: Option<HistoryCache>,
        preferences: HistoryPreferences,
        on_change: impl Fn(HistorySettingsChange, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cache_fields = NumberFields::new(window, cx);
        cache_fields.set(preference_values(preferences.cache), window, cx);
        Self {
            runtime,
            section: HistorySection::Cache,
            public_key,
            sync,
            offline_cache,
            on_change: Rc::new(on_change),
            source_generation: 0,
            server_generation: 0,
            usage_generation: 0,
            snapshot: None,
            snapshot_time: None,
            server_operation: None,
            server_stale: false,
            server_message: None,
            copied: false,
            clock_review: None,
            server_review: None,
            server_enabled: false,
            server_fields: NumberFields::new(window, cx),
            preferences,
            cache_enabled: preferences.cache.enabled,
            cache_fields,
            cache_busy: false,
            clear_review: None,
            cache_message: None,
            cache_usage: None,
        }
    }

    pub(crate) fn apply_preferences(
        &mut self,
        preferences: HistoryPreferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.cache_busy {
            return;
        }
        self.preferences = preferences;
        self.cache_enabled = preferences.cache.enabled;
        self.cache_fields
            .set(preference_values(preferences.cache), window, cx);
        cx.notify();
    }

    pub(crate) fn set_sources(
        &mut self,
        sync: Option<Arc<HistorySync>>,
        offline_cache: Option<HistoryCache>,
        cx: &mut Context<Self>,
    ) {
        let same_sync = match (&self.sync, &sync) {
            (Some(old), Some(new)) => Arc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if !same_sync {
            self.server_generation = self.server_generation.wrapping_add(1);
            self.server_operation = None;
            self.clock_review = None;
            self.server_review = None;
            self.server_stale = true;
            if sync.is_none() {
                self.snapshot = None;
                self.snapshot_time = None;
            }
        }
        if !same_sync || self.offline_cache.is_some() || offline_cache.is_some() {
            self.source_generation = self.source_generation.wrapping_add(1);
            self.clear_review = None;
            self.cache_usage = None;
        }
        self.sync = sync;
        self.offline_cache = offline_cache;
        cx.notify();
    }

    pub(crate) fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_usage(window, cx);
        self.run_server(
            ServerOperation::Read,
            HistoryRequest::GetSettings,
            window,
            cx,
        );
    }

    pub(crate) fn open_section(
        &mut self,
        section: HistorySection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.section = section;
        match section {
            HistorySection::Cache => self.refresh_usage(window, cx),
            HistorySection::Server if self.snapshot.is_none() || self.server_stale => {
                self.refresh(window, cx);
            }
            HistorySection::Server => cx.notify(),
        }
    }

    fn signal(&self, change: HistorySettingsChange, window: &mut Window, cx: &mut App) {
        let handler = self.on_change.clone();
        window.defer(cx, move |window, cx| handler(change, window, cx));
    }

    fn refresh_usage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.usage_generation = self.usage_generation.wrapping_add(1);
        let generation = (self.source_generation, self.usage_generation);
        let cache = self.cache();
        let Some(cache) = cache else {
            self.cache_usage = None;
            cx.notify();
            return;
        };
        let task = self.runtime.spawn(async move { cache.usage().await });
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = this.update_in(cx, |view, _, cx| {
                if generation == (view.source_generation, view.usage_generation) {
                    view.cache_usage = Some(result);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn cache(&self) -> Option<HistoryCache> {
        self.sync
            .as_ref()
            .and_then(|sync| sync.active_cache())
            .or_else(|| self.offline_cache.clone())
    }

    fn install_snapshot(
        &mut self,
        snapshot: SettingsSnapshot,
        recorded_at: OffsetDateTime,
        window: &mut Window,
        cx: &mut App,
    ) {
        let history = &snapshot.history;
        self.server_enabled = history.enabled;
        self.server_fields.set(
            [
                history.idle_interval_ms,
                history.min_interval_ms,
                history.seal_interval_secs,
                history.retention_days.into(),
                history.retention_bytes,
            ],
            window,
            cx,
        );
        self.snapshot = Some(snapshot);
        self.snapshot_time = Some(recorded_at);
        self.server_stale = false;
    }

    fn run_server(
        &mut self,
        operation: ServerOperation,
        request: HistoryRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.server_operation.is_some() || (operation.mutates() && self.cache_busy) {
            return;
        }
        let Some(sync) = self.sync.clone() else {
            return;
        };
        self.server_operation = Some(operation);
        self.server_message = None;
        self.server_review = None;
        self.clock_review = None;
        let generation = self.server_generation;
        let task = self.runtime.spawn(async move {
            let response = sync.request_settings(request).await;
            let received_at = Instant::now();
            let client_utc = OffsetDateTime::now_utc();
            let result = match response {
                Ok(HistoryResponse::Error(error)) => Err(HistoryError::Remote {
                    code: error.code,
                    detail: error.detail,
                }),
                response => response,
            };
            // 无论写入结果为何都不重放。连接或协议出错时只读回当前修订和时钟状态。
            let reread = match (operation, &result) {
                (ServerOperation::AckClock, _) | (ServerOperation::Save, Err(_)) => Some(
                    read_snapshot(&sync)
                        .await
                        .map(|snapshot| (snapshot, OffsetDateTime::now_utc())),
                ),
                _ => None,
            };
            ServerResult {
                result,
                reread,
                client_utc,
                received_at,
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |view, window, cx| {
                if generation != view.server_generation {
                    return;
                }
                view.server_operation = None;
                match result {
                    Ok(result) => view.finish_server(operation, result, window, cx),
                    Err(error) => {
                        view.server_stale = true;
                        view.server_message =
                            Some(format!("服务器请求未完成：{error}。请刷新服务器状态。"));
                    }
                }
                view.refresh_usage(window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn finish_server(
        &mut self,
        operation: ServerOperation,
        response: ServerResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ServerResult {
            result,
            reread,
            client_utc,
            received_at,
        } = response;
        let mut changed = false;
        let read_failed = matches!(
            operation,
            ServerOperation::Read | ServerOperation::ReviewClock
        ) && !matches!(result, Ok(HistoryResponse::Settings(_)));
        self.server_message = match result {
            Ok(HistoryResponse::Settings(snapshot)) => {
                if operation == ServerOperation::ReviewClock {
                    self.clock_review = snapshot
                        .clock
                        .as_ref()
                        .filter(|clock| {
                            snapshot.can_manage
                                && clock.confirmation.is_some()
                                && clock.current_wall_ms.is_some()
                        })
                        .cloned()
                        .map(|clock| ClockReview {
                            clock,
                            client_utc,
                            started: received_at,
                        });
                }
                self.install_snapshot(snapshot, client_utc, window, cx);
                match operation {
                    ServerOperation::Save => {
                        changed = true;
                        Some("设置已保存。记录器状态和清理进度可能稍后更新。".into())
                    }
                    ServerOperation::ReviewClock if self.clock_review.is_none() => {
                        Some("当前没有可远程确认的时钟事件。".into())
                    }
                    _ => None,
                }
            }
            Ok(HistoryResponse::SettingsConflict(snapshot)) => {
                self.install_snapshot(snapshot, client_utc, window, cx);
                Some("没有保存：服务器修订已改变。已载入当前设置，请核对后重新编辑。".into())
            }
            Ok(HistoryResponse::Clock(clock)) => {
                changed = true;
                if let Some(snapshot) = &mut self.snapshot {
                    snapshot.clock = clock;
                }
                Some("服务器已接受时钟确认。年龄清理可能立即开始。".into())
            }
            Err(HistoryError::Remote { detail, .. }) => {
                Some(format!("服务器未接受请求：{detail}。"))
            }
            Err(error) if operation.mutates() => {
                changed = true;
                Some(format!("未能确认操作结果：{error}。不会自动重试。"))
            }
            Err(error) => Some(format!("无法读取服务器设置：{error}。")),
            Ok(_) => Some("服务器返回了意外的历史设置响应。".into()),
        };
        if let Some(reread) = reread {
            match reread {
                Ok((snapshot, reread_at)) => {
                    self.install_snapshot(snapshot, reread_at, window, cx);
                    self.append_server_message("已重新读取服务器状态，请先核对再决定是否重试。");
                }
                Err(error) => {
                    self.server_stale = true;
                    self.append_server_message(&format!(
                        "无法刷新当前状态：{error}。请手动刷新后再操作。"
                    ));
                }
            }
        } else if read_failed {
            // 如果读取失败，旧快照只供参考，不用于写入。
            self.server_stale = true;
        }
        if changed {
            self.signal(HistorySettingsChange::ServerChanged, window, cx);
        }
    }

    fn append_server_message(&mut self, message: &str) {
        self.server_message
            .get_or_insert_with(String::new)
            .push_str(message);
    }

    fn can_save(&self) -> bool {
        !self.server_stale
            && !self.cache_busy
            && self.server_operation.is_none()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.can_manage && !snapshot.external_change)
    }

    fn preview_save(&mut self, cx: &mut Context<Self>) {
        if !self.can_save() {
            return;
        }
        let parsed = self
            .server_fields
            .values(SERVER_FIELDS, cx)
            .and_then(|values| validated_history(self.server_enabled, values));
        self.server_message = None;
        self.clock_review = None;
        match parsed {
            Ok(history)
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| history == snapshot.history) =>
            {
                self.server_message = Some("服务器设置没有更改。".into());
            }
            Ok(history) => self.server_review = Some(history),
            Err(error) => self.server_message = Some(error),
        }
        cx.notify();
    }

    fn save_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_save() {
            return;
        }
        let (Some(history), Some(snapshot)) = (self.server_review.take(), &self.snapshot) else {
            return;
        };
        self.run_server(
            ServerOperation::Save,
            HistoryRequest::SetSettings {
                expected_revision: snapshot.revision,
                history,
            },
            window,
            cx,
        );
    }

    fn review_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.server_stale
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.can_manage && snapshot.clock.is_some())
        {
            self.run_server(
                ServerOperation::ReviewClock,
                HistoryRequest::GetSettings,
                window,
                cx,
            );
        }
    }

    fn ack_clock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cache_busy {
            return;
        }
        let Some(review) = self.clock_review.take() else {
            return;
        };
        if review.started.elapsed() >= CLOCK_REVIEW_LIMIT {
            self.run_server(
                ServerOperation::ReviewClock,
                HistoryRequest::GetSettings,
                window,
                cx,
            );
            return;
        }
        let Some(confirmation) = review.clock.confirmation else {
            return;
        };
        self.run_server(
            ServerOperation::AckClock,
            HistoryRequest::AckClock {
                store_id: review.clock.store_id,
                event_id: review.clock.event_id,
                confirmation,
            },
            window,
            cx,
        );
    }

    fn save_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cache_busy || self.server_operation.is_some_and(ServerOperation::mutates) {
            return;
        }
        let preferences = match self
            .cache_fields
            .values(cache_labels(), cx)
            .and_then(|values| validated_cache(self.cache_enabled, values, self.preferences.cache))
        {
            Ok(preferences) => preferences,
            Err(error) => {
                self.cache_message = Some(error);
                cx.notify();
                return;
            }
        };
        self.cache_busy = true;
        self.cache_message = None;
        self.clear_review = None;
        let task = self
            .runtime
            .spawn(async move { preferences.cache.save().await });
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = this.update_in(cx, |view, window, cx| {
                view.cache_busy = false;
                view.cache_message = Some(match result {
                    Ok(()) => {
                        view.preferences = preferences;
                        view.signal(HistorySettingsChange::Preferences(preferences), window, cx);
                        "客户端偏好已保存。".into()
                    }
                    Err(error) => format!("无法保存客户端偏好：{error}"),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn clear_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cache_busy {
            return;
        }
        let Some(cache) = self.clear_review.take() else {
            return;
        };
        let sync = self.sync.clone();
        self.cache_busy = true;
        self.cache_message = None;
        let generation = self.source_generation;
        let task = self.runtime.spawn(async move {
            if let Some(sync) = sync {
                sync.clear_cache_partition(cache).await
            } else {
                cache.clear().await
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = this.update_in(cx, |view, window, cx| {
                view.cache_busy = false;
                if view.source_generation == generation {
                    view.cache_message = Some(match result {
                        Ok(()) => {
                            view.signal(HistorySettingsChange::CacheCleared, window, cx);
                            "本地缓存已清除，下次同步将从目录重建。".into()
                        }
                        Err(error) => format!("无法清除本地缓存：{error}"),
                    });
                    view.refresh_usage(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn render_server(&self, cx: &mut Context<Self>) -> Div {
        let busy = self.server_operation.is_some();
        let body = match (&self.sync, &self.snapshot) {
            (None, _) => hint("此会话未启用已验证的历史连接，只能管理客户端缓存。", cx),
            (_, None) if busy => hint("正在读取服务器历史设置…", cx),
            (_, None) => hint("尚未读取服务器历史设置。", cx),
            (_, Some(snapshot)) => self.render_snapshot(snapshot, cx),
        };
        panel(cx)
            .child(h_flex().justify_between().gap_2()
                .child(heading("服务器历史"))
                .child(Button::new("history-refresh").small().label("刷新服务器状态")
                    .disabled(busy || self.sync.is_none())
                    .on_click(cx.listener(|view, _, window, cx| view.refresh(window, cx)))))
            .child(body)
            .children(self.server_message.as_ref().map(|message| hint(message.clone(), cx)))
            .child(div().border_t_1().border_color(cx.theme().border).pt_3().child(
                v_flex().gap_2()
                    .child(heading("当前客户端公钥（Base64）"))
                    .child(div().text_sm().child(if self.public_key.is_empty() { "无可用的登录公钥".into() } else { self.public_key.clone() }))
                    .child(Button::new("history-copy-key").small()
                        .label(if self.copied { "公钥已复制" } else { "复制公钥" })
                        .disabled(self.public_key.is_empty())
                        .on_click(cx.listener(|view, _, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(view.public_key.clone()));
                            view.copied = true;
                            cx.notify();
                        })))
                    .children(self.snapshot.as_ref().is_none_or(|snapshot| !snapshot.can_manage).then(|| {
                        hint("将此公钥加入服务端 config.toml 的 [history] 下的 admin_keys，然后重启服务端。该公钥也必须在 [web_panel] 的 authorized_keys 中。", cx)
                    })),
            ))
    }

    fn render_snapshot(&self, snapshot: &SettingsSnapshot, cx: &mut Context<Self>) -> Div {
        let readonly = !self.can_save();
        let review = self.server_review.as_ref();
        let locked = readonly || review.is_some();
        let history = &snapshot.history;
        let mut body = v_flex()
            .gap_3()
            .child(info("目录", &snapshot.directory, cx))
            .child(info(
                "已用空间",
                bytes_label(snapshot.summary.disk_bytes),
                cx,
            ))
            .child(info(
                "最早记录",
                utc_label(snapshot.summary.range.map(|range| range.from)),
                cx,
            ))
            .child(info(
                "存储状态",
                storage_label(&snapshot.summary.storage),
                cx,
            ))
            .child(info(
                "权限",
                if snapshot.can_manage {
                    "历史管理员"
                } else {
                    "只读"
                },
                cx,
            ))
            .child(info("配置修订", snapshot.revision.number.to_string(), cx))
            .child(info(
                "等待清理",
                format!("{} 个日文件", snapshot.pending_deletions),
                cx,
            ))
            .children(
                snapshot
                    .retention_error
                    .as_ref()
                    .map(|error| hint(format!("清理异常：{error}"), cx)),
            )
            .children(snapshot.external_change.then(|| {
                hint(
                    "服务器配置文件已被外部修改。请在服务端重新加载配置后刷新；当前设置只读。",
                    cx,
                )
            }))
            .children(
                self.server_stale
                    .then(|| hint("下方显示上次成功读取的设置；请刷新以继续操作。", cx)),
            )
            .child(
                Button::new("history-recording")
                    .small()
                    .disabled(locked)
                    .label(if self.server_enabled {
                        "历史记录：已启用"
                    } else {
                        "历史记录：已停用"
                    })
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.server_enabled = !view.server_enabled;
                        cx.notify();
                    })),
            )
            .child(self.server_fields.render(SERVER_FIELDS, locked))
            .child(hint(
                format!(
                    "当前服务器保留：{} 天，{}；0 表示关闭对应限制。",
                    history.retention_days,
                    limit_label(history.retention_bytes)
                ),
                cx,
            ));
        if let Some(proposed) = review {
            body = body.child(
                v_flex()
                    .gap_2()
                    .child(heading("提交前核对保留策略"))
                    .children(
                        retention_estimate(snapshot, proposed, self.snapshot_time)
                            .into_iter()
                            .map(|line| hint(line, cx)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("history-confirm-save")
                                    .small()
                                    .primary()
                                    .label("确认保存")
                                    .disabled(readonly)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.save_server(window, cx)
                                    })),
                            )
                            .child(
                                Button::new("history-cancel-save")
                                    .small()
                                    .ghost()
                                    .label("返回编辑")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.server_review = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
        } else if snapshot.can_manage {
            body = body.child(
                Button::new("history-preview-save")
                    .small()
                    .primary()
                    .label(if self.server_operation == Some(ServerOperation::Save) {
                        "正在保存…"
                    } else {
                        "预览清理并保存…"
                    })
                    .disabled(readonly || self.clock_review.is_some())
                    .on_click(cx.listener(|view, _, _, cx| view.preview_save(cx))),
            );
        }
        if let Some(clock) = &snapshot.clock {
            body = body.child(self.render_clock(clock, snapshot.can_manage, cx));
        }
        body
    }

    fn render_clock(&self, clock: &ClockStatus, can_manage: bool, cx: &mut Context<Self>) -> Div {
        let review = self.clock_review.as_ref();
        let clock = review.map_or(clock, |review| &review.clock);
        let client_utc = review
            .map(|review| review.client_utc)
            .or(self.snapshot_time)
            .map(ms);
        let mut body = v_flex().gap_2().border_t_1().border_color(cx.theme().border).pt_3()
            .child(heading("服务器时钟异常：年龄清理已暂停"))
            .child(h_flex().gap_4().items_start()
                .child(clock_column("服务器当前时间", clock.current_wall_ms, cx))
                .child(clock_column("客户端当前时间（同次读取）", client_utc, cx)))
            .child(info("检测时刻", utc_label(clock.detected_wall_ms), cx))
            .child(info("暂停事件 ID", id_label(clock.event_id), cx))
            .child(hint("如果不能远程确认，请让服务器本机管理员从服务端日志核对历史目录和标识，确认 UTC 时钟正确后，在该目录写入 clock.ack。JSON 的 store_id 和 event_id 均为 16 个十进制字节组成的数组。", cx))
            .child(hint(format!("本机确认内容：{{\"store_id\":{:?},\"event_id\":{:?}}}", clock.store_id, clock.event_id), cx))
            .child(hint("请先核对服务器时间；恢复后可能立即清理旧记录。容量清理不受此时钟暂停影响。", cx));
        if review.is_some() {
            body = body.child(hint(
                "请核对本次读取的时间。确认逾时后会重新读取时钟，须重新核对，不会自动提交。",
                cx,
            ));
            body = body.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("history-clock-ack")
                            .small()
                            .danger()
                            .label("时间已核对，确认恢复清理")
                            .disabled(self.cache_busy)
                            .on_click(
                                cx.listener(|view, _, window, cx| view.ack_clock(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("history-clock-cancel")
                            .small()
                            .ghost()
                            .label("取消确认")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.clock_review = None;
                                cx.notify();
                            })),
                    ),
            );
        } else if can_manage {
            body = body.child(
                Button::new("history-clock-review")
                    .small()
                    .label("确认服务器时间并恢复清理…")
                    .disabled(
                        self.server_stale
                            || self.server_operation.is_some()
                            || self.server_review.is_some(),
                    )
                    .on_click(cx.listener(|view, _, window, cx| view.review_clock(window, cx))),
            );
        }
        body
    }

    fn render_cache(&self, cx: &mut Context<Self>) -> Div {
        let blocked = self.cache_busy || self.clear_review.is_some();
        let server_mutating = self.server_operation.is_some_and(ServerOperation::mutates);
        let status = match &self.cache_usage {
            Some(Ok((bytes, status))) => {
                format!("{} · {}", bytes_label(*bytes), sync_label(status))
            }
            Some(Err(error)) => format!("无法读取缓存状态：{error}"),
            None if self.cache().is_some() => "等待读取本地占用和同步状态".into(),
            None => "当前没有已打开的历史缓存".into(),
        };
        let mut body = panel(cx)
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(heading("客户端缓存"))
                    .child(
                        Button::new("history-cache-refresh")
                            .small()
                            .label("刷新缓存状态")
                            .disabled(self.cache_busy)
                            .on_click(
                                cx.listener(|view, _, window, cx| view.refresh_usage(window, cx)),
                            ),
                    ),
            )
            .child(info(
                if self.preferences.cache.enabled {
                    "本地占用与同步"
                } else {
                    "内存占用与同步"
                },
                status,
                cx,
            ))
            .child(
                Button::new("history-cache-enabled")
                    .small()
                    .disabled(blocked)
                    .label(if self.cache_enabled {
                        "磁盘缓存：已启用"
                    } else {
                        "磁盘缓存：已停用"
                    })
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.cache_enabled = !view.cache_enabled;
                        cx.notify();
                    })),
            )
            .child(hint(
                "关闭磁盘缓存后仅使用有界内存；关闭自动预取后仍可按趋势图可见区间在线加载。",
                cx,
            ))
            .child(self.cache_fields.render(cache_labels(), blocked))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("history-cache-save")
                            .small()
                            .primary()
                            .label("保存客户端偏好")
                            .disabled(blocked || server_mutating)
                            .on_click(
                                cx.listener(|view, _, window, cx| view.save_cache(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("history-cache-defaults")
                            .small()
                            .ghost()
                            .label("填入默认值")
                            .disabled(blocked)
                            .on_click(cx.listener(|view, _, window, cx| {
                                let defaults = CachePolicy::default();
                                view.cache_enabled = defaults.enabled;
                                view.cache_fields
                                    .set(preference_values(defaults), window, cx);
                                view.cache_message = Some("已填入默认值，保存后生效。".into());
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("history-cache-clear")
                            .small()
                            .label("清除本地缓存…")
                            .disabled(blocked || self.cache().is_none())
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.clear_review = view.cache();
                                view.cache_message = None;
                                cx.notify();
                            })),
                    ),
            );
        if let Some(cache) = &self.clear_review {
            body = body
                .child(hint(
                    format!(
                        "将清除本地存储分区 {}。清除后该缓存将无法离线查看；服务器历史不会删除。",
                        id_label(cache.store_id())
                    ),
                    cx,
                ))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("history-cache-confirm-clear")
                                .small()
                                .danger()
                                .label("确认清除本地缓存")
                                .on_click(
                                    cx.listener(|view, _, window, cx| view.clear_cache(window, cx)),
                                ),
                        )
                        .child(
                            Button::new("history-cache-cancel-clear")
                                .small()
                                .ghost()
                                .label("取消")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.clear_review = None;
                                    cx.notify();
                                })),
                        ),
                );
        }
        body.children(
            self.cache_message
                .as_ref()
                .map(|message| hint(message.clone(), cx)),
        )
    }
}

impl Render for HistorySettings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.section {
            HistorySection::Cache => self.render_cache(cx),
            HistorySection::Server => self.render_server(cx),
        }
    }
}

async fn read_snapshot(sync: &HistorySync) -> Result<SettingsSnapshot, String> {
    match sync.request_settings(HistoryRequest::GetSettings).await {
        Ok(HistoryResponse::Settings(snapshot)) => Ok(snapshot),
        Ok(HistoryResponse::Error(error)) => Err(error.detail),
        Ok(_) => Err("服务器返回了意外的状态响应".into()),
        Err(error) => Err(error.to_string()),
    }
}

fn cache_labels() -> [&'static str; 5] {
    [
        CACHE_FIELDS[0],
        CACHE_FIELDS[1],
        CACHE_FIELDS[2],
        CACHE_FIELDS[3],
        TREND_FIELD,
    ]
}

fn preference_values(cache: CachePolicy) -> [u64; 5] {
    [
        cache.retention_days.into(),
        cache.retention_bytes,
        cache.prefetch_days.into(),
        cache.refresh_interval.as_secs(),
        cache.trend_window_ms / 1_000,
    ]
}

fn validated_history(enabled: bool, values: [u64; 5]) -> Result<EditableHistory, String> {
    let [
        idle_interval_ms,
        min_interval_ms,
        seal_interval_secs,
        days,
        retention_bytes,
    ] = values;
    if !(250..=60_000).contains(&idle_interval_ms) {
        return Err("空闲采样间隔须为 250–60,000 毫秒。".into());
    }
    if !(100..=idle_interval_ms).contains(&min_interval_ms) {
        return Err("最短采样间隔须为 100 毫秒至空闲采样间隔。".into());
    }
    if !(60..=3600).contains(&seal_interval_secs) {
        return Err("封存间隔须为 60–3,600 秒。".into());
    }
    if days > 3650 {
        return Err("保留天数须为 0–3,650。".into());
    }
    if retention_bytes != 0 && !(16 * MIB..=16 * 1024 * MIB).contains(&retention_bytes) {
        return Err(
            "服务器保留上限须为 0，或 16 MiB–16 GiB（16,777,216–17,179,869,184 字节）。".into(),
        );
    }
    if days == 0 && retention_bytes == 0 {
        return Err("至少启用一种服务器保留限制。".into());
    }
    Ok(EditableHistory {
        enabled,
        idle_interval_ms,
        min_interval_ms,
        seal_interval_secs,
        retention_days: days as u32,
        retention_bytes,
    })
}

fn validated_cache(
    enabled: bool,
    values: [u64; 5],
    current: CachePolicy,
) -> Result<HistoryPreferences, String> {
    let [days, retention_bytes, prefetch, refresh_secs, trend_secs] = values;
    let retention_days = u32::try_from(days).map_err(|_| "本地保留天数过大。".to_string())?;
    let prefetch_days = u32::try_from(prefetch).map_err(|_| "预取天数过大。".to_string())?;
    if !(1..=86_400).contains(&refresh_secs) {
        return Err("增量同步间隔须为 1–86,400 秒。".into());
    }
    if !(10..=86_400).contains(&trend_secs) {
        return Err("趋势图默认窗口须为 10–86,400 秒。".into());
    }
    if enabled && retention_days == 0 && retention_bytes == 0 {
        return Err("启用磁盘缓存时至少启用一种本地保留限制。".into());
    }
    Ok(HistoryPreferences::from_cache(CachePolicy {
        enabled,
        retention_days,
        retention_bytes,
        prefetch_days,
        refresh_interval: Duration::from_secs(refresh_secs),
        trend_window_ms: trend_secs * 1_000,
        ..current
    }))
}

fn retention_estimate(
    snapshot: &SettingsSnapshot,
    proposed: &EditableHistory,
    as_of: Option<OffsetDateTime>,
) -> Vec<String> {
    let old = &snapshot.history;
    let tighter_age = tightens(old.retention_days.into(), proposed.retention_days.into());
    let tighter_capacity = tightens(old.retention_bytes, proposed.retention_bytes);
    let mut lines = Vec::new();
    if !tighter_age && !tighter_capacity {
        lines.push("没有缩小保留范围。原有清理规则和已排队的删除仍可能继续执行。".into());
        return lines;
    }
    if tighter_age {
        let clock = snapshot.clock.as_ref();
        let cut = clock
            .and_then(|clock| clock.current_wall_ms)
            .or_else(|| as_of.map(ms))
            .and_then(|now| {
                estimate_age_range(snapshot.summary.range, now, proposed.retention_days)
            });
        lines.push(match cut {
            Some(range) => format!(
                "按 UTC 整天清理，现有记录中预计可能受影响的年龄范围：{} 至 {}（不含结束时刻）。",
                utc_label(Some(range.from)),
                utc_label(Some(range.to))
            ),
            None => "按当前已知的记录范围，预计没有记录落在新年龄边界之前。".into(),
        });
        if clock.is_some() {
            lines.push(
                "服务器时钟未受信任，年龄清理已暂停；此日期仅供参考，恢复后可能变化。".into(),
            );
        } else {
            lines.push("年龄边界按本机读取设置时的 UTC 日期估算，以服务器实际日期为准。".into());
        }
    }
    if tighter_capacity {
        // 容量清理会预留写入空间；线上的摘要没有逐日容量，不能给出精确的删除日期。
        let used = snapshot.summary.disk_bytes;
        let over = used.saturating_sub(proposed.retention_bytes.saturating_sub(MIB));
        if over > 0 {
            let earliest = utc_label(snapshot.summary.range.map(|range| range.from));
            lines.push(format!("当前占用超出含预留空间的目标约 {}。容量清理从最旧的日文件开始（最早记录：{earliest}），无法从摘要估算结束日期。", bytes_label(over)));
        } else {
            lines.push("按当前占用估算，新容量上限不会立即触发清理。".into());
        }
    }
    lines.push("估算不计持续写入、单日占用和正在记录的日文件。保存后服务器可能立即清理。".into());
    lines
}

fn tightens(old: u64, new: u64) -> bool {
    new != 0 && (old == 0 || new < old)
}

fn estimate_age_range(range: Option<TimeRange>, now: i64, days: u32) -> Option<TimeRange> {
    let range = range?;
    let cutoff = now
        .div_euclid(DAY_MS)
        .saturating_sub(i64::from(days.checked_sub(1)?))
        .saturating_mul(DAY_MS);
    let to = cutoff.min(range.to);
    (range.from < to).then_some(TimeRange {
        from: range.from,
        to,
    })
}

fn heading(text: &'static str) -> Div {
    div().font_semibold().child(text)
}
fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}
fn info(name: &str, value: impl AsRef<str>, cx: &App) -> Div {
    hint(format!("{name}：{}", value.as_ref()), cx)
}

fn bytes_label(bytes: u64) -> String {
    format!("{:.1} MiB（{bytes} 字节）", bytes as f64 / MIB as f64)
}
fn limit_label(bytes: u64) -> String {
    if bytes == 0 {
        "不限".to_string()
    } else {
        bytes_label(bytes)
    }
}
fn ms(at: OffsetDateTime) -> i64 {
    at.unix_timestamp_nanos().div_euclid(1_000_000) as i64
}
fn from_ms(at: Option<i64>) -> Option<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(at?) * 1_000_000).ok()
}
fn utc_label(at: Option<i64>) -> String {
    from_ms(at)
        .and_then(|at| {
            at.format(format_description!(
                "[year]-[month]-[day] [hour]:[minute]:[second] UTC"
            ))
            .ok()
        })
        .unwrap_or_else(|| "未知".into())
}
fn local_label(at: Option<i64>) -> String {
    from_ms(at).and_then(|at| {
        let offset = UtcOffset::local_offset_at(at).ok()?;
        at.to_offset(offset).format(format_description!("[year]-[month]-[day] [hour]:[minute]:[second] (UTC[offset_hour sign:mandatory]:[offset_minute])")).ok()
    }).unwrap_or_else(|| "未知".into())
}
fn clock_column(name: &'static str, at: Option<i64>, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .flex_1()
        .min_w_0()
        .child(heading(name))
        .child(hint(utc_label(at), cx))
        .child(info("客户端本地时区", local_label(at), cx))
}
fn id_label(id: Id) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn storage_label(status: &StorageStatus) -> String {
    match status {
        StorageStatus::Starting => "正在启动".into(),
        StorageStatus::Disabled => "记录已停用".into(),
        StorageStatus::Recording => "正在记录".into(),
        StorageStatus::ReadOnly(reason) => format!("只读：{reason}"),
        StorageStatus::Error(reason) => format!("异常：{reason}"),
    }
}
fn sync_label(status: &SyncStatus) -> String {
    match status {
        SyncStatus::Never => "尚未同步".into(),
        SyncStatus::InProgress => "正在同步".into(),
        SyncStatus::Synced { at_ms } => format!("上次同步：{}", utc_label(Some(*at_ms))),
        SyncStatus::Failed { at_ms, reason } => {
            format!("同步失败（{}）：{reason}", utc_label(Some(*at_ms)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DAY_MS, MIB, estimate_age_range, preference_values, tightens, validated_cache,
        validated_history,
    };
    use client::history::CachePolicy;
    use common::history_api::TimeRange;
    use std::time::Duration;

    #[test]
    fn age_preview_counts_today_and_uses_exclusive_utc_boundary() {
        let range = TimeRange {
            from: DAY_MS - 1,
            to: DAY_MS * 4,
        };
        assert_eq!(
            estimate_age_range(Some(range), DAY_MS * 3 + 12, 2),
            Some(TimeRange {
                from: DAY_MS - 1,
                to: 2 * DAY_MS
            })
        );
        assert_eq!(estimate_age_range(Some(range), DAY_MS, 2), None);
        assert_eq!(estimate_age_range(Some(range), DAY_MS, 0), None);
    }

    #[test]
    fn save_validation_preserves_server_limits_and_zero_is_unbounded() {
        assert!(validated_history(true, [250, 100, 60, 1, 0]).is_ok());
        assert!(validated_history(true, [250, 100, 60, 0, 0]).is_err());
        assert!(validated_history(true, [250, 251, 60, 1, 0]).is_err());
        assert!(validated_history(true, [250, 100, 60, 0, 16 * MIB]).is_ok());
        assert!(tightens(0, 30));
        assert!(!tightens(30, 0));
    }

    #[test]
    fn client_defaults_round_trip_and_prefetch_may_be_disabled() {
        let defaults = CachePolicy::default();
        let mut values = preference_values(defaults);
        assert_eq!(values, [30, 512 * MIB, 7, 60, 300]);
        values[2] = 0;
        let prefs = validated_cache(true, values, defaults).unwrap();
        assert_eq!(prefs.cache.prefetch_days, 0);
        assert_eq!(prefs.trend_window, Duration::from_secs(300));
    }
}
