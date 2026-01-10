use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use crate::driver::error::{DriverError, DriverResult};

use tracing;

/// Embedded PawnIO 0.2.1 driver binaries
pub struct DriverResource {
    temp_dir: PathBuf,
    extracted_paths: Vec<PathBuf>,
}

impl DriverResource {
    /// Create new driver resource manager
    pub fn new() -> DriverResult<Self> {
        let temp_dir = std::env::temp_dir().join("lhm_pawnio");
        fs::create_dir_all(&temp_dir)?;

        Ok(Self {
            temp_dir,
            extracted_paths: Vec::new(),
        })
    }

    /// Extract the main driver binary
    pub fn extract_driver(&mut self) -> DriverResult<PathBuf> {
        // Extract as .sys file (required for Windows driver loading)
        // Using AMD Family 17h driver which is compatible with Family 19h (Zen 4)
        self.extract_binary("AMDFamily17.sys", AMD_FAMILY_17_BIN, AMD_FAMILY_17_HASH)
    }

    /// Extract AMD Family 17h driver
    pub fn extract_amd_family17(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("AMDFamily17.bin", AMD_FAMILY_17_BIN, AMD_FAMILY_17_HASH)
    }

    /// Extract Ryzen SMU driver
    pub fn extract_ryzen_smu(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("RyzenSMU.bin", RYZEN_SMU_BIN, RYZEN_SMU_HASH)
    }

    /// Extract Isa Bridge EC driver (PawnIO 0.2.1 feature)
    pub fn extract_isa_bridge_ec(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("IsaBridgeEC.bin", ISA_BRIDGE_EC_BIN, ISA_BRIDGE_EC_HASH)
    }

    /// Extract Intel MSR driver
    pub fn extract_intel_msr(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("IntelMSR.bin", INTEL_MSR_BIN, INTEL_MSR_HASH)
    }

    /// Extract LPC ACPI EC driver
    pub fn extract_lpc_acpi_ec(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("LpcACPIEC.bin", LPC_ACPI_EC_BIN, LPC_ACPI_EC_HASH)
    }

    /// Extract LPC CrOS EC driver
    pub fn extract_lpc_cros_ec(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("LpcCrOSEC.bin", LPC_CROS_EC_BIN, LPC_CROS_EC_HASH)
    }

    /// Extract LPC IO driver
    pub fn extract_lpc_io(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("LpcIO.bin", LPC_IO_BIN, LPC_IO_HASH)
    }

    /// Extract SMBus I801 driver
    pub fn extract_smbus_i801(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("SmbusI801.bin", SMBUS_I801_BIN, SMBUS_I801_HASH)
    }

    /// Extract SMBus NCT6793 driver
    pub fn extract_smbus_nct6793(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("SmbusNCT6793.bin", SMBUS_NCT6793_BIN, SMBUS_NCT6793_HASH)
    }

    /// Extract SMBus PIIX4 driver
    pub fn extract_smbus_piix4(&mut self) -> DriverResult<PathBuf> {
        self.extract_binary("SmbusPIIX4.bin", SMBUS_PIIX4_BIN, SMBUS_PIIX4_HASH)
    }

    /// Generic binary extraction with hash verification
    fn extract_binary(&mut self, filename: &str, data: &[u8], expected_hash: &str) -> DriverResult<PathBuf> {
        // Verify hash of embedded data
        let mut hasher = Sha256::new();
        hasher.update(data);
        let actual_hash = format!("{:x}", hasher.finalize());

        if actual_hash != expected_hash {
            return Err(DriverError::HashMismatch {
                expected: expected_hash.to_string(),
                actual: actual_hash,
            });
        }

        // Write to temp file
        let file_path = self.temp_dir.join(filename);
        fs::write(&file_path, data)?;

        self.extracted_paths.push(file_path.clone());
        tracing::info!("Extracted {} to {:?}", filename, file_path);

        Ok(file_path)
    }

    /// Cleanup all extracted files
    pub fn cleanup(&mut self) -> DriverResult<()> {
        for path in &self.extracted_paths {
            if path.exists() {
                if let Err(e) = fs::remove_file(path) {
                    tracing::warn!("Failed to remove {}: {}", path.display(), e);
                } else {
                    tracing::info!("Removed {}", path.display());
                }
            }
        }
        self.extracted_paths.clear();

        // Remove temp directory if empty
        if self.temp_dir.exists() {
            if let Err(e) = fs::remove_dir(&self.temp_dir) {
                tracing::debug!("Failed to remove temp directory (may not be empty): {}", e);
            }
        }

        Ok(())
    }
}

impl Drop for DriverResource {
    fn drop(&mut self) {
        if let Err(e) = self.cleanup() {
            tracing::error!("Failed to cleanup driver resources: {}", e);
        }
    }
}

