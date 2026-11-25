//! 缺口、同步和保留边界与阶梯共用同一组裁切后的证据。

use client::history::SyncStatus;
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::*;
use history::GapReason;

use super::{
    playback::{Band, BandKind, Mark},
    viewport::date_time,
};

#[derive(Clone, Copy)]
pub(super) struct Colors {
    gap: Hsla,
    sync: Hsla,
    expired: Hsla,
    mark: Hsla,
}

impl Colors {
    pub(super) fn new(cx: &App) -> Self {
        let theme = cx.theme();
        Self {
            gap: theme.chart_3,
            sync: theme.muted_foreground,
            expired: theme.muted_foreground,
            mark: theme.chart_2,
        }
    }
}

pub(super) fn duration(ms: u64) -> String {
    match ms {
        0..=999 => format!("{ms} 毫秒"),
        1_000..=59_999 if ms % 1_000 != 0 => format!("{:.3} 秒", ms as f64 / 1_000.),
        1_000..=59_999 => format!("{} 秒", ms / 1_000),
        60_000..=3_599_999 => format!("{} 分 {} 秒", ms / 60_000, ms / 1_000 % 60),
        3_600_000..=86_399_999 => format!("{} 小时 {} 分", ms / 3_600_000, ms / 60_000 % 60),
        _ => format!("{} 天 {} 小时", ms / 86_400_000, ms / 3_600_000 % 24),
    }
}

fn reason(reason: GapReason) -> &'static str {
    match reason {
        GapReason::Disabled => "停止记录",
        GapReason::Storage => "存储故障",
        GapReason::RecorderOverrun => "记录队列丢样",
        GapReason::Rebooted => "主机重启",
        GapReason::Stopped => "服务正常停止",
        GapReason::Interrupted => "服务中断",
        GapReason::Sleep => "主机休眠",
        GapReason::ClockJump => "时钟跳变",
        GapReason::Stalled => "采样停顿",
        GapReason::Unknown => "原因未知",
    }
}

fn sync_label(status: Option<&SyncStatus>) -> &'static str {
    match status {
        Some(SyncStatus::Failed { reason, .. }) if reason == "离线查看" => "离线 · 尚未同步",
        Some(SyncStatus::Failed { .. }) => "同步失败",
        Some(SyncStatus::InProgress) => "正在同步",
        _ => "尚未同步",
    }
}

pub(super) fn annotation(
    bands: &[Band],
    marks: &[Mark],
    at: f64,
    tolerance: f64,
    status: Option<&SyncStatus>,
) -> Option<String> {
    let i = marks.partition_point(|mark| (mark.at as f64) < at);
    let mark = [marks.get(i), i.checked_sub(1).and_then(|i| marks.get(i))]
        .into_iter()
        .flatten()
        .filter(|mark| (mark.at as f64 - at).abs() <= tolerance)
        .min_by(|a, b| {
            (a.at as f64 - at)
                .abs()
                .total_cmp(&(b.at as f64 - at).abs())
        });
    if let Some(mark) = mark {
        let name = if mark.rollback {
            "时钟回拨"
        } else {
            "时钟分段"
        };
        return Some(format!("{name} · {}", date_time(mark.at as f64, true)));
    }
    let index = bands.partition_point(|band| (band.range.to as f64) <= at);
    let band = bands
        .get(index)
        .filter(|band| band.range.from as f64 <= at)?;
    Some(match band.kind {
        BandKind::Gap {
            reason: why,
            duration: ms,
        } => format!("记录空缺 · {} · {}", reason(why), duration(ms)),
        BandKind::Expired { until: Some(until) } => {
            format!("已过保留范围 · {} 之前", date_time(until as f64, false))
        }
        BandKind::Expired { until: None } => "服务器已淘汰；此处无本地记录".into(),
        BandKind::Sync => sync_label(status).into(),
    })
}

