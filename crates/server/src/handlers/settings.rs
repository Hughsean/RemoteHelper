use super::history::{error, summary};
use crate::config::{AppConfig, CONFIG_PATH, HistoryConfig, read_config};
use crate::history::{ClockAckError, RecorderHandle, RecorderOptions, atomic_write};
use crate::state::AppState;
use common::history_api::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

pub(crate) struct RuntimeSettings {
    config: HistoryConfig,
    file_digest: [u8; 32],
    normalized_digest: [u8; 32],
}

impl RuntimeSettings {
    pub fn new(config: HistoryConfig, bytes: &[u8]) -> Self {
        // 此结构字段顺序固定；保留原始摘要单独用于检测外部文件改动。
        let normalized =
            toml::from_str::<AppConfig>(std::str::from_utf8(bytes).unwrap_or_default())
                .ok()
                .and_then(|config| serde_json::to_vec(&config.history).ok());
        Self {
            config,
            file_digest: digest(bytes),
            normalized_digest: digest(normalized.as_deref().unwrap_or(bytes)),
        }
    }

    fn revision(&self) -> SettingsRevision {
        let mut hash = Sha256::new();
        hash.update(b"RemoteHelper/history/settings/v1\0");
        hash.update(self.normalized_digest);
        hash.update(self.config.revision.to_le_bytes());
        SettingsRevision {
            number: self.config.revision,
            digest: hash.finalize().into(),
        }
    }
}

pub(super) struct Confirmation {
    token: Id,
    store_id: Id,
    event_id: Id,
    issued: Instant,
    wall_ms: i64,
}

pub(super) async fn handle(
    state: &AppState,
    confirmation: &mut Option<Confirmation>,
    client_key: &str,
    recorder: &RecorderHandle,
    request: &HistoryRequest,
) -> HistoryResponse {
    let admin = state
        .config
        .history
        .admin_keys
        .iter()
        .any(|key| key == client_key)
        && state
            .config
            .web_panel
            .authorized_keys
            .iter()
            .any(|key| key == client_key);
    match request {
        HistoryRequest::GetSettings => {
            let current = read_current().await;
            let runtime = state.history_settings.lock().await;
            let mut snapshot = snapshot(
                &runtime,
                recorder,
                admin,
                current
                    .as_ref()
                    .ok()
                    .is_none_or(|bytes| digest(bytes) != runtime.file_digest),
            );
            *confirmation = snapshot.clock.as_mut().and_then(|clock| {
                if !admin {
                    return None;
                }
                let wall_ms = clock.current_wall_ms?;
                let token = rand::random();
                clock.confirmation = Some(token);
                Some(Confirmation {
                    token,
                    store_id: clock.store_id,
                    event_id: clock.event_id,
                    issued: Instant::now(),
                    wall_ms,
                })
            });
            HistoryResponse::Settings(snapshot)
        }
        HistoryRequest::SetSettings {
            expected_revision,
            history,
        } if admin => {
            // 连接超时可以丢弃响应，但已经开始的配置提交和运行时切换必须一起完成。
            let state = state.clone();
            let recorder = recorder.clone();
            let expected_revision = *expected_revision;
            let history = history.clone();
            tokio::spawn(
                async move { set_settings(state, recorder, expected_revision, history).await },
            )
            .await
            .unwrap_or_else(|_| error(HistoryErrorCode::SaveFailed, "配置提交任务失败"))
        }
        HistoryRequest::AckClock {
            store_id,
            event_id,
            confirmation: token,
        } if admin => {
            let current = recorder.snapshot();
            if current.store_id != Some(*store_id)
                || current
                    .retention
                    .clock_pause
                    .is_none_or(|event| event.event_id != *event_id)
            {
                *confirmation = None;
                return error(
                    HistoryErrorCode::ClockEventChanged,
                    "当前时钟事件已改变，请刷新设置",
                );
            }
            let Some(challenge) = confirmation.take().filter(|c| {
                c.token == *token && c.store_id == *store_id && c.event_id == *event_id
            }) else {
                return error(
                    HistoryErrorCode::ConfirmationExpired,
                    "确认凭据无效，请刷新设置",
                );
            };
            match recorder
                .ack_clock(*store_id, *event_id, challenge.wall_ms, challenge.issued)
                .await
            {
                Ok(()) => HistoryResponse::Clock(clock_status(&recorder.snapshot())),
                Err(ClockAckError::Expired) => error(
                    HistoryErrorCode::ConfirmationExpired,
                    "确认凭据已过期，请刷新设置",
                ),
                Err(ClockAckError::EventChanged) => error(
                    HistoryErrorCode::ClockEventChanged,
                    "当前时钟事件已改变，请刷新设置",
                ),
                Err(ClockAckError::ClockChanged) => error(
                    HistoryErrorCode::ClockChanged,
                    "服务器墙钟再次跳变，请核对并刷新设置",
                ),
                Err(ClockAckError::Storage) => error(
                    HistoryErrorCode::SaveFailed,
                    "时钟确认未持久化，年龄清理仍暂停",
                ),
            }
        }
        _ => error(
            HistoryErrorCode::Unauthorized,
            "仅历史管理员可修改服务器设置或确认时钟",
        ),
    }
}

