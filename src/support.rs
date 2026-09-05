//! Machine-readable support and validation evidence derived from the model catalog.

use serde::Serialize;

use crate::models::{ModelCapabilities, Protocol, SupportLevel, POPULAR_RADIOS};

const SOFTWARE_EVIDENCE: &[&str] = &["src/models.rs", "vendor profile and protocol tests"];
const IC7300_HARDWARE_EVIDENCE: &[&str] = &[
    "docs/radio-capability-matrix.md",
    "examples/ci_v_probe.rs",
    "QSONaut issues #41 and #46",
];
const FTDX10_HARDWARE_EVIDENCE: &[&str] =
    &["docs/radio-capability-matrix.md", "examples/yaesu_probe.rs"];
const NO_HARDWARE_EVIDENCE: &[&str] = &[];

/// The three evidence states deliberately remain independent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupportEvidence {
    /// The model is present in [`crate::models::POPULAR_RADIOS`].
    pub cataloged: bool,
    /// Deterministic profile, parser, or fake-transport tests cover the model.
    pub software_tested: bool,
    /// A physical radio has been exercised and the result was reviewed.
    pub hardware_tested: bool,
    /// Stable repository references supporting the claims above.
    pub references: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupportMatrixEntry {
    pub manufacturer: &'static str,
    pub model: &'static str,
    pub protocol: &'static str,
    pub default_address: Option<u8>,
    pub support_level: &'static str,
    pub capabilities: ModelCapabilitiesOutput,
    pub hal: HalCapabilitiesOutput,
    pub baud_rates: &'static [u32],
    pub preferred_baud_rate: u32,
    pub evidence: SupportEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ModelCapabilitiesOutput {
    pub hf: bool,
    pub vhf_uhf: bool,
    pub frequency: bool,
    pub mode: bool,
    pub ptt: bool,
    pub levels: bool,
    pub spectrum: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HalCapabilitiesOutput {
    pub can_get_frequency: bool,
    pub can_set_frequency: bool,
    pub can_get_mode: bool,
    pub can_set_mode: bool,
    pub can_get_ptt: bool,
    pub can_set_ptt: bool,
    pub can_get_power: bool,
    pub can_set_power: bool,
    pub can_raw_protocol: bool,
}

/// A generated support matrix. Every entry comes from the catalog and selected
/// vendor profile; no client-maintained capability list is involved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupportMatrix {
    pub schema_version: u32,
    pub source: &'static str,
    pub models: Vec<SupportMatrixEntry>,
}

impl SupportMatrix {
    pub fn from_catalog() -> Self {
        let models = POPULAR_RADIOS
            .iter()
            .map(|profile| {
                let hal = profile.driver_capabilities();
                let default_address = match profile.protocol {
                    Protocol::IcomCiV { default_address } => Some(default_address),
                    _ => None,
                };
                let hardware_tested = profile.support == SupportLevel::HardwareValidated;
                let references = match profile.model {
                    "IC-7300" => IC7300_HARDWARE_EVIDENCE,
                    "FTDX10" => FTDX10_HARDWARE_EVIDENCE,
                    _ => NO_HARDWARE_EVIDENCE,
                };
                SupportMatrixEntry {
                    manufacturer: profile.manufacturer.label(),
                    model: profile.model,
                    protocol: profile.protocol.label(),
                    default_address,
                    support_level: profile.support.detail_label(),
                    capabilities: profile.capabilities.into(),
                    hal: hal.into(),
                    baud_rates: profile.supported_baud_rates(),
                    preferred_baud_rate: profile.preferred_baud_rate(),
                    evidence: SupportEvidence {
                        cataloged: true,
                        // The catalog/profile coherence suite and each vendor's
                        // protocol/profile tests are the software evidence.
                        software_tested: true,
                        hardware_tested,
                        references: if hardware_tested {
                            references
                        } else {
                            SOFTWARE_EVIDENCE
                        },
                    },
                }
            })
            .collect();
        Self {
            schema_version: 1,
            source: "rigwright::models::POPULAR_RADIOS",
            models,
        }
    }

    pub fn to_json(&self, pretty: bool) -> serde_json::Result<String> {
        if pretty {
            serde_json::to_string_pretty(self)
        } else {
            serde_json::to_string(self)
        }
    }
}

impl From<ModelCapabilities> for ModelCapabilitiesOutput {
    fn from(value: ModelCapabilities) -> Self {
        Self {
            hf: value.hf,
            vhf_uhf: value.vhf_uhf,
            frequency: value.frequency,
            mode: value.mode,
            ptt: value.ptt,
            levels: value.levels,
            spectrum: value.spectrum,
        }
    }
}

impl From<crate::RadioCapabilities> for HalCapabilitiesOutput {
    fn from(value: crate::RadioCapabilities) -> Self {
        Self {
            can_get_frequency: value.can_get_frequency,
            can_set_frequency: value.can_set_frequency,
            can_get_mode: value.can_get_mode,
            can_set_mode: value.can_set_mode,
            can_get_ptt: value.can_get_ptt,
            can_set_ptt: value.can_set_ptt,
            can_get_power: value.can_get_power,
            can_set_power: value.can_set_power,
            can_raw_protocol: value.can_raw_protocol,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SupportMatrix;

    #[test]
    fn support_matrix_is_generated_from_the_catalog() {
        let matrix = SupportMatrix::from_catalog();
        let ic7300 = matrix
            .models
            .iter()
            .find(|entry| entry.model == "IC-7300")
            .expect("IC-7300 support entry");
        assert!(ic7300.evidence.cataloged);
        assert!(ic7300.evidence.software_tested);
        assert!(ic7300.evidence.hardware_tested);
        assert_eq!(ic7300.default_address, Some(0x94));
        assert!(ic7300.hal.can_set_power);
        assert!(!ic7300.hal.can_get_power);
    }

    #[test]
    fn support_matrix_json_is_machine_readable_and_separates_evidence() {
        let json = SupportMatrix::from_catalog()
            .to_json(false)
            .expect("support matrix JSON");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let entry = value["models"]
            .as_array()
            .expect("models array")
            .iter()
            .find(|entry| entry["model"] == "IC-7200")
            .expect("IC-7200 entry");
        assert_eq!(entry["evidence"]["cataloged"], true);
        assert_eq!(entry["evidence"]["software_tested"], true);
        assert_eq!(entry["evidence"]["hardware_tested"], false);
    }
}
