//! Icom IC-7610 model profile (framework only; hardware validation pending).

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MainSubSpec, MemoryLayout,
    ScopeSpec,
};
use crate::controls::ControlId;
use crate::hal_types::MeterId;
use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-7610").expect("built-in IC-7610 profile")
}

pub const DOCUMENTED_CONTROLS: &[&str] = &[
    "RF power",
    "preamp",
    "AGC",
    "noise blanker",
    "noise reduction",
    "attenuator",
    "split",
];

pub const DOCUMENTED_FEATURES: &[&str] = &[
    "VFO A/B",
    "main/sub receiver",
    "I/Q output",
    "scope waveform",
];

const FREQUENCY_RANGES: &[(u64, u64)] = &[(30_000, 60_000_000)];
const CONTROLS: &[ControlSpec] = &[
    // `0x21 0x01`: RIT enable/disable; `0x21 0x02`: XIT enable/disable.
    ControlSpec {
        id: ControlId::Xit,
        command_prefix: &[0x21, 0x02],
        encoding: ControlEncoding::Bool,
    },
    // `0x14 0x0A`: RF transmit power, packed decimal level 0..255.
    // `0x14 0x01`: AF/audio gain, packed decimal level 0..255.
    // `0x14 0x02`: RF gain, packed decimal level 0..255.
    // `0x14 0x03`: squelch threshold, packed decimal level 0..255.
    // `0x11`: attenuator selection in dB. The manual's `29` marker means the
    // command supports direct main/sub targeting; it is not a subcommand byte.
    // `0x16 0x02`: internal preamplifier selection.
    // `0x16 0x22`: noise blanker enable/disable.
    // `0x16 0x40`: noise reduction enable/disable.
    // `0x0F`: split operation enable/disable (the value is not a subcommand).
    ControlSpec {
        id: ControlId::NoiseReductionLevel,
        command_prefix: &[0x14, 0x06],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::ManualNotchPosition,
        command_prefix: &[0x14, 0x0D],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::Antenna,
        command_prefix: &[0x12],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::IpPlus,
        command_prefix: &[0x1A, 0x07],
        encoding: ControlEncoding::Bool,
    },
];
const ATTENUATOR_VALUES: &[u8] = &[0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45];
const SCOPE: ScopeSpec = ScopeSpec {
    enable_command: &[0x27, 0x10, 0x01],
    stream_command: &[0x27, 0x11, 0x01],
    disable_stream_command: &[0x27, 0x11, 0x00],
};
const MAIN_SUB: MainSubSpec = MainSubSpec {
    set_command: 0x07,
    set_subcommand_base: 0xD0,
    get_command: 0x07,
    get_subcommand: 0xD2,
};
const METERS: &[MeterId] = &[
    MeterId::Signal,
    MeterId::Power,
    MeterId::Swr,
    MeterId::Alc,
    MeterId::Compression,
    MeterId::Voltage,
    MeterId::Current,
    MeterId::Temperature,
];

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Ic7610,
    baud_rates: super::profile::DEFAULT_BAUD_RATES,
    preferred_baud_rate: 115_200,
    default_address: 0x98,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    scope_geometry: Some(crate::models::IcomScopeGeometry {
        divisions: 15,
        bins: 689,
        full_chunk_bins: 50,
        last_chunk_bins: 39,
        bin_max: 200,
        supports_main_sub_scope: true,
    }),
    scope: Some(SCOPE),
    main_sub: Some(MAIN_SUB),
    external_preamp: None,
    attenuator_values: ATTENUATOR_VALUES,
    preamp_max_level: 2,
    agc_max: 0,
    noise_reduction_level_max: 15,
    supports_iq_output: true,
    meters: METERS,
    control_capabilities: ControlCapabilities {
        supports_data_mode: true,
        filter_values: &[1, 2, 3],
        supports_vfo: true,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::Hf,
    supports_repeater_settings: true,
    supports_memory_channels: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ic7610_catalog_profile() {
        assert_eq!(profile().model, "IC-7610");
        assert!(!DOCUMENTED_CONTROLS.is_empty());
        assert!(DOCUMENTED_FEATURES.contains(&"I/Q output"));
    }
}
