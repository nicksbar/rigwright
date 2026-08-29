//! Icom IC-705 model profile (framework only; hardware validation pending).

use super::profile::{ControlEncoding, ControlSpec, IcomCivProfile, ScopeSpec};
use crate::controls::ControlId;
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
    // `0x21 0x01`: RIT enable/disable.
    ControlSpec {
        id: ControlId::Rit,
        command_prefix: &[0x21, 0x01],
        encoding: ControlEncoding::Bool,
    },
    // `0x14 0x0A`: RF transmit power, packed decimal level 0..255.
    // `0x14 0x01`: AF/audio gain, packed decimal level 0..255.
    // `0x14 0x02`: RF gain, packed decimal level 0..255.
    // `0x14 0x03`: squelch threshold, packed decimal level 0..255.
    // `0x11`: attenuator selection in dB (the value is not a subcommand).
    // `0x16 0x02`: internal preamplifier selection.
    // `0x16 0x12`: automatic gain control preset selection.
    // `0x16 0x22`: noise blanker enable/disable.
    // `0x16 0x40`: noise reduction enable/disable.
    // `0x0F`: split operation enable/disable (the value is not a subcommand).
    ControlSpec {
        id: ControlId::RfPower,
        command_prefix: &[0x14, 0x0A],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::AfGain,
        command_prefix: &[0x14, 0x01],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::RfGain,
        command_prefix: &[0x14, 0x02],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::Squelch,
        command_prefix: &[0x14, 0x03],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::Attenuator,
        command_prefix: &[0x11],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::Preamp,
        command_prefix: &[0x16, 0x02],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::Agc,
        command_prefix: &[0x16, 0x12],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::NoiseBlanker,
        command_prefix: &[0x16, 0x22],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::NoiseReduction,
        command_prefix: &[0x16, 0x40],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::IpPlus,
        command_prefix: &[0x1A, 0x07],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::Notch,
        command_prefix: &[0x16, 0x41],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::ManualNotch,
        command_prefix: &[0x16, 0x48],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::Tuner,
        command_prefix: &[0x1C, 0x01],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::Split,
        command_prefix: &[0x0F],
        encoding: ControlEncoding::Bool,
    },
];
const ATTENUATOR_VALUES: &[u8] = &[0, 20];
const SCOPE: ScopeSpec = ScopeSpec {
    enable_command: &[0x27, 0x10, 0x01],
    stream_command: &[0x27, 0x11, 0x01],
    disable_stream_command: &[0x27, 0x11, 0x00],
};

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
};
