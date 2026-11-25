use crate::config::HistoryConfig;
use crate::history::{RecorderHandle, Sample, clock::Clock};
use common::{SampleIdentity, SystemInfo};
use history::{Catalog, Frame, Session};
use nvml_wrapper::{Nvml, enum_wrappers::device::TemperatureSensor};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};
use tokio::sync::{oneshot, watch};

const DEFAULT_PERIOD_MS: u64 = 1_000;

#[derive(Clone)]
pub struct Monitor {
    latest: watch::Sender<Option<Arc<SystemInfo>>>,
    schedule: Arc<(Mutex<Schedule>, Condvar)>,
}

struct Schedule {
    stopped: bool,
    min_ms: u64,
    idle_ms: u64,
    next_id: u64,
    leases: HashMap<u64, (u64, Instant)>,
}

impl Schedule {
    fn period(&mut self, now: Instant) -> Duration {
        self.leases.retain(|_, (_, expires)| *expires > now);
        Duration::from_millis(
            self.leases
                .values()
                .map(|(requested, _)| requested.clamp(&self.min_ms, &self.idle_ms))
                .copied()
                .min()
                .unwrap_or(self.idle_ms),
        )
    }
}

pub struct Lease {
    monitor: Monitor,
    id: u64,
    requested: u64,
}

impl Lease {
    pub fn renew(&mut self, requested: Option<u64>) {
        if let Some(requested) = requested {
            self.requested = requested.min(60_000);
        }
        let (lock, changed) = &*self.monitor.schedule;
        let mut state = lock.lock().unwrap();
        let ttl = Duration::from_millis((2 * self.requested + 5_000).max(10_000));
        state
            .leases
            .insert(self.id, (self.requested, Instant::now() + ttl));
        changed.notify_one();
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let (lock, changed) = &*self.monitor.schedule;
        lock.lock().unwrap().leases.remove(&self.id);
        changed.notify_one();
    }
}

pub struct RunningMonitor {
    monitor: Monitor,
    finished: oneshot::Receiver<()>,
}

impl RunningMonitor {
    pub async fn stop(mut self, timeout: Duration) -> anyhow::Result<()> {
        self.monitor.stop();
        tokio::time::timeout(timeout, &mut self.finished).await??;
        Ok(())
    }
}

impl Drop for RunningMonitor {
    fn drop(&mut self) {
        self.monitor.stop();
    }
}

impl Monitor {
    pub fn new(config: &HistoryConfig) -> Self {
        Self {
            latest: watch::channel(None).0,
            schedule: Arc::new((
                Mutex::new(Schedule {
                    stopped: false,
                    min_ms: config.min_interval_ms,
                    idle_ms: config.idle_interval_ms,
                    next_id: 0,
                    leases: HashMap::new(),
                }),
                Condvar::new(),
            )),
        }
    }

    pub fn configure(&self, config: &HistoryConfig) {
        let (lock, changed) = &*self.schedule;
        let mut state = lock.lock().unwrap();
        state.min_ms = config.min_interval_ms;
        state.idle_ms = config.idle_interval_ms;
        changed.notify_one();
    }

    pub fn lease(&self) -> Lease {
        let mut schedule = self.schedule.0.lock().unwrap();
        let id = schedule.next_id;
        schedule.next_id = id.wrapping_add(1);
        Lease {
            monitor: self.clone(),
            id,
            requested: DEFAULT_PERIOD_MS,
        }
    }

    pub async fn snapshot(&self) -> Option<Arc<SystemInfo>> {
        let mut latest = self.latest.subscribe();
        tokio::time::timeout(Duration::from_millis(500), async {
            latest.wait_for(Option::is_some).await.ok()?.clone()
        })
        .await
        .ok()
        .flatten()
    }

    pub fn spawn(&self, recorder: Option<RecorderHandle>) -> std::io::Result<RunningMonitor> {
        let monitor = self.clone();
        let (done, finished) = oneshot::channel();
        std::thread::Builder::new()
            .name("monitor-sampler".into())
            .spawn(move || {
                monitor.run(recorder);
                let _ = done.send(());
            })?;
        Ok(RunningMonitor {
            monitor: self.clone(),
            finished,
        })
    }

    fn stop(&self) {
        let (lock, changed) = &*self.schedule;
        lock.lock().unwrap().stopped = true;
        changed.notify_one();
    }

