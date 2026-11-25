use history::{ClockEvidence, Frame, GapReason, Id, Session};
#[cfg(not(windows))]
use std::time::Instant;
use std::time::SystemTime;

const TOLERANCE_MS: i128 = 3_000;

pub(crate) struct Clock {
    pub boot_id: Option<Id>,
    #[cfg(not(windows))]
    start: Instant,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            boot_id: windows::boot_id(),
            #[cfg(not(windows))]
            boot_id: None,
            #[cfg(not(windows))]
            start: Instant::now(),
        }
    }

    pub fn read(&self) -> ClockEvidence {
        let wall_ms = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok());
        #[cfg(windows)]
        let (tick_ms, awake_ms) = windows::read();
        // 此计时只在当前进程内有效，不能作为跨进程的开机证据。
        #[cfg(not(windows))]
        let (tick_ms, awake_ms) = (None, u64::try_from(self.start.elapsed().as_millis()).ok());
        ClockEvidence {
            wall_ms,
            tick_ms,
            awake_ms,
        }
    }
}

pub(super) fn classify(
    previous_session: &Session,
    previous: &Frame,
    clean_exit: bool,
    next_session: &Session,
    next: &Frame,
) -> Option<GapReason> {
    let wall = i128::from(next.time_ms) - i128::from(previous.time_ms);
    let delta =
        |first: Option<u64>, last: Option<u64>| Some(i128::from(last?) - i128::from(first?));
    let tick = delta(previous.clock.tick_ms, next.clock.tick_ms);
    let awake = delta(previous.clock.awake_ms, next.clock.awake_ms);

    if previous_session.session_id != next_session.session_id {
        let rebooted = match (previous_session.boot_id, next_session.boot_id) {
            (Some(first), Some(last)) if first != last => true,
            (Some(_), Some(_)) => {
                if tick.is_some_and(|elapsed| elapsed < 0)
                    || (cfg!(windows) && awake.is_some_and(|elapsed| elapsed < 0))
                {
                    tracing::warn!("历史时钟证据冲突：开机标识相同但开机计时回退");
                }
                false
            }
            _ => tick.is_some_and(|elapsed| elapsed < 0),
        };
        return Some(if rebooted {
            GapReason::Rebooted
        } else if clean_exit {
            GapReason::Stopped
        } else {
            GapReason::Interrupted
        });
    }

    if let Some(elapsed) = tick.or(awake).filter(|elapsed| *elapsed >= 0) {
        if (wall - elapsed).abs() > TOLERANCE_MS {
            return Some(GapReason::ClockJump);
        }
        if let (Some(tick), Some(awake)) = (tick, awake)
            && awake >= 0
            && tick - awake >= TOLERANCE_MS
        {
            return Some(GapReason::Sleep);
        }
        let threshold = (3 * i128::from(previous.period_ms.max(next.period_ms))).max(10_000);
        return (elapsed > threshold).then_some(GapReason::Stalled);
    }

    if wall < 0 {
        Some(GapReason::ClockJump)
    } else {
        let threshold = (3 * i128::from(previous.period_ms.max(next.period_ms))).max(10_000);
        (wall > threshold).then_some(GapReason::Unknown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: u8, boot: Option<u8>) -> Session {
        Session {
            session_id: [id; 16],
            boot_id: boot.map(|id| [id; 16]),
        }
    }

    fn frame(wall: i64, tick: u64, awake: u64, period_ms: u32) -> Frame {
        Frame {
            sample_seq: 0,
            time_ms: wall,
            period_ms,
            clock: ClockEvidence {
                wall_ms: Some(wall),
                tick_ms: Some(tick),
                awake_ms: Some(awake),
            },
            values: Vec::new(),
        }
    }

    #[test]
    fn differing_boot_ids_override_elapsed_and_exit_marker() {
        assert_eq!(
            classify(
                &session(1, Some(1)),
                &frame(30_000, 20_000, 20_000, 1000),
                true,
                &session(2, Some(2)),
                &frame(60_000, 30_000, 30_000, 1000)
            ),
            Some(GapReason::Rebooted)
        );
        assert_eq!(
            classify(
                &session(1, Some(1)),
                &frame(30_000, 20_000, 20_000, 1000),
                true,
                &session(2, None),
                &frame(60_000, 3_000, 3_000, 1000)
            ),
            Some(GapReason::Rebooted)
        );
        assert_eq!(
            classify(
                &session(1, Some(1)),
                &frame(30_000, 20_000, 20_000, 1000),
                true,
                &session(2, Some(1)),
                &frame(60_000, 3_000, 3_000, 1000)
            ),
            Some(GapReason::Stopped)
        );
    }

    #[test]
    fn uses_signed_deltas_and_the_slower_transition_period() {
        let s = session(1, Some(1));
        let a = frame(30_000, 20_000, 20_000, 5_000);
        assert_eq!(
            classify(&s, &a, false, &s, &frame(43_000, 33_000, 33_000, 250)),
            None
        );
        assert_eq!(
            classify(&s, &a, false, &s, &frame(47_000, 37_000, 37_000, 250)),
            Some(GapReason::Stalled)
        );
        assert_eq!(
            classify(&s, &a, false, &s, &frame(36_000, 26_000, 21_000, 250)),
            Some(GapReason::Sleep)
        );
        assert_eq!(
            classify(&s, &a, false, &s, &frame(24_000, 26_000, 21_000, 250)),
            Some(GapReason::ClockJump)
        );
    }
}

#[cfg(windows)]
mod windows {
    use super::Id;
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTickCount64() -> u64;
        fn QueryUnbiasedInterruptTime(time: *mut u64) -> i32;
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    pub(super) fn read() -> (Option<u64>, Option<u64>) {
        let mut awake = 0;
        // 安全：输出参数指向一个可写的 ULONGLONG。
        unsafe {
            (
                Some(GetTickCount64()),
                (QueryUnbiasedInterruptTime(&mut awake) != 0).then_some(awake / 10_000),
            )
        }
    }

    pub(super) fn boot_id() -> Option<Id> {
        type Query = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
        const SYSTEM_BOOT_ENVIRONMENT_INFORMATION: u32 = 90;
        const NTDLL: [u16; 10] = [110, 116, 100, 108, 108, 46, 100, 108, 108, 0];
        let mut buf = [0u64; 8]; // GUID 从偏移零开始，缓冲区按原生结构对齐。
        let mut len = 0u32;
        // 安全：进程已加载 ntdll，动态查询函数使用声明的 NT 调用约定，输出缓冲区可写。
        unsafe {
            let module = GetModuleHandleW(NTDLL.as_ptr());
            if module.is_null() {
                return None;
            }
            let addr = GetProcAddress(module, c"NtQuerySystemInformation".as_ptr().cast());
            if addr.is_null() {
                return None;
            }
            let query: Query = std::mem::transmute(addr);
            if query(
                SYSTEM_BOOT_ENVIRONMENT_INFORMATION,
                buf.as_mut_ptr().cast(),
                64,
                &mut len,
            ) < 0
                || !(16..=64).contains(&len)
            {
                return None;
            }
            let mut id = [0u8; 16];
            id[..8].copy_from_slice(&buf[0].to_le_bytes());
            id[8..].copy_from_slice(&buf[1].to_le_bytes());
            // 将 Windows 内存中的 GUID 字节序转换成 UUID 字符串字节序。
            id[..4].reverse();
            id[4..6].reverse();
            id[6..8].reverse();
            (id != [0; 16]).then_some(id)
        }
    }
}