async fn set_settings(
    state: AppState,
    recorder: RecorderHandle,
    expected_revision: SettingsRevision,
    history: EditableHistory,
) -> HistoryResponse {
    let mut runtime = state.history_settings.lock().await;
    let Ok(current) = read_current().await else {
        return error(HistoryErrorCode::SaveFailed, "无法读取当前服务器配置");
    };
    let external = digest(&current) != runtime.file_digest;
    if external || expected_revision != runtime.revision() {
        return HistoryResponse::SettingsConflict(snapshot(&runtime, &recorder, true, external));
    }
    let mut proposed = runtime.config.clone();
    proposed.enabled = history.enabled;
    proposed.idle_interval_ms = history.idle_interval_ms;
    proposed.min_interval_ms = history.min_interval_ms;
    proposed.seal_interval_secs = history.seal_interval_secs;
    proposed.retention_days = history.retention_days;
    proposed.retention_bytes = history.retention_bytes;
    let Some(revision) = proposed
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
    else {
        return error(HistoryErrorCode::InvalidSettings, "历史设置修订号已用尽");
    };
    proposed.revision = revision;
    if let Err(err) = proposed.validate() {
        return error(HistoryErrorCode::InvalidSettings, err.to_string());
    }
    match tokio::task::spawn_blocking(move || save_current(&current, &history, revision)).await {
        Ok(Ok(Some(bytes))) => {
            recorder.configure(RecorderOptions {
                enabled: proposed.enabled,
                seal_interval: Duration::from_secs(proposed.seal_interval_secs),
                retention_days: proposed.retention_days,
                retention_bytes: proposed.retention_bytes,
                compression_level: proposed.compression_level,
            });
            state.monitor.configure(&proposed);
            *runtime = RuntimeSettings::new(proposed, &bytes);
            HistoryResponse::Settings(snapshot(&runtime, &recorder, true, false))
        }
        Ok(Ok(None)) => {
            HistoryResponse::SettingsConflict(snapshot(&runtime, &recorder, true, true))
        }
        Ok(Err(err)) => {
            tracing::warn!(%err, "保存历史设置失败");
            error(HistoryErrorCode::SaveFailed, "无法持久化历史设置")
        }
        Err(_) => error(HistoryErrorCode::SaveFailed, "配置写入任务失败"),
    }
}

fn snapshot(
    runtime: &RuntimeSettings,
    recorder: &RecorderHandle,
    admin: bool,
    external: bool,
) -> SettingsSnapshot {
    let current = recorder.snapshot();
    let config = &runtime.config;
    SettingsSnapshot {
        history: EditableHistory {
            enabled: config.enabled,
            idle_interval_ms: config.idle_interval_ms,
            min_interval_ms: config.min_interval_ms,
            seal_interval_secs: config.seal_interval_secs,
            retention_days: config.retention_days,
            retention_bytes: config.retention_bytes,
        },
        revision: runtime.revision(),
        can_manage: admin,
        directory: config.dir.to_string_lossy().into_owned(),
        summary: summary(&current),
        clock: clock_status(&current),
        pending_deletions: current.retention.pending_deletions.len(),
        retention_error: current.retention.error.clone(),
        external_change: external,
    }
}