    fn run(&self, recorder: Option<RecorderHandle>) {
        let (lock, changed) = &*self.schedule;
        let mut reader = Reader::new();
        let clock = Clock::new();
        let session = Session {
            session_id: rand::random(),
            boot_id: clock.boot_id,
        };
        let catalog = Catalog::standard(1);
        let mut seq = 0;
        let mut last_start = None;
        let mut last_finish = None;
        loop {
            let (period, start) = {
                let mut state = lock.lock().unwrap();
                loop {
                    if state.stopped {
                        return;
                    }
                    let now = Instant::now();
                    let period = state.period(now);
                    let target = last_start.map_or(now, |start: Instant| {
                        let due = start + period;
                        // 硬件读取超时后跳过已错过的采样点，不集中补采。
                        if last_finish.is_some_and(|finish| finish >= due) {
                            last_finish.unwrap_or(due) + period
                        } else {
                            due
                        }
                    });
                    if target <= now {
                        break (period, now);
                    }
                    let wake = state
                        .leases
                        .values()
                        .map(|(_, deadline)| *deadline)
                        .min()
                        .map_or(target, |expiry| expiry.min(target));
                    state = changed
                        .wait_timeout(state, wake.saturating_duration_since(now))
                        .unwrap()
                        .0;
                }
            };
            let evidence = clock.read();
            let period_ms = period.as_millis() as u32;
            let sample_identity = SampleIdentity {
                session_id: session.session_id,
                sample_seq: seq,
            };
            let info = reader.read(
                start,
                evidence.tick_ms.zip(evidence.awake_ms),
                period_ms,
                evidence.wall_ms.unwrap_or_default() as u64,
                sample_identity,
            );
            last_start = Some(start);
            last_finish = Some(Instant::now());
            let frame = recorder.as_ref().map(|_| Frame {
                sample_seq: seq,
                time_ms: evidence.wall_ms.unwrap_or_default(),
                period_ms,
                clock: evidence,
                values: values(&info, &catalog),
            });
            self.latest.send_replace(Some(Arc::new(info)));
            if let (Some(recorder), Some(frame)) = (&recorder, frame) {
                recorder.record(Sample { session, frame });
            }
            seq += 1;
        }
    }
}

fn values(info: &SystemInfo, catalog: &Catalog) -> Vec<Option<u64>> {
    catalog
        .channels
        .iter()
        .map(|channel| {
            let value = match channel.key.as_str() {
                "cpu.usage" => Some(f64::from(info.cpu_usage)),
                "gpu.usage" => info.gpu_usage.map(f64::from),
                "cpu.temp" => info.cpu_temperature.map(f64::from),
                "gpu.temp" => info.gpu_temperature.map(f64::from),
                "cpu.power" => info.cpu_package_power.map(f64::from),
                "gpu.power" => info.gpu_power_watts.map(f64::from),
                "mem.used" => return (info.total_memory != 0).then_some(info.memory_usage),
                "mem.total" => return (info.total_memory != 0).then_some(info.total_memory),
                "gpu.mem.used" => return info.gpu_memory_usage,
                "gpu.mem.total" => return info.gpu_total_memory,
                "net.rx.rate" => {
                    return (info.network_rx_speed_valid == Some(true))
                        .then_some(info.network_rx_speed);
                }
                "net.tx.rate" => {
                    return (info.network_tx_speed_valid == Some(true))
                        .then_some(info.network_tx_speed);
                }
                "uptime" => return Some(info.uptime),
                _ => None,
            };
            value.and_then(|value| channel.quantize(value))
        })
        .collect()
}

struct Reader {
    sys: System,
    networks: Networks,
    last_net: Option<NetReading>,
    nvml: Option<Nvml>,
    nvml_retry: Instant,
    sensor_hub: Option<hwlib::sensors::SensorHub>,
}

type Counters = HashMap<String, (u64, u64)>;
struct NetReading {
    at: Instant,
    clock_ms: Option<(u64, u64)>,
    period_ms: u32,
    counters: Counters,
}

impl Reader {
    fn new() -> Self {
        Self {
            sys: System::new(),
            networks: Networks::new(),
            last_net: None,
            nvml: None,
            nvml_retry: Instant::now(),
            sensor_hub: None,
        }
    }

