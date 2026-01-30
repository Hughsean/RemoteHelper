use crate::core::{Hardware, HardwareResult, HardwareType, Identifier, Sensor};
use crate::driver::PawnModuleManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tracing;

pub struct Motherboard {
    identifier: Identifier,
    name: String,
    sensors: Vec<Box<dyn Sensor>>,
    properties: HashMap<String, String>,
    pawn_manager: Option<Arc<Mutex<PawnModuleManager>>>,
}

impl Motherboard {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let identifier = Identifier::new(HardwareType::Motherboard, 0, &name);
        let mut properties = HashMap::new();
        properties.insert("Name".to_string(), name.clone());

        Self {
            identifier,
            name,
            sensors: Vec::new(),
            properties,
            pawn_manager: None,
        }
    }

    pub fn set_pawn_manager(&mut self, pm: Arc<Mutex<PawnModuleManager>>) {
        tracing::debug!("Setting pawn_manager for Motherboard {}", self.name);
        self.pawn_manager = Some(pm);
    }
}

impl Hardware for Motherboard {
    fn identifier(&self) -> &Identifier {
        &self.identifier
    }

    fn hardware_type(&self) -> crate::core::HardwareType {
        HardwareType::Motherboard
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn parent(&self) -> Option<&dyn Hardware> {
        None
    }

    fn sensors(&self) -> &[Box<dyn Sensor>] {
        &self.sensors
    }

    fn sub_hardware(&self) -> &[Box<dyn Hardware>] {
        &[]
    }

    fn properties(&self) -> &HashMap<String, String> {
        &self.properties
    }

    fn get_report(&self) -> String {
        format!("Motherboard: {}", self.name)
    }

    fn update(&mut self) -> HardwareResult<()> {
        // If pawn manager available, try to read SuperIO / EC / SMBus registers.
        if let Some(pm) = &self.pawn_manager {
            let mut pm = pm.lock().unwrap();
            tracing::debug!("Updating motherboard sensors for {}", self.name);

            // Try detecting SuperIO chip (placeholder implementation)
            if let Some(chip) = crate::motherboard::superio::detect_superio(&mut pm) {
                tracing::debug!("Detected SuperIO chip: {}", chip.name);
                self.properties.insert("SuperIO".to_string(), chip.name);
            } else {
                tracing::debug!("No SuperIO detected (or detection not implemented)");

                // Try IsaBridge EC MMIO probing as a fallback
                match crate::motherboard::ec::find_superio_mmio(&mut pm) {
                    Some(vec) => {
                        tracing::debug!("IsaBridgeEC reported potential MMIO regions: {:?}", vec);
                        self.properties
                            .insert("IsaBridgeMMIO".to_string(), format!("{:?}", vec));
                    }
                    None => tracing::debug!(
                        "IsaBridgeEC did not find MMIO regions or module unavailable"
                    ),
                }
            }

            // TODO: read actual sensor registers (temperatures/fans/voltages) and populate `self.sensors`
        } else {
            tracing::debug!("No pawn_manager available for motherboard");
        }

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
