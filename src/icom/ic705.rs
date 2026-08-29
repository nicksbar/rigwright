//! Icom IC-705 model profile (framework only; hardware validation pending).

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MemoryLayout, ScopeSpec,
};
use crate::controls::ControlId;
use crate::hal_types::MeterId;
use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-705").expect("built-in IC-705 profile")
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
    "memory/call channels",
    "VHF/UHF modes",
    "scope waveform",
];

const FREQUENCY_RANGES: &[(u64, u64)] = &[(30_000, 200_000_000), (400_000_000, 470_000_000)];
const CONTROLS: &[ControlSpec] = &[
    // `0x16 0x12`: automatic gain control preset selection.
    ControlSpec {
        id: ControlId::Agc,
        command_prefix: &[0x16, 0x12],
        encoding: ControlEncoding::U8,
    },
];
const ATTENUATOR_VALUES: &[u8] = &[0, 20];
const SCOPE: ScopeSpec = ScopeSpec {
    enable_command: &[0x27, 0x10, 0x01],
    stream_command: &[0x27, 0x11, 0x01],
    disable_stream_command: &[0x27, 0x11, 0x00],
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
    model: crate::models::IcomCivModel::Ic705,
    default_address: 0xA4,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    scope_geometry: Some(crate::models::IcomScopeGeometry {
        divisions: 11,
        bins: 475,
        full_chunk_bins: 50,
        last_chunk_bins: 25,
        bin_max: 160,
        supports_main_sub_scope: false,
    }),
    scope: Some(SCOPE),
    main_sub: None,
    external_preamp: None,
    attenuator_values: ATTENUATOR_VALUES,
    preamp_max_level: 2,
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