pub(super) fn legend(
    bands: &[Band],
    marks: &[Mark],
    (start, end): (f64, f64),
    status: &SyncStatus,
    truncated: bool,
    colors: Colors,
    muted: Hsla,
) -> AnyElement {
    let mut present = [false; 3];
    for band in bands
        .iter()
        .filter(|band| band.range.from as f64 <= end && band.range.to as f64 > start)
    {
        present[match band.kind {
            BandKind::Gap { .. } => 0,
            BandKind::Sync => 1,
            BandKind::Expired { .. } => 2,
        }] = true;
    }
    let visible_mark = marks
        .iter()
        .any(|mark| start <= mark.at as f64 && mark.at as f64 <= end);
    let label = |word, symbol, color| {
        h_flex()
            .gap_1()
            .child(div().text_color(color).child(symbol))
            .child(word)
    };
    h_flex()
        .pl(px(46.))
        .gap_3()
        .flex_shrink_0()
        .text_size(px(11.))
        .text_color(muted)
        .children(present[0].then(|| label("记录空缺", "╱╱", colors.gap)))
        .children(
            (present[1] || matches!(status, SyncStatus::Failed { .. }))
                .then(|| label(sync_label(Some(status)), "⠿", colors.sync)),
        )
        .children(present[2].then(|| label("已过保留范围", "▬", colors.expired)))
        .children(visible_mark.then(|| label("时钟变化", "┆", colors.mark)))
        .children(truncated.then(|| label("查询结果受限 · 请放大", "◌", colors.sync)))
        .into_any_element()
}

pub(super) fn paint_bands(
    bands: &[Band],
    (start, end): (f64, f64),
    bounds: Bounds<Pixels>,
    colors: Colors,
    window: &mut Window,
) {
    let span = (end - start).max(1.);
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let mut stripes = PathBuilder::stroke(px(1.));
    let mut dots = PathBuilder::stroke(px(1.5));
    for band in bands {
        let left = (((band.range.from as f64 - start) / span) as f32 * width).max(0.);
        let right = (((band.range.to as f64 - start) / span) as f32 * width).min(width);
        if left >= right {
            continue;
        }
        let pos = |x, y| point(bounds.left() + px(x), bounds.top() + px(y));
        let rect = Bounds::new(pos(left, 0.), size(px(right - left), bounds.size.height));
        match band.kind {
            BandKind::Gap { .. } => {
                window.paint_quad(fill(rect, colors.gap.opacity(0.09)));
                // 斜线锚定画布网格，只沿带的矩形边界裁切，不覆盖邻接读数。
                let mut line = left - left.rem_euclid(8.);
                while line <= right + height {
                    let top = (line - right).max(0.);
                    let bottom = (line - left).min(height);
                    if bottom > top {
                        stripes.move_to(pos(line - top, top));
                        stripes.line_to(pos(line - bottom, bottom));
                    }
                    line += 8.;
                }
            }
            BandKind::Sync => {
                window.paint_quad(fill(rect, colors.sync.opacity(0.06)));
                let mut x = (left / 7.).ceil() * 7. + 1.;
                while x + 1. < right {
                    let mut y = 4.;
                    while y < height {
                        dots.move_to(pos(x, y));
                        dots.line_to(pos(x + 1., y));
                        y += 7.;
                    }
                    x += 7.;
                }
            }
            BandKind::Expired { .. } => {
                window.paint_quad(fill(rect, colors.expired.opacity(0.15)));
                if band.range.to as f64 <= end {
                    window.paint_quad(fill(
                        Bounds::new(pos(right - 1., 0.), size(px(1.), bounds.size.height)),
                        colors.expired.opacity(0.5),
                    ));
                }
            }
        }
    }
    if let Ok(path) = stripes.build() {
        window.paint_path(path, colors.gap.opacity(0.36));
    }
    if let Ok(path) = dots.build() {
        window.paint_path(path, colors.sync.opacity(0.43));
    }
}

pub(super) fn paint_marks(
    marks: &[Mark],
    (start, end): (f64, f64),
    bounds: Bounds<Pixels>,
    colors: Colors,
    window: &mut Window,
) {
    let mut lines = PathBuilder::stroke(px(1.)).dash_array(&[px(4.), px(2.)]);
    for mark in marks
        .iter()
        .filter(|mark| start <= mark.at as f64 && mark.at as f64 <= end)
    {
        let x = bounds.left()
            + bounds.size.width * ((mark.at as f64 - start) / (end - start).max(1.)) as f32;
        lines.move_to(point(x, bounds.top()));
        lines.line_to(point(x, bounds.bottom()));
    }
    if let Ok(path) = lines.build() {
        window.paint_path(path, colors.mark.opacity(0.8));
    }
}
