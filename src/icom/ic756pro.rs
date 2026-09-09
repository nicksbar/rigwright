//! Icom IC-756PRO CI-V profile.
//!
//! Command and address data is taken from the IC-756PRO instruction manual
//! extraction in the workspace `_manuals/IC-756PRO.md`.

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MemoryLayout, ModeCommand,
};
use crate::controls::ControlId;
use crate::hal_types::MeterId;
use crate::models::{find_model, RadioModelProfile};

const FREQUENCY_RANGES: &[(u64, u64)] = &[
    (1_800_000, 1_999_999),
    (3_500_000, 3_999_999),
    (7_000_000, 7_299_999),
    (10_100_000, 10_149_999),
    (14_000_000, 14_349_999),
    (18_068_000, 18_167_999),
    (21_000_000, 21_449_999),
    (24_890_000, 24_989_999),
    (28_000_000, 29_699_999),
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
];
const BAUD_RATES: &[u32] = &[300, 1_200, 4_800, 9_600, 19_200];
const METERS: &[MeterId] = &[MeterId::Signal];
const PROFILE_COMMON_CONTROLS: &[ControlSpec] = &[
    super::profile::COMMON_CONTROLS[0],
    super::profile::COMMON_CONTROLS[1],
    super::profile::COMMON_CONTROLS[2],
    super::profile::COMMON_CONTROLS[3],
    super::profile::COMMON_CONTROLS[4],
    super::profile::COMMON_CONTROLS[5],
    super::profile::COMMON_CONTROLS[6],
    super::profile::COMMON_CONTROLS[7],
    super::profile::COMMON_CONTROLS[8],
    super::profile::COMMON_CONTROLS[9],
    super::profile::COMMON_CONTROLS[10],
    super::profile::COMMON_CONTROLS[12],
];

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-756PRO").expect("built-in IC-756PRO profile")
}

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Ic756Pro,
    baud_rates: BAUD_RATES,
    usb_baud_rates: BAUD_RATES,
    supports_auto_baud: true,
    preferred_baud_rate: 19_200,
    default_address: 0x5C,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    common_controls: PROFILE_COMMON_CONTROLS,
    mode_command: ModeCommand::Legacy,
    tuning_step_values: &[0, 1, 2, 3, 4, 5, 6, 7, 8],
    modes: super::profile::DEFAULT_MODES,
    scope_geometry: None,
    scope: None,
    scope_options: super::profile::EMPTY_SCOPE_OPTIONS,
    main_sub: None,
    external_preamp: None,
    attenuator_values: &[0, 6, 12, 18],
    preamp_max_level: 2,
    agc_max: 3,
    noise_reduction_level_max: 255,
    supports_iq_output: false,
    meters: METERS,
    meter_poll_specs: &[],
    control_capabilities: ControlCapabilities {
        supports_data_mode: false,
        filter_values: &[],
        supports_vfo: true,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::Hf,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    filter_bandwidths: &[],
    swr_sweep_setup: None,
    meter_presentation: None,
    scope_ack_optional: false,
    usb_detection: &[],
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_matches_manual_identity_and_legacy_commands() {
        assert_eq!(profile().model, "IC-756PRO");
        assert_eq!(CIV_PROFILE.default_address, 0x5C);
        assert_eq!(CIV_PROFILE.mode_command, ModeCommand::Legacy);
        assert!(!CIV_PROFILE.supports_control(ControlId::DataMode));
        assert!(!CIV_PROFILE.supports_control(ControlId::Tuner));
        assert!(CIV_PROFILE.supports_control(ControlId::TuningStep));
    }
}
