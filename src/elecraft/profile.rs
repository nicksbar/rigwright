//! Declarative profiles for the first Elecraft transceiver family slice.

use anyhow::{bail, Result};

use crate::hal::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElecraftModel {
    K2,
    Kx2,
    Kx3,
    K3,
    K3s,
    K4,
}

impl ElecraftModel {
    pub const fn model_name(self) -> &'static str {
        match self {
            Self::K2 => "K2",
            Self::Kx2 => "KX2",
            Self::Kx3 => "KX3",
            Self::K3 => "K3",
            Self::K3s => "K3S",
            Self::K4 => "K4",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "K2" => Some(Self::K2),
            "KX2" => Some(Self::Kx2),
            "KX3" => Some(Self::Kx3),
            "K3" => Some(Self::K3),
            "K3S" => Some(Self::K3s),
            "K4" => Some(Self::K4),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElecraftModeSpec {
    pub code: char,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElecraftProfile {
    pub model: ElecraftModel,
    pub baud_rates: &'static [u32],
    pub frequency_ranges: &'static [(u64, u64)],
    pub modes: &'static [ElecraftModeSpec],
    pub supports_vfo_b: bool,
}

impl ElecraftProfile {
    pub fn supports_frequency(self, frequency_hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&frequency_hz))
    }

    pub fn encode_mode(self, mode: Mode) -> Result<char> {
        self.modes
            .iter()
            .find(|spec| spec.mode == mode)
            .map(|spec| spec.code)
            .ok_or_else(|| anyhow::anyhow!("{} does not support {mode:?}", self.model.model_name()))
    }

    pub fn decode_mode(self, code: char) -> Result<Mode> {
        self.modes
            .iter()
            .find(|spec| spec.code == code.to_ascii_uppercase())
            .map(|spec| spec.mode)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} returned unsupported mode {code}",
                    self.model.model_name()
                )
            })
    }

    pub fn validate_baud(self, baud_rate: u32) -> Result<()> {
        if !self.baud_rates.contains(&baud_rate) {
            bail!(
                "unsupported Elecraft baud rate {baud_rate} for {}; documented rates: {:?}",
                self.model.model_name(),
                self.baud_rates
            );
        }
        Ok(())
    }
}

const HF_RANGES: &[(u64, u64)] = &[(100_000, 54_000_000)];
const K4_RANGES: &[(u64, u64)] = &[(100_000, 54_000_000)];
const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400];
const K4_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];
const K3_MODES: &[ElecraftModeSpec] = &[
    ElecraftModeSpec {
        code: '1',
        mode: Mode::Lsb,
    },
    ElecraftModeSpec {
        code: '2',
        mode: Mode::Usb,
    },
    ElecraftModeSpec {
        code: '3',
        mode: Mode::Cw,
    },
    ElecraftModeSpec {
        code: '4',
        mode: Mode::Fm,
    },
    ElecraftModeSpec {
        code: '5',
        mode: Mode::Am,
    },
    ElecraftModeSpec {
        code: '6',
        mode: Mode::Data,
    },
    ElecraftModeSpec {
        code: '7',
        mode: Mode::CwReverse,
    },
    ElecraftModeSpec {
        code: '9',
        mode: Mode::RttyReverse,
    },
];
const K2_MODES: &[ElecraftModeSpec] = &[
    ElecraftModeSpec {
        code: '1',
        mode: Mode::Lsb,
    },
    ElecraftModeSpec {
        code: '2',
        mode: Mode::Usb,
    },
    ElecraftModeSpec {
        code: '3',
        mode: Mode::Cw,
    },
    ElecraftModeSpec {
        code: '6',
        mode: Mode::Rtty,
    },
    ElecraftModeSpec {
        code: '7',
        mode: Mode::CwReverse,
    },
    ElecraftModeSpec {
        code: '9',
        mode: Mode::RttyReverse,
    },
];

pub const K2_PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K2,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: K2_MODES,
    supports_vfo_b: true,
};
pub const KX2_PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::Kx2,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: K3_MODES,
    supports_vfo_b: true,
};
pub const KX3_PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::Kx3,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: K3_MODES,
    supports_vfo_b: true,
};
pub const K3_PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K3,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: K3_MODES,
    supports_vfo_b: true,
};
pub const K3S_PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K3s,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: K3_MODES,
    supports_vfo_b: true,
};
pub const K4_PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K4,
    baud_rates: K4_BAUD_RATES,
    frequency_ranges: K4_RANGES,
    modes: K3_MODES,
    supports_vfo_b: true,
};

pub const fn profile_for_model(model: ElecraftModel) -> ElecraftProfile {
    match model {
        ElecraftModel::K2 => K2_PROFILE,
        ElecraftModel::Kx2 => KX2_PROFILE,
        ElecraftModel::Kx3 => KX3_PROFILE,
        ElecraftModel::K3 => K3_PROFILE,
        ElecraftModel::K3s => K3S_PROFILE,
        ElecraftModel::K4 => K4_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_keep_k2_and_k3_mode_tables_distinct() {
        assert!(K2_PROFILE.encode_mode(Mode::Rtty).is_ok());
        assert!(K2_PROFILE.encode_mode(Mode::Am).is_err());
        assert_eq!(K3_PROFILE.encode_mode(Mode::Data).unwrap(), '6');
    }

    #[test]
    fn model_aliases_and_baud_contract_are_explicit() {
        assert_eq!(
            ElecraftModel::from_model_name("k3s"),
            Some(ElecraftModel::K3s)
        );
        assert!(K4_PROFILE.validate_baud(115_200).is_ok());
        assert!(K3_PROFILE.validate_baud(115_200).is_err());
    }
}
