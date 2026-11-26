use std::time::{Duration, Instant};

const PERCENT_TOPS: [u32; 7] = [10, 20, 25, 40, 50, 75, 100];
const TRANSITION: Duration = Duration::from_millis(180);

fn percent_top(peak: Option<f32>) -> u32 {
    peak.and_then(|peak| {
        PERCENT_TOPS
            .into_iter()
            .find(|&top| peak * 106. <= top as f32)
    })
    .unwrap_or(100)
}

fn marks(top: u32) -> impl Iterator<Item = f32> {
    let rows = [4, 5, 3]
        .into_iter()
        .find(|rows| top % rows == 0)
        .unwrap_or(4);
    (0..=rows)
        .rev()
        .map(move |row| (top * row) as f32 / rows as f32)
}

#[derive(Default)]
pub(super) struct PercentAxis {
    motion: Option<Motion>,
}

struct Motion {
    from: f32,
    previous: u32,
    target: u32,
    started: Instant,
}

impl Motion {
    fn progress(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.started).as_secs_f32()
            / TRANSITION.as_secs_f32())
        .min(1.);
        1. - (1. - t).powi(3)
    }

    fn top(&self, progress: f32) -> f32 {
        // 以零点为锚，按比例过渡轴上限。
        if progress >= 1. {
            self.target as f32
        } else {
            self.from * (self.target as f32 / self.from).powf(progress)
        }
    }
}

impl PercentAxis {
    pub(super) fn update(
        &mut self,
        peak: Option<f32>,
        now: Instant,
        animate: bool,
    ) -> (f32, Vec<(f32, f32, f32)>, bool) {
        let target = percent_top(peak);
        let active = animate && peak.is_some();
        let Some(motion) = self.motion.as_mut().filter(|_| active) else {
            self.motion = Some(Motion {
                from: target as f32,
                previous: target,
                target,
                started: now,
            });
            return (
                target as f32 / 100.,
                marks(target)
                    .map(|value| (value, 1. - value / target as f32, 1.))
                    .collect(),
                false,
            );
        };
        if motion.target != target {
            *motion = Motion {
                from: motion.top(motion.progress(now)),
                previous: motion.target,
                target,
                started: now,
            };
        }
        let progress = motion.progress(now);
        let top = motion.top(progress);
        let moving = progress < 1. && motion.from != motion.target as f32;
        let mut ticks: Vec<_> = marks(target)
            .map(|value| {
                let shared = marks(motion.previous).any(|old| old == value);
                (
                    value,
                    1. - value / top,
                    if moving && !shared { progress } else { 1. },
                )
            })
            .collect();
        if moving {
            ticks.extend(
                marks(motion.previous)
                    .filter(|&old| !marks(target).any(|new| new == old))
                    .map(|value| (value, 1. - value / top, 1. - progress)),
            );
        }
        ticks.retain(|&(_, at, alpha)| at >= -0.0001 && alpha > 0.);
        (top / 100., ticks, moving)
    }
}

#[cfg(test)]
mod tests {
    use super::{PercentAxis, TRANSITION, marks, percent_top};
    use std::time::Instant;

    #[test]
    fn percent_axis_uses_round_ticks_and_headroom() {
        assert_eq!(percent_top(None), 100);
        assert_eq!(percent_top(Some(0.)), 10);
        assert_eq!(percent_top(Some(0.19)), 25);
        assert_eq!(percent_top(Some(1.)), 100);
        assert_eq!(marks(25).collect::<Vec<_>>(), [25., 20., 15., 10., 5., 0.]);
        assert_eq!(marks(75).collect::<Vec<_>>(), [75., 60., 45., 30., 15., 0.]);
    }

    #[test]
    fn axis_animates_and_honors_reduced_motion() {
        let mut axis = PercentAxis::default();
        let now = Instant::now();
        assert_eq!(axis.update(Some(0.08), now, true).0, 0.1);
        assert!(axis.update(Some(0.7), now, true).2);
        let top = axis.update(Some(0.7), now + TRANSITION / 2, true).0;
        assert!(top > 0.1 && top < 0.75);
        let (top, _, moving) = axis.update(Some(0.7), now + TRANSITION, true);
        assert_eq!(top, 0.75);
        assert!(!moving);
        assert_eq!(axis.update(Some(0.08), now + TRANSITION, false).0, 0.1);
    }
}