fn clock_status(snapshot: &crate::history::HistorySnapshot) -> Option<ClockStatus> {
    let pause = snapshot.retention.clock_pause?;
    Some(ClockStatus {
        store_id: snapshot.store_id?,
        event_id: pause.event_id,
        detected_wall_ms: pause.detected_wall_ms,
        current_wall_ms: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|time| i64::try_from(time.as_millis()).ok()),
        confirmation: None,
    })
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

async fn read_current() -> std::io::Result<Vec<u8>> {
    tokio::task::spawn_blocking(|| read_config(Path::new(CONFIG_PATH)))
        .await
        .map_err(std::io::Error::other)?
}

fn save_current(
    current: &[u8],
    history: &EditableHistory,
    revision: u64,
) -> anyhow::Result<Option<Vec<u8>>> {
    let path = std::path::absolute(CONFIG_PATH)?;
    let bytes = apply_edit(current, history, revision)?;
    if read_config(&path)? != current {
        return Ok(None);
    }
    // 提交前核对配置来源，运行时只应用已落盘的版本。
    atomic_write(&path, &bytes)?;
    Ok(Some(bytes))
}

fn apply_edit(current: &[u8], history: &EditableHistory, revision: u64) -> anyhow::Result<Vec<u8>> {
    use toml_edit::{DocumentMut, Item, Table, value};
    let mut doc = std::str::from_utf8(current)?.parse::<DocumentMut>()?;
    if !doc.contains_key("history") {
        doc.insert("history", Item::Table(Table::new()));
    }
    let item = doc["history"]
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("历史配置不是表"))?;
    for (key, setting) in [
        ("enabled", value(history.enabled)),
        (
            "idle_interval_ms",
            value(i64::try_from(history.idle_interval_ms)?),
        ),
        (
            "min_interval_ms",
            value(i64::try_from(history.min_interval_ms)?),
        ),
        (
            "seal_interval_secs",
            value(i64::try_from(history.seal_interval_secs)?),
        ),
        ("retention_days", value(i64::from(history.retention_days))),
        (
            "retention_bytes",
            value(i64::try_from(history.retention_bytes)?),
        ),
        ("revision", value(i64::try_from(revision)?)),
    ] {
        let mut setting = setting;
        if let (Some(new), Some(old)) = (
            setting.as_value_mut(),
            item.get(key).and_then(Item::as_value),
        ) {
            *new.decor_mut() = old.decor().clone();
        }
        item.insert(key, setting);
    }
    let text = doc.to_string();
    toml::from_str::<AppConfig>(&text)?.history.validate()?;
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_revision_ignores_comments_but_external_change_detects_them() {
        let bytes = include_bytes!("../../../../config.example.toml");
        let config: AppConfig = toml::from_str(std::str::from_utf8(bytes).unwrap()).unwrap();
        let mut altered = bytes.to_vec();
        altered.extend_from_slice(b"\n# note\n");
        let first = RuntimeSettings::new(config.history.clone(), bytes);
        let second = RuntimeSettings::new(config.history, &altered);
        assert_eq!(first.revision(), second.revision());
        assert_ne!(first.file_digest, second.file_digest);
    }

    #[test]
    fn update_keeps_noneditable_settings_and_comments() {
        let source = include_bytes!("../../../../config.example.toml");
        let settings = EditableHistory {
            enabled: false,
            idle_interval_ms: 3000,
            min_interval_ms: 100,
            seal_interval_secs: 300,
            retention_days: 5,
            retention_bytes: 0,
        };
        let bytes = apply_edit(source, &settings, 1).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("# 可以定义多个服务"));
        let config: AppConfig = toml::from_str(text).unwrap();
        assert!(!config.history.enabled);
        assert_eq!(config.history.revision, 1);
        assert_eq!(config.history.dir, Path::new("history"));
        assert_eq!(config.history.compression_level, 3);
        assert_eq!(config.web_panel.local_port, 9999);
    }
}
