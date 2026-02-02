use hwlib::sensors::SensorHub;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    // Init tracing to file + stdout at TRACE level
    let _guard =
        common::func::tracing_init(Some("logs"), Some("load_test.log"), tracing::Level::TRACE);
    tracing::info!("Starting load test");

    // Init driver
    if let Err(e) = hwlib::driver::init_driver() {
        tracing::error!("Failed to init hwlib driver: {}", e);
        return;
    }

    // Prepare sensor hub and pawn manager
    let mut sensor_hub = SensorHub::new();
    let pm = match hwlib::driver::get_driver().and_then(|d| d.pawn_manager()) {
        Ok(pm) => pm,
        Err(e) => {
            tracing::error!("Failed to get pawn manager: {}", e);
            return;
        }
    };

    sensor_hub.set_pawn_manager(pm.clone());

    if let Err(e) = sensor_hub.detect() {
        tracing::warn!("SensorHub detect failed: {}", e);
    } else {
        tracing::info!("SensorHub detect OK");
    }

    // CPU load control flag
    let load_flag = Arc::new(AtomicBool::new(false));
    // Spawn a worker thread that will busy-loop when load_flag is true
    let lf = load_flag.clone();
    let _worker = thread::spawn(move || {
        loop {
            if lf.load(Ordering::Relaxed) {
                // Busy work for a short span
                let mut x: u64 = 0;
                for _ in 0..1_000_000 {
                    x = x.wrapping_mul(1234567).wrapping_add(891011);
                }
                // yield
                thread::yield_now();
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
    });

    // Constants for MSR registers
    const MSR_PKG_ENERGY_STAT: u32 = 0xC001_029B;
    const MSR_PWR_UNIT: u32 = 0xC001_0299;

    // We'll toggle load: collect samples for 5s, then enable load for 6s, then disable for 5s
    let sample_interval = Duration::from_millis(500);
    let mut samples: Vec<(Instant, Option<u64>, Option<u64>, Option<f32>, Option<f32>)> =
        Vec::new();

    let start = Instant::now();
    let total_duration = Duration::from_secs(600);

    while start.elapsed() < total_duration {
        let t = Instant::now();

        // toggle load at around 5s..11s
        let elapsed = start.elapsed().as_secs();
        if (5..11).contains(&elapsed) {
            if !load_flag.load(Ordering::Relaxed) {
                tracing::info!("Enabling CPU load");
                load_flag.store(true, Ordering::Relaxed);
            }
        } else if load_flag.load(Ordering::Relaxed) {
            tracing::info!("Disabling CPU load");
            load_flag.store(false, Ordering::Relaxed);
        }

        // Read raw MSRs via pawn manager
        let mut raw_pkg: Option<u64> = None;
        let mut raw_unit: Option<u64> = None;

        match pm.lock() {
            Ok(mut pm_locked) => {
                match pm_locked.read_msr_amd(MSR_PKG_ENERGY_STAT) {
                    Ok(v) => raw_pkg = Some(v),
                    Err(e) => tracing::warn!("read_msr_amd(PKG) failed: {}", e),
                }
                match pm_locked.read_msr_amd(MSR_PWR_UNIT) {
                    Ok(v) => raw_unit = Some(v),
                    Err(e) => tracing::warn!("read_msr_amd(UNIT) failed: {}", e),
                }
            }
            Err(e) => tracing::warn!("Failed to lock pawn_manager: {}", e),
        }

        // Read sensor Hub readings
        let mut temp: Option<f32> = None;
        let mut power: Option<f32> = None;

        match sensor_hub.read_all() {
            Ok((cpu_readings, _mb)) => {
                for r in cpu_readings.iter() {
                    if r.sensor_type == hwlib::core::SensorType::Temperature
                        && let Some(v) = &r.value
                    {
                        temp = Some(v.value);
                    }
                    if r.sensor_type == hwlib::core::SensorType::Power
                        && let Some(v) = &r.value
                    {
                        power = Some(v.value);
                    }
                }
            }
            Err(e) => tracing::warn!("SensorHub read_all failed: {}", e),
        }

        tracing::trace!(
            "sample @ {:?}: raw_pkg={:?}, raw_unit={:?}, temp={:?}, power={:?}",
            t,
            raw_pkg,
            raw_unit,
            temp,
            power
        );
        samples.push((t, raw_pkg, raw_unit, temp, power));

        thread::sleep(sample_interval);
    }

    tracing::info!("Test complete, printing summary of samples:");
    for (i, (t, raw_pkg, raw_unit, temp, power)) in samples.iter().enumerate() {
        println!(
            "{:02}: {:?} | raw_pkg={:?} raw_unit={:?} temp={:?} power={:?}",
            i,
            t.elapsed(),
            raw_pkg,
            raw_unit,
            temp,
            power
        );
    }

    tracing::info!("Load test finished");
}
