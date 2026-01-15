use anyhow::Result;
use hwlib::motherboard::MotherboardGroup;
use tracing::{info, warn};

fn main() -> Result<()> {
    // init tracing
    let _guard = common::func::tracing_init(None, None);
    info!("Starting motherboard test CLI");

    // Initialize driver
    let driver_opt = match hwlib::driver::init_driver() {
        Ok(()) => match hwlib::driver::get_driver() {
            Ok(driver) => Some(driver),
            Err(e) => {
                warn!("Failed to get driver: {}", e);
                None
            }
        },
        Err(e) => {
            warn!("Failed to init driver: {}", e);
            None
        }
    };

    let mut mb_group = MotherboardGroup::new();

    if let Some(driver) = driver_opt {
        if let Ok(pm) = driver.pawn_manager() {
            mb_group.set_pawn_manager(pm.clone());

            // Run SuperIO diagnostic and print summary
            if let Ok(pm_ref) = driver.pawn_manager() {
                let pm_arc = pm_ref.clone();
                let mut pm = pm_arc.lock().unwrap();
                let diag = hwlib::motherboard::superio::diagnose(&mut pm);
                println!("\n--- SuperIO Diagnostic ---");
                for line in diag {
                    println!("  {}", line);
                }
                println!("--- End SuperIO Diagnostic ---\n");

                // Run EC (IsaBridge) diagnostic if available
                let ec_diag = hwlib::motherboard::ec::diagnose_ec(&mut pm);
                println!("\n--- EC (IsaBridge) Diagnostic ---");
                for line in ec_diag {
                    println!("  {}", line);
                }
                println!("--- End EC Diagnostic ---\n");
            }
        }
    }

    if let Err(e) = mb_group.detect_motherboards() {
        warn!("Failed to detect motherboards: {}", e);
    }

    if let Err(e) = mb_group.update_all() {
        warn!("Failed to update motherboard sensors: {}", e);
    }

    for mb in mb_group.motherboards() {
        println!("Motherboard: {}", mb.name());
        let sensors = mb.sensors();
        if sensors.is_empty() {
            println!("  Sensors: none detected or driver not available");
        } else {
            for s in sensors {
                match s.value() {
                    Some(v) => println!("  {}: {:.2} {:?}", s.name(), v, s.sensor_type()),
                    None => println!("  {}: no value", s.name()),
                }
            }
        }
    }

    Ok(())
}
