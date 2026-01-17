use anyhow::Result;
use clap::Parser;
use hwlib::cpu::CpuGroup;
use hwlib::motherboard::MotherboardGroup;
use tracing::{Level, info, warn};

#[derive(Parser)]
#[command(name = "lhm-cli")]
#[command(about = "A command line tool for LibreHardwareMonitor")]
struct Args {
    /// Run once and exit
    #[arg(short, long)]
    once: bool,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Skip driver initialization (software-only mode)
    #[arg(long)]
    no_driver: bool,
}

fn check_admin_rights() -> bool {
    use winapi::um::processthreadsapi::GetCurrentProcess;
    use winapi::um::processthreadsapi::OpenProcessToken;
    use winapi::um::securitybaseapi::GetTokenInformation;
    use winapi::um::winnt::{TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut returned_length = 0;

        if GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned_length,
        ) == 0
        {
            return false;
        }

        elevation.TokenIsElevated != 0
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logger
    // 注意：默认的 env_logger 输出目标为 stderr；某些封装器仅显示 stdout。
    // Logging to stdout makes diagnostics visible in more environments.
    let _guard = common::func::tracing_init(None, None, Level::TRACE);

    info!("Starting LibreHardwareMonitor CLI");

    // Check admin rights
    let has_admin = check_admin_rights();

    if !has_admin && !args.no_driver {
        warn!("Not running with administrator privileges. Hardware access will be limited.");
        println!("Warning: This program requires administrator privileges for hardware access.");
        println!("Please run as administrator or use --no-driver for software-only mode.");
        println!("Current capabilities without driver:");
        println!("  ✓ CPU vendor/model identification");
        println!("  ✗ Temperature sensors");
        println!("  ✗ Voltage sensors");
        println!("  ✗ Fan sensors");
        println!();
    }

    // Initialize driver
    let driver_opt = if !args.no_driver {
        match hwlib::driver::init_driver() {
            Ok(()) => {
                info!("Driver initialized successfully");
                match hwlib::driver::get_driver() {
                    Ok(driver) => Some(driver),
                    Err(e) => {
                        warn!("Failed to get driver: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Failed to initialize driver: {}", e);
                if !has_admin {
                    warn!("This is likely due to missing administrator privileges");
                }
                None
            }
        }
    } else {
        info!("Driver initialization skipped (--no-driver)");
        None
    };

    // Initialize hardware groups
    let mut cpu_group = CpuGroup::new();
    let mut motherboard_group = MotherboardGroup::new();

    // Detect CPUs
    if let Err(e) = cpu_group.detect_cpus() {
        warn!("Failed to detect CPUs: {}", e);
    }

    // Set pawn manager if driver is available
    if let Some(driver) = driver_opt
        && let Ok(pm) = driver.pawn_manager()
    {
        cpu_group.set_pawn_manager(pm.clone());
        motherboard_group.set_pawn_manager(pm.clone());
    }

    // Detect motherboards
    if let Err(e) = motherboard_group.detect_motherboards() {
        warn!("Failed to detect motherboards: {}", e);
    }

    info!("Hardware groups initialized");

    if args.once {
        // Run once and exit
        println!("Scanning hardware...");

        // Show CPU info even without driver
        println!("\n=== CPU Information ===");
        if let Some(cpuid) = raw_cpuid::CpuId::new().get_vendor_info() {
            println!("CPU Vendor: {}", cpuid.as_str());
        }

        if let Some(feature_info) = raw_cpuid::CpuId::new().get_feature_info() {
            println!("Family: {:#x}", feature_info.family_id());
            println!("Model: {:#x}", feature_info.model_id());
            println!("Stepping: {:#x}", feature_info.stepping_id());
        }

        // Show detected CPUs and their sensors
        if !cpu_group.cpus().is_empty() {
            println!("\n=== Detected CPUs ===");

            // Update all CPUs first
            if let Err(e) = cpu_group.update_all() {
                warn!("Failed to update CPU sensors: {}", e);
            }

            // Some sensors (e.g. package power from energy counters) need two samples.
            if driver_opt.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Err(e) = cpu_group.update_all() {
                    warn!("Failed to update CPU sensors (second sample): {}", e);
                }
            }

            for (i, cpu) in cpu_group.cpus().iter().enumerate() {
                println!("CPU {}: {}", i, cpu.name());

                if driver_opt.is_some() {
                    // Display sensors
                    let sensors = cpu.sensors();
                    if !sensors.is_empty() {
                        println!("  Sensors:");
                        for sensor in sensors {
                            match sensor.value() {
                                Some(value) => {
                                    println!(
                                        "    {}: {:.2} {:?}",
                                        sensor.name(),
                                        value,
                                        sensor.sensor_type()
                                    );
                                }
                                None => {
                                    println!("    {}: No value available", sensor.name());
                                }
                            }
                        }
                    }
                } else {
                    println!("  Sensors: Driver not available");
                }
            }
        }

        // Show motherboards
        if !motherboard_group.motherboards().is_empty() {
            println!("\n=== Detected Motherboards ===");

            // Update motherboards (may require driver)
            if driver_opt.is_some() {
                if let Err(e) = motherboard_group.update_all() {
                    warn!("Failed to update motherboard sensors: {}", e);
                }
            }

            for (i, mb) in motherboard_group.motherboards().iter().enumerate() {
                println!("Motherboard {}: {}", i, mb.name());

                if driver_opt.is_some() {
                    let sensors = mb.sensors();
                    if !sensors.is_empty() {
                        println!("  Sensors:");
                        for s in sensors {
                            match s.value() {
                                Some(v) => {
                                    println!("    {}: {:.2} {:?}", s.name(), v, s.sensor_type())
                                }
                                None => println!("    {}: No value available", s.name()),
                            }
                        }
                    }
                } else {
                    println!("  Sensors: Driver not available");
                }
            }
        }

        if driver_opt.is_some() {
            println!("\n=== Hardware Sensors ===");
            println!("Driver loaded: Hardware sensor access enabled");
        } else {
            println!("\n=== Hardware Sensors ===");
            println!("Driver not loaded: Hardware sensor access disabled");
            println!("Run with administrator privileges to enable hardware sensors");
        }

        println!("\nHardware scan complete!");
    } else {
        // Continuous monitoring
        println!("Starting continuous monitoring... Press Ctrl+C to stop");

        // Show CPU info once
        println!("\n=== CPU Information ===");
        if let Some(cpuid) = raw_cpuid::CpuId::new().get_vendor_info() {
            println!("CPU Vendor: {}", cpuid.as_str());
        }

        if let Some(feature_info) = raw_cpuid::CpuId::new().get_feature_info() {
            println!("Family: {:#x}", feature_info.family_id());
            println!("Model: {:#x}", feature_info.model_id());
            println!("Stepping: {:#x}", feature_info.stepping_id());
        }

        println!("\n=== Continuous Monitoring ===");

        loop {
            // Update all CPUs
            if let Err(e) = cpu_group.update_all() {
                warn!("Failed to update CPU sensors: {}", e);
            }

            // Some sensors need two samples
            if driver_opt.is_some() {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Err(e) = cpu_group.update_all() {
                    warn!("Failed to update CPU sensors (second sample): {}", e);
                }
            }

            // Clear screen and print current values
            print!("\x1B[2J\x1B[1;1H"); // ANSI escape to clear screen and move to top

            println!(
                "=== CPU Sensors (Updated: {:?}) ===",
                std::time::SystemTime::now()
            );

            for (i, cpu) in cpu_group.cpus().iter().enumerate() {
                println!("CPU {}: {}", i, cpu.name());

                if driver_opt.is_some() {
                    let sensors = cpu.sensors();
                    if !sensors.is_empty() {
                        println!("  Sensors:");
                        for sensor in sensors {
                            match sensor.value() {
                                Some(value) => {
                                    println!(
                                        "    {}: {:.2} {:?}",
                                        sensor.name(),
                                        value,
                                        sensor.sensor_type()
                                    );
                                }
                                None => {
                                    println!("    {}: No value available", sensor.name());
                                }
                            }
                        }
                    }
                } else {
                    println!("  Sensors: Driver not available");
                }
                println!();
            }

            // Update and print motherboard sensors
            if driver_opt.is_some() {
                if let Err(e) = motherboard_group.update_all() {
                    warn!("Failed to update motherboard sensors: {}", e);
                }
            }

            if !motherboard_group.motherboards().is_empty() {
                println!("\n=== Motherboards ===");
                for (i, mb) in motherboard_group.motherboards().iter().enumerate() {
                    println!("Motherboard {}: {}", i, mb.name());
                    let sensors = mb.sensors();
                    if !sensors.is_empty() {
                        println!("  Sensors:");
                        for s in sensors {
                            match s.value() {
                                Some(v) => {
                                    println!("    {}: {:.2} {:?}", s.name(), v, s.sensor_type())
                                }
                                None => println!("    {}: No value available", s.name()),
                            }
                        }
                    } else {
                        println!("  Sensors: none");
                    }
                }
            }

            if driver_opt.is_some() {
                println!("Driver loaded: Hardware sensor access enabled");
            } else {
                println!("Driver not loaded: Hardware sensor access disabled");
            }

            // Wait before next update
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }

    Ok(())
}
