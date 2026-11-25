//! 图标资源：GPUI Kit 默认资源包之外额外用到的 Lucide 图标。

use gpui_kit::*;

// GPUI Kit 的默认资源包不含这些图标，按需单独打包。
gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Activity,
        Server,
        Cpu,
        MemoryStick,
        Monitor,
        RefreshCw,
        Plus,
        ChevronDown
    ]
);

/// 应用资源入口：先查额外图标，再回落到 GPUI Kit 的默认资源包。
pub(crate) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(data) => Ok(Some(data)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ExtraIcons.list(path)?;
        paths.extend(gpui_kit::assets::Assets.list(path)?);
        paths.sort_unstable();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::AppAssets;
    use gpui_kit::AssetSource;
    use gpui_kit::assets::IconName;

    #[test]
    fn application_icons_load_from_the_asset_source() {
        for icon in [
            IconName::Activity,
            IconName::Server,
            IconName::Cpu,
            IconName::MemoryStick,
            IconName::Monitor,
            IconName::RefreshCw,
            IconName::Plus,
            IconName::ChevronDown,
            IconName::Check,
        ] {
            let path = icon.path();
            let svg = AppAssets
                .load(path.as_ref())
                .expect("icon path should exist");
            assert!(svg.is_some_and(|bytes| !bytes.is_empty()), "missing {path}");
        }
    }
}