// Embedded driver binaries from PawnIO 0.2.1 release
// SHA256 hashes verified for integrity

// AMD Family 17h driver (Zen/Zen+/Zen2/Zen3/Zen4 architectures)
// This driver works for AMD Family 17h and 19h (Zen through Zen 4)
const AMD_FAMILY_17_BIN: &[u8] = include_bytes!("../../../PawnIO/AMDFamily17.bin");
const AMD_FAMILY_17_HASH: &str = "374d4bc3e88284d08f2c65e292df5340c6a034affc30b614db6e780d7094d117";

// AMD Family 0F driver (legacy)
const AMD_FAMILY_0F_BIN: &[u8] = include_bytes!("../../../PawnIO/AMDFamily0F.bin");
const AMD_FAMILY_0F_HASH: &str = "1788550c02100ad6bbc91604399ed6e055058ea0f6c0c828c02d9b01a09ad27f";

// AMD Family 10h driver (legacy)
const AMD_FAMILY_10_BIN: &[u8] = include_bytes!("../../../PawnIO/AMDFamily10.bin");
const AMD_FAMILY_10_HASH: &str = "79be1396621aa44eb149c5dc6d1bab4519b9cfa49a32af5e022cb9f9ca887658";

// Ryzen SMU driver (enhanced in 0.2.1)
const RYZEN_SMU_BIN: &[u8] = include_bytes!("../../../PawnIO/RyzenSMU.bin");
const RYZEN_SMU_HASH: &str = "8cec3a2d03b19d585fd75e36be2875fde8825834968509582d4351201dc2871a";

// Isa Bridge EC driver (NEW in PawnIO 0.2.1)
const ISA_BRIDGE_EC_BIN: &[u8] = include_bytes!("../../../PawnIO/IsaBridgeEC.bin");
const ISA_BRIDGE_EC_HASH: &str = "7218c633384bd019cce09e8df91f74996f980765ac44dde0f22b0b5318c20f03";

// Intel MSR driver
const INTEL_MSR_BIN: &[u8] = include_bytes!("../../../PawnIO/IntelMSR.bin");
const INTEL_MSR_HASH: &str = "d09fa2d4232f92d9902fc90b058adc55ae5469b9b6f2f3f1441184796945bad1";

// LPC ACPI EC driver
const LPC_ACPI_EC_BIN: &[u8] = include_bytes!("../../../PawnIO/LpcACPIEC.bin");
const LPC_ACPI_EC_HASH: &str = "c38fd116e7aff4d1fdb0a494e296be0a6708e5a22fc72f14587442fb7f8f7906";

// LPC CrOS EC driver
const LPC_CROS_EC_BIN: &[u8] = include_bytes!("../../../PawnIO/LpcCrOSEC.bin");
const LPC_CROS_EC_HASH: &str = "277ed6ce6b5f647d5dae9b06c83ed39e904974b06f95598d49f4737237974776";

// LPC IO driver
const LPC_IO_BIN: &[u8] = include_bytes!("../../../PawnIO/LpcIO.bin");
const LPC_IO_HASH: &str = "3dcf8b2bc80ff642d97c4608511a818642b5bf315ff53df3df393d043e71d101";

// SMBus I801 driver
const SMBUS_I801_BIN: &[u8] = include_bytes!("../../../PawnIO/SmbusI801.bin");
const SMBUS_I801_HASH: &str = "76b082a144027244fe7bbf3ca0e987e7dfa279aa87e0ccc2a6a918e10e2038e1";

// SMBus NCT6793 driver
const SMBUS_NCT6793_BIN: &[u8] = include_bytes!("../../../PawnIO/SmbusNCT6793.bin");
const SMBUS_NCT6793_HASH: &str = "385bd1b229faa44aefe2eb1f938ac629f6d897f1a4931dab5341b18b3bd432c1";

// SMBus PIIX4 driver
const SMBUS_PIIX4_BIN: &[u8] = include_bytes!("../../../PawnIO/SmbusPIIX4.bin");
const SMBUS_PIIX4_HASH: &str = "3f8b44c93eb030d59fb68c6fdc6857c61313eab2b6aa0ed64595396d71bf3ea3";

/// Helper function to calculate SHA256 hash of a file
pub fn calculate_file_hash<P: AsRef<Path>>(path: P) -> DriverResult<String> {
    let data = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_resource_creation() {
        let resource = DriverResource::new();
        assert!(resource.is_ok());
    }

    #[test]
    fn test_hash_calculation() {
        let test_data = b"hello world";
        let mut hasher = Sha256::new();
        hasher.update(test_data);
        let hash = format!("{:x}", hasher.finalize());

        // Known SHA256 of "hello world"
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }
}










