//! Icom IC-7200 model profile.
//!
//! This profile is based on `IC-7200_ENG_CD_0b.pdf` in the local manual
//! archive. It is framework-level support: protocol fixtures are covered, but
//! no physical IC-7200 has been tested by Rigwright yet.

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MemoryLayout,
};
use crate::controls::ControlId;
use crate::hal_types::MeterId;
use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-7200").expect("built-in IC-7200 profile")
}

pub const DOCUMENTED_CONTROLS: &[&str] = &[
    "RF power",
    "preamp",
    "AGC",
    "noise blanker",
    "noise reduction",
    "attenuator",
    "split",
    "RIT",
    "tuning step",
    "filter",
    "tuner",
];

pub const DOCUMENTED_FEATURES: &[&str] = &["VFO A/B", "memory channels", "HF/50 MHz operation"];

const FREQUENCY_RANGES: &[(u64, u64)] = &[
    (1_800_000, 1_999_999),
    (3_400_000, 4_099_999),
    (6_900_000, 7_499_999),
    (9_900_000, 10_499_999),
    (13_900_000, 14_499_999),
    (17_900_000, 18_499_999),
    (20_900_000, 21_499_999),
    (24_400_000, 25_099_999),
    (28_000_000, 29_999_999),
    (50_000_000, 54_000_000),
];
const BAUD_RATES: &[u32] = &[300, 1_200, 4_800, 9_600, 19_200];

const CONTROLS: &[ControlSpec] = &[
    // 14 06: NR level, 0..255.
    ControlSpec {
        id: ControlId::NoiseReductionLevel,
        command_prefix: &[0x14, 0x06],
        encoding: ControlEncoding::Level255Bcd,
    },
    // 16 12: AGC off, fast, or slow.
    ControlSpec {
        id: ControlId::Agc,
        command_prefix: &[0x16, 0x12],
        encoding: ControlEncoding::U8,
    },
    // 10 00..05: model-native tuning-step selector.
    ControlSpec {
        id: ControlId::TuningStep,
        command_prefix: &[0x10],
        encoding: ControlEncoding::U8,
    },
];

const METERS: &[MeterId] = &[MeterId::Signal, MeterId::Power, MeterId::Swr, MeterId::Alc];

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Ic7200,
    baud_rates: BAUD_RATES,
    preferred_baud_rate: 19_200,
    default_address: 0x76,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    scope_geometry: None,
    scope: None,
    main_sub: None,
    external_preamp: None,
    attenuator_values: &[0, 20],
    preamp_max_level: 1,
    agc_max: 2,
    noise_reduction_level_max: 15,
    supports_iq_output: false,
    meters: METERS,
    control_capabilities: ControlCapabilities {
        supports_data_mode: true,
        filter_values: &[1, 2, 3],
        supports_vfo: true,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::Hf,
    supports_repeater_settings: false,
    supports_memory_channels: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_untested_ic7200_catalog_profile() {
        assert_eq!(profile().model, "IC-7200");
        assert_eq!(CIV_PROFILE.default_address, 0x76);
        assert!(CIV_PROFILE.scope.is_none());
        assert!(CIV_PROFILE.supports_control(ControlId::TuningStep));
        assert!(CIV_PROFILE.supports_control(ControlId::NoiseReductionLevel));
        assert_eq!(profile().control_max(ControlId::Agc), Some(2));
        assert!(!DOCUMENTED_FEATURES.is_empty());
    }
}