    fn read(
        &mut self,
        at: Instant,
        clock_ms: Option<(u64, u64)>,
        period_ms: u32,
        timestamp: u64,
        identity: SampleIdentity,
    ) -> SystemInfo {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        let mut info = SystemInfo {
            timestamp,
            sample_identity: Some(identity),
            period_ms: Some(period_ms),
            cpu_usage: self.sys.global_cpu_usage(),
            cpu_model: self
                .sys
                .cpus()
                .first()
                .map_or_else(String::new, |cpu| cpu.brand().to_owned()),
            memory_usage: self.sys.used_memory(),
            total_memory: self.sys.total_memory(),
            uptime: System::uptime(),
            ..Default::default()
        };
        self.networks.refresh(true);
        let counters: Counters = self
            .networks
            .iter()
            .map(|(name, data)| {
                (
                    name.clone(),
                    (data.total_received(), data.total_transmitted()),
                )
            })
            .collect();
        for &(rx, tx) in counters.values() {
            info.network_rx_bytes = info.network_rx_bytes.saturating_add(rx);
            info.network_tx_bytes = info.network_tx_bytes.saturating_add(tx);
        }
        let current = NetReading {
            at,
            clock_ms,
            period_ms,
            counters,
        };
        let (rx, tx) = self
            .last_net
            .as_ref()
            .map_or((None, None), |previous| network_rates(previous, &current));
        self.last_net = Some(current);
        info.network_rx_speed_valid = Some(rx.is_some());
        info.network_rx_speed = rx.unwrap_or(0);
        info.network_tx_speed_valid = Some(tx.is_some());
        info.network_tx_speed = tx.unwrap_or(0);

        if self.nvml.is_none() && at >= self.nvml_retry {
            self.nvml = Nvml::init().ok();
            self.nvml_retry = at + Duration::from_secs(30);
        }
        if let Some(nvml) = &self.nvml
            && let Ok(device) = nvml.device_by_index(0)
        {
            info.gpu_usage = device.utilization_rates().ok().map(|rates| rates.gpu);
            if let Ok(memory) = device.memory_info() {
                info.gpu_memory_usage = Some(memory.used);
                info.gpu_total_memory = Some(memory.total);
            }
            info.gpu_model = device.name().ok();
            info.gpu_temperature = device
                .temperature(TemperatureSensor::Gpu)
                .ok()
                .map(|t| t as f32);
            info.gpu_power_watts = device.power_usage().ok().map(|mw| mw as f32 / 1000.0);
        }
        hwlib::sensors::ensure_sensor_hub(&mut self.sensor_hub, "monitor");
        if let Some(hub) = &mut self.sensor_hub {
            match hub.read_all() {
                Ok((readings, _)) => {
                    for reading in readings {
                        if let Some(value) = reading.value.filter(|v| v.value.is_finite()) {
                            match reading.sensor_type {
                                hwlib::core::SensorType::Temperature => {
                                    info.cpu_temperature = Some(value.value)
                                }
                                hwlib::core::SensorType::Power => {
                                    info.cpu_package_power = Some(value.value)
                                }
                            }
                        }
                    }
                }
                Err(err) => tracing::debug!(%err, "读取 CPU 传感器失败"),
            }
        }
        info
    }
}

fn network_rates(previous: &NetReading, current: &NetReading) -> (Option<u64>, Option<u64>) {
    let awake = current
        .at
        .saturating_duration_since(previous.at)
        .as_millis();
    let elapsed = match (previous.clock_ms, current.clock_ms) {
        (Some((tick_a, awake_a)), Some((tick_b, awake_b))) => {
            match (tick_b.checked_sub(tick_a), awake_b.checked_sub(awake_a)) {
                (Some(tick), Some(awake)) if tick.saturating_sub(awake) < 3_000 => u128::from(tick),
                _ => return (None, None),
            }
        }
        _ => awake,
    };
    let threshold = u128::from((3 * previous.period_ms.max(current.period_ms)).max(10_000));
    if elapsed == 0
        || elapsed > threshold
        || previous.counters.len() != current.counters.len()
        || current.counters.is_empty()
    {
        return (None, None);
    }
    let (mut rx, mut tx) = (Some(0u128), Some(0u128));
    for (name, &(new_rx, new_tx)) in &current.counters {
        let Some(&(old_rx, old_tx)) = previous.counters.get(name) else {
            return (None, None);
        };
        rx = rx
            .zip(new_rx.checked_sub(old_rx))
            .map(|(sum, delta)| sum + u128::from(delta));
        tx = tx
            .zip(new_tx.checked_sub(old_tx))
            .map(|(sum, delta)| sum + u128::from(delta));
    }
    let per_second = |bytes: u128| u64::try_from(bytes.saturating_mul(1_000) / elapsed).ok();
    (rx.and_then(per_second), tx.and_then(per_second))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_use_finest_effective_period_and_expire() {
        let monitor = Monitor::new(&HistoryConfig::default());
        let mut slow = monitor.lease();
        slow.renew(Some(60_000));
        let mut fast = monitor.lease();
        fast.renew(Some(0));
        let now = Instant::now();
        let period = |at| monitor.schedule.0.lock().unwrap().period(at).as_millis();
        assert_eq!(period(now), 250);
        fast.renew(None);
        assert_eq!(period(now), 250);
        drop(fast);
        assert_eq!(period(now), 5_000);
        assert_eq!(monitor.schedule.0.lock().unwrap().leases.len(), 1);
        period(now + Duration::from_secs(121));
        assert_eq!(monitor.schedule.0.lock().unwrap().leases.len(), 1);
        period(now + Duration::from_secs(126));
        assert!(monitor.schedule.0.lock().unwrap().leases.is_empty());
    }

    #[test]
    fn network_rates_use_elapsed_and_reject_reset_and_sleep() {
        let now = Instant::now();
        let sample = |ms, tick, rx, tx| NetReading {
            at: now + Duration::from_millis(ms),
            clock_ms: Some((tick, ms)),
            period_ms: 1000,
            counters: [("eth0".into(), (rx, tx))].into(),
        };
        let first = sample(0, 100, 10, 100);
        assert_eq!(
            network_rates(&first, &sample(750, 850, 310, 400)),
            (Some(400), Some(400))
        );
        assert_eq!(
            network_rates(&first, &sample(750, 850, 5, 400)),
            (None, Some(400))
        );
        assert_eq!(
            network_rates(&first, &sample(750, 8_500, 310, 400)),
            (None, None)
        );
    }
}
