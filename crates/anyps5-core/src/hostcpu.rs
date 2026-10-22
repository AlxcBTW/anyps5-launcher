//! The host CPU check (PORT.md 6.12). AnyPS5 runs titles natively and needs the extensions of the
//! console's Zen 2 CPU; the engine stops at start and says so. The launcher warns early.

use serde::{Deserialize, Serialize};

/// The extensions the engine requires, in the order they are reported.
pub const REQUIRED_FEATURES: [&str; 7] = ["AVX2", "FMA", "F16C", "BMI1", "BMI2", "LZCNT", "MOVBE"];

/// The `cpuConversion` preference (D1): whether the relinker converts AMD-only instructions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuConversion {
    /// `--to-intel` when the host CPU vendor is Intel.
    #[default]
    Auto,
    None,
    Intel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostCpu {
    /// The CPUID vendor string, e.g. `GenuineIntel`, `AuthenticAMD`; empty when unknown.
    pub vendor: String,
    /// The brand string, e.g. `AMD Ryzen 5 3600 6-Core Processor`; empty when unknown.
    pub brand: String,
    present: Vec<&'static str>,
}

impl HostCpu {
    /// Reads this machine's CPU.
    pub fn detect() -> HostCpu {
        let (vendor, brand) = vendor_and_brand();
        HostCpu {
            vendor,
            brand,
            present: detect_features(),
        }
    }

    /// A CPU described by hand (tests, or a report from another machine).
    pub fn from_parts(vendor: &str, brand: &str, present: &[&str]) -> HostCpu {
        HostCpu {
            vendor: vendor.into(),
            brand: brand.into(),
            present: REQUIRED_FEATURES
                .into_iter()
                .filter(|f| present.iter().any(|p| p.eq_ignore_ascii_case(f)))
                .collect(),
        }
    }

    /// The required extensions this CPU lacks; empty when it can run titles.
    pub fn missing_features(&self) -> Vec<&'static str> {
        REQUIRED_FEATURES
            .into_iter()
            .filter(|f| !self.present.contains(f))
            .collect()
    }

    pub fn is_intel(&self) -> bool {
        self.vendor == "GenuineIntel"
    }

    /// Whether the relinker should get `--to-intel` under this preference.
    pub fn wants_intel_conversion(&self, mode: CpuConversion) -> bool {
        match mode {
            CpuConversion::Auto => self.is_intel(),
            CpuConversion::None => false,
            CpuConversion::Intel => true,
        }
    }
}

#[cfg(target_arch = "x86_64")]
fn detect_features() -> Vec<&'static str> {
    let flags = [
        ("AVX2", is_x86_feature_detected!("avx2")),
        ("FMA", is_x86_feature_detected!("fma")),
        ("F16C", is_x86_feature_detected!("f16c")),
        ("BMI1", is_x86_feature_detected!("bmi1")),
        ("BMI2", is_x86_feature_detected!("bmi2")),
        ("LZCNT", is_x86_feature_detected!("lzcnt")),
        ("MOVBE", is_x86_feature_detected!("movbe")),
    ];
    flags
        .into_iter()
        .filter(|(_, present)| *present)
        .map(|(name, _)| name)
        .collect()
}

#[cfg(not(target_arch = "x86_64"))]
fn detect_features() -> Vec<&'static str> {
    Vec::new()
}

#[cfg(target_arch = "x86_64")]
#[allow(unused_unsafe)]
fn vendor_and_brand() -> (String, String) {
    use std::arch::x86_64::__cpuid;
    // SAFETY: CPUID exists on every x86_64 CPU.
    unsafe {
        let leaf0 = __cpuid(0);
        let mut vendor = Vec::new();
        for reg in [leaf0.ebx, leaf0.edx, leaf0.ecx] {
            vendor.extend(reg.to_le_bytes());
        }
        let mut brand = Vec::new();
        if __cpuid(0x8000_0000).eax >= 0x8000_0004 {
            for leaf in 0x8000_0002u32..=0x8000_0004 {
                let r = __cpuid(leaf);
                for reg in [r.eax, r.ebx, r.ecx, r.edx] {
                    brand.extend(reg.to_le_bytes());
                }
            }
        }
        let text = |bytes: Vec<u8>| {
            String::from_utf8_lossy(&bytes)
                .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                .to_string()
        };
        (text(vendor), text(brand))
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn vendor_and_brand() -> (String, String) {
    (String::new(), String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_features_are_listed_in_the_engines_order() {
        let cpu = HostCpu::from_parts("GenuineIntel", "Old CPU", &["fma", "BMI1", "lzcnt"]);
        assert_eq!(cpu.missing_features(), ["AVX2", "F16C", "BMI2", "MOVBE"]);
        let full = HostCpu::from_parts("AuthenticAMD", "Zen", &REQUIRED_FEATURES);
        assert!(full.missing_features().is_empty());
    }

    #[test]
    fn intel_conversion_follows_the_preference() {
        let intel = HostCpu::from_parts("GenuineIntel", "", &[]);
        let amd = HostCpu::from_parts("AuthenticAMD", "", &[]);
        assert!(intel.wants_intel_conversion(CpuConversion::Auto));
        assert!(!amd.wants_intel_conversion(CpuConversion::Auto));
        assert!(amd.wants_intel_conversion(CpuConversion::Intel));
        assert!(!intel.wants_intel_conversion(CpuConversion::None));
    }

    #[test]
    fn preference_json_names() {
        assert_eq!(
            serde_json::to_string(&CpuConversion::Auto).unwrap(),
            "\"auto\""
        );
        let parsed: CpuConversion = serde_json::from_str("\"intel\"").unwrap();
        assert_eq!(parsed, CpuConversion::Intel);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn this_machine_reports_a_vendor() {
        let cpu = HostCpu::detect();
        assert!(!cpu.vendor.is_empty());
        // The development and CI machines run AnyPS5-capable or at least modern CPUs; the call
        // only has to work and agree with itself.
        assert_eq!(cpu.missing_features(), HostCpu::detect().missing_features());
    }
}
