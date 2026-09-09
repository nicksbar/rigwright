//! Icom IC-756PROIII CI-V profile.
//!
//! Command and address data is taken from the IC-756PROIII instruction manual
//! extraction in the workspace `_manuals/IC-756PROIII.md`.

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
const METERS: &[MeterId] = &[
    MeterId::Signal,
    MeterId::Power,
    MeterId::Swr,
    MeterId::Alc,
    MeterId::Compression,
];

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-756PROIII").expect("built-in IC-756PROIII profile")
}

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Ic756ProIii,
    baud_rates: BAUD_RATES,
    usb_baud_rates: BAUD_RATES,
    supports_auto_baud: true,
    preferred_baud_rate: 19_200,
    default_address: 0x6E,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    common_controls: super::profile::COMMON_CONTROLS,
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
    meter_poll_specs: super::profile::DEFAULT_METER_POLL_SPECS,
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
    swr_sweep_setup: Some(super::profile::SWR_SWEEP_SETUP),
    meter_presentation: Some(super::profile::swr_meter_presentation),
    scope_ack_optional: false,
    usb_detection: &[],
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_matches_manual_identity_and_legacy_commands() {
        assert_eq!(profile().model, "IC-756PROIII");
        assert_eq!(CIV_PROFILE.default_address, 0x6E);
        assert_eq!(CIV_PROFILE.mode_command, ModeCommand::Legacy);
        assert!(CIV_PROFILE.supports_meter(MeterId::Swr));
        assert!(!CIV_PROFILE.supports_control(ControlId::DataMode));
    }
}
