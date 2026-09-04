//! Icom IC-718 model profile.
//!
//! Based on `IC-718 ADVANCED MANUAL 2024.pdf` in the local manual archive.
//! This is official-but-untested framework support; no physical IC-718 has
//! been validated by Rigwright.

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MemoryLayout,
};
use crate::controls::ControlId;
use crate::hal_types::MeterId;
use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-718").expect("built-in IC-718 profile")
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
    "VFO A/B",
    "memory channels",
];

pub const DOCUMENTED_FEATURES: &[&str] = &["HF/50 MHz operation", "CI-V remote control"];

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

const CONTROLS: &[ControlSpec] = &[
    ControlSpec {
        id: ControlId::NoiseReductionLevel,
        command_prefix: &[0x14, 0x06],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::Agc,
        command_prefix: &[0x16, 0x12],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::TuningStep,
        command_prefix: &[0x10],
        encoding: ControlEncoding::U8,
    },
];

const METERS: &[MeterId] = &[MeterId::Signal];
const BAUD_RATES: &[u32] = &[300, 1_200, 4_800, 9_600, 19_200];

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Ic718,
    baud_rates: BAUD_RATES,
    usb_baud_rates: BAUD_RATES,
    supports_auto_baud: true,
    preferred_baud_rate: 19_200,
    default_address: 0x5E,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    scope_geometry: None,
    scope: None,
    scope_options: super::profile::EMPTY_SCOPE_OPTIONS,
    main_sub: None,
    external_preamp: None,
    attenuator_values: &[0, 20],
    preamp_max_level: 1,
    agc_max: 2,
    noise_reduction_level_max: 15,
    supports_iq_output: false,
    meters: METERS,
    meter_poll_specs: super::profile::DEFAULT_METER_POLL_SPECS,
    control_capabilities: ControlCapabilities {
        supports_data_mode: true,
        filter_values: &[],
        supports_vfo: true,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::Hf,
    supports_repeater_settings: false,
    supports_memory_channels: true,
    filter_bandwidths: &[],
    swr_sweep_setup: None,
    meter_presentation: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_untested_ic718_catalog_profile() {
        assert_eq!(profile().model, "IC-718");
        assert_eq!(CIV_PROFILE.default_address, 0x5E);
        assert_eq!(CIV_PROFILE.preferred_baud_rate, 19_200);
        assert!(CIV_PROFILE.supports_control(ControlId::TuningStep));
        assert!(CIV_PROFILE.supports_control(ControlId::Split));
        assert!(CIV_PROFILE.supports_meter(MeterId::Signal));
        assert!(!CIV_PROFILE.supports_meter(MeterId::Swr));
    }
}
