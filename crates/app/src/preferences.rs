use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Motion {
    #[default]
    System,
    Allow,
    Reduce,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartPage {
    #[default]
    Monitor,
    Services,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RefreshRate {
    HalfSecond,
    #[default]
    OneSecond,
    ThreeSeconds,
}

impl RefreshRate {
    pub(crate) const CHOICES: [(Self, &str); 3] = [
        (Self::HalfSecond, "0.5 秒"),
        (Self::OneSecond, "1 秒"),
        (Self::ThreeSeconds, "3 秒"),
    ];

    pub(crate) fn millis(self) -> u64 {
        match self {
            Self::HalfSecond => 500,
            Self::OneSecond => 1_000,
            Self::ThreeSeconds => 3_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ClientPreferences {
    pub(crate) appearance: Appearance,
    pub(crate) motion: Motion,
    pub(crate) frosted: bool,
    pub(crate) start_page: StartPage,
    pub(crate) refresh_rate: RefreshRate,
}

impl Default for ClientPreferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            motion: Motion::default(),
            frosted: true,
            start_page: StartPage::default(),
            refresh_rate: RefreshRate::default(),
        }
    }
}

impl Global for ClientPreferences {}

impl ClientPreferences {
    pub(crate) fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("remotehelper/client-preferences.json"))
    }

    pub(crate) fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let result = std::fs::read(path)
            .and_then(|bytes| serde_json::from_slice(&bytes).map_err(io::Error::other));
        match result {
            Ok(preferences) => preferences,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound {
                    tracing::warn!(%error, "无法加载客户端偏好，使用默认设置");
                }
                Self::default()
            }
        }
    }

    pub(crate) fn get(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }

    pub(crate) fn install(self, cx: &mut App) {
        use gpui_kit::base::{ReduceMotionPreference, set_reduce_motion_preference};

        if cx.try_global::<Self>().map(|previous| previous.motion) != Some(self.motion) {
            let motion = match self.motion {
                Motion::System => ReduceMotionPreference::System,
                Motion::Allow => ReduceMotionPreference::Allow,
                Motion::Reduce => ReduceMotionPreference::Reduce,
            };
            set_reduce_motion_preference(motion, cx);
        }
        cx.set_global(self);
    }

    pub(crate) async fn save(self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::other("无法定位客户端配置目录"))?;
        self.save_to(&path).await
    }

    async fn save_to(self, path: &Path) -> io::Result<()> {
        use tokio::io::AsyncWriteExt;

        let bytes = serde_json::to_vec_pretty(&self).map_err(io::Error::other)?;
        let dir = path.parent().expect("client preferences directory");
        tokio::fs::create_dir_all(dir).await?;
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = async {
            let mut file = tokio::fs::File::create(&tmp).await?;
            file.write_all(&bytes).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&tmp, path).await
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_preferences_fill_defaults_and_preserve_explicit_choices() {
        let prefs: ClientPreferences = serde_json::from_str(
            r#"{"appearance":"dark","frosted":false,"refresh_rate":"three_seconds"}"#,
        )
        .unwrap();
        assert_eq!(prefs.appearance, Appearance::Dark);
        assert!(!prefs.frosted);
        assert_eq!(prefs.refresh_rate.millis(), 3_000);
        assert_eq!(prefs.motion, Motion::System);
        assert_eq!(prefs.start_page, StartPage::Monitor);
        assert_eq!(
            serde_json::from_value::<ClientPreferences>(serde_json::to_value(prefs).unwrap())
                .unwrap(),
            prefs
        );
    }

    #[tokio::test]
    async fn a_second_save_replaces_the_previous_preferences() {
        let dir = std::env::temp_dir().join(format!(
            "remotehelper-preferences-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("preferences.json");
        let first = ClientPreferences::default();
        let second = ClientPreferences {
            appearance: Appearance::Light,
            frosted: false,
            motion: Motion::Reduce,
            start_page: StartPage::Services,
            refresh_rate: RefreshRate::ThreeSeconds,
        };
        first.save_to(&path).await.unwrap();
        second.save_to(&path).await.unwrap();
        let saved: ClientPreferences =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved, second);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
