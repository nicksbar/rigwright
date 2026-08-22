//! Declarative profiles for classic five-byte Yaesu CAT radios.

use crate::{models::YaesuLegacyModel, protocol::yaesu_legacy_cat::LegacyMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YaesuLegacyProfile {
    pub model: YaesuLegacyModel,
    /// Conservative receive/tuning ranges from the model operating manual.
    pub frequency_ranges: &'static [(u64, u64)],
    /// CAT menu rates. Classic CAT always uses 8 data bits, no parity, and two
    /// stop bits.
    pub baud_rates: &'static [u32],
    /// Modes accepted by the documented `07` set command.
    pub writable_modes: &'static [LegacyMode],
    /// FT-817ND/FT-818 document radio power commands; these remain deliberately
    /// outside the protocol-neutral HAL because remote power-off is hazardous.
    pub documents_power_commands: bool,
}

impl YaesuLegacyProfile {
    pub fn supports_frequency(self, hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&hz))
    }

    pub fn supports_mode(self, mode: LegacyMode) -> bool {
        self.writable_modes.contains(&mode)
    }
}

const BAUD_RATES: &[u32] = &[4_800, 9_600, 38_400];
const BASE_MODES: &[LegacyMode] = &[
    LegacyMode::Lsb,
    LegacyMode::Usb,
    LegacyMode::Cw,
    LegacyMode::CwReverse,
    LegacyMode::Am,
    LegacyMode::Fm,
    LegacyMode::Digital,
    LegacyMode::Packet,
];
const MOBILE_MODES: &[LegacyMode] = &[
    LegacyMode::Lsb,
    LegacyMode::Usb,
    LegacyMode::Cw,
    LegacyMode::CwReverse,
    LegacyMode::Am,
    LegacyMode::Fm,
    LegacyMode::FmNarrow,
    LegacyMode::Digital,
    LegacyMode::Packet,
];

const FT817_RANGES: &[(u64, u64)] = &[
    (100_000, 30_000_000),
    (50_000_000, 54_000_000),
    (76_000_000, 154_000_000),
    (420_000_000, 470_000_000),
];
const FT818_RANGES: &[(u64, u64)] = &[
    (100_000, 56_000_000),
    (76_000_000, 154_000_000),
    (420_000_000, 470_000_000),
];
const MOBILE_RANGES: &[(u64, u64)] = &[
    (100_000, 56_000_000),
    (76_000_000, 108_000_000),
    (118_000_000, 164_000_000),
    (420_000_000, 470_000_000),
];

pub const FT817ND_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft817Nd,
    frequency_ranges: FT817_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: BASE_MODES,
    documents_power_commands: true,
};

pub const FT818_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft818,
    frequency_ranges: FT818_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: BASE_MODES,
    documents_power_commands: true,
};

pub const FT857D_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft857D,
    frequency_ranges: MOBILE_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: MOBILE_MODES,
    documents_power_commands: false,
};

pub const FT897D_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft897D,
    frequency_ranges: MOBILE_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: MOBILE_MODES,
    documents_power_commands: false,
};

pub fn profile_for_model(model: YaesuLegacyModel) -> &'static YaesuLegacyProfile {
    match model {
        YaesuLegacyModel::Ft817Nd => &FT817ND_PROFILE,
        YaesuLegacyModel::Ft818 => &FT818_PROFILE,
        YaesuLegacyModel::Ft857D => &FT857D_PROFILE,
        YaesuLegacyModel::Ft897D => &FT897D_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ranges_remain_individual() {
        assert!(!FT817ND_PROFILE.supports_frequency(40_000_000));
        assert!(FT818_PROFILE.supports_frequency(40_000_000));
        assert!(!FT857D_PROFILE.supports_frequency(110_000_000));
        assert!(FT857D_PROFILE.supports_frequency(145_000_000));
    }

    #[test]
    fn only_mobile_profiles_write_narrow_fm() {
        assert!(!FT817ND_PROFILE.supports_mode(LegacyMode::FmNarrow));
        assert!(FT857D_PROFILE.supports_mode(LegacyMode::FmNarrow));
    }
}
