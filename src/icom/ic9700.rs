//! Icom IC-9700 model profile (framework only; hardware validation pending).

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, ExternalPreampSpec, IcomCivProfile,
    MainSubSpec, MemoryLayout, ScopeSpec,
};
use crate::controls::ControlId;
use crate::hal_types::MeterId;
use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-9700").expect("built-in IC-9700 profile")
}

pub const DOCUMENTED_CONTROLS: &[&str] = &[
    "RF power",
    "preamp/external preamp",
    "AGC",
    "noise blanker",
    "noise reduction",
    "attenuator",
    "split",
];

pub const DOCUMENTED_FEATURES: &[&str] = &[
    "VFO A/B",
    "main/sub band",
    "satellite mode",
    "DV/DD modes",
    "scope waveform",
];

const FREQUENCY_RANGES: &[(u64, u64)] = &[
    (144_000_000, 148_000_000),
    (430_000_000, 450_000_000),
    (1_240_000_000, 1_300_000_000),
];
const CONTROLS: &[ControlSpec] = &[
    // `0x21 0x01`: RIT enable/disable.
    // `0x14 0x0A`: RF transmit power, packed decimal level 0..255.
    // `0x14 0x01`: AF/audio gain, packed decimal level 0..255.
    // `0x14 0x02`: RF gain, packed decimal level 0..255.
    // `0x14 0x03`: squelch threshold, packed decimal level 0..255.
    // `0x11`: attenuator selection in dB (the value is not a subcommand).
    // `0x16 0x02`: internal or external preamplifier selection.
    // `0x16 0x02`: external preamplifier selection uses model-specific values.
    // `0x16 0x12`: automatic gain control preset selection.
    // `0x16 0x22`: noise blanker enable/disable.
    // `0x16 0x40`: noise reduction enable/disable.
    // `0x0F`: split operation enable/disable (the value is not a subcommand).
    ControlSpec {
        id: ControlId::ExternalPreamp,
        command_prefix: &[0x16, 0x02],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::Agc,
        command_prefix: &[0x16, 0x12],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::IpPlus,
        command_prefix: &[0x1A, 0x07],
        encoding: ControlEncoding::Bool,
    },
];
const ATTENUATOR_VALUES: &[u8] = &[0, 10];
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
const EXTERNAL_PREAMP: ExternalPreampSpec = ExternalPreampSpec {
    command_prefix: &[0x16, 0x02],
    enabled_mask: 0x02,
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
    model: crate::models::IcomCivModel::Ic9700,
    baud_rates: super::profile::DEFAULT_BAUD_RATES,
    preferred_baud_rate: 115_200,
    default_address: 0xA2,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    scope_geometry: Some(crate::models::IcomScopeGeometry {
        divisions: 11,
        bins: 475,
        full_chunk_bins: 50,
        last_chunk_bins: 25,
        bin_max: 160,
        supports_main_sub_scope: true,
    }),
    scope: Some(SCOPE),
    main_sub: Some(MAIN_SUB),
    external_preamp: Some(EXTERNAL_PREAMP),
    attenuator_values: ATTENUATOR_VALUES,
    preamp_max_level: 1,
    agc_max: 3,
    noise_reduction_level_max: 15,
    supports_iq_output: false,
    meters: METERS,
    control_capabilities: ControlCapabilities {
        supports_data_mode: true,
        filter_values: &[1, 2, 3],
        supports_vfo: true,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::VhfUhf,
    supports_repeater_settings: true,
    supports_memory_channels: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ic9700_catalog_profile() {
        assert_eq!(profile().model, "IC-9700");
        assert!(!DOCUMENTED_CONTROLS.is_empty());
        assert!(DOCUMENTED_FEATURES.contains(&"satellite mode"));
    }
}
