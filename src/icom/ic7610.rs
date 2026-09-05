//! Icom IC-7610 model profile (framework only; hardware validation pending).

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MainSubSpec, MemoryLayout,
    ScopeSpec,
};
use crate::controls::ControlId;
use crate::hal_types::{
    MeterId, ScopeCenterType, ScopeMarkerPosition, ScopeMaxHold, ScopeWaveformType,
};
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
    menu: Some(super::profile::ScopeMenuSpec {
        tx_display: 0x0166,
        max_hold: 0x0167,
        center_type: 0x0168,
        marker_position: 0x0169,
        vbw: 0x016A,
        averaging: 0x0170,
        waveform_type: 0x0171,
        waterfall_display: 0x0175,
        waterfall_speed: 0x0176,
        waterfall_size: 0x0177,
        waterfall_peak_level: 0x0178,
        marker_auto_hide: 0x0179,
        waveform_color_current: 0x0172,
        waveform_color_line: 0x0173,
        waveform_color_max_hold: 0x0174,
    }),
};
const MAIN_SUB: MainSubSpec = MainSubSpec {
    set_command: 0x07,
    set_subcommand_base: 0xD0,
    get_command: 0x07,
    get_subcommand: 0xD2,
};
const SCOPE_OPTIONS: super::profile::ScopeOptions = super::profile::ScopeOptions {
    span_options_hz: &[
        2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
    ],
    sweep_speed_values: &[0, 1, 2],
    fixed_edge_numbers: &[1, 2, 3],
    center_types: &[
        ScopeCenterType::FilterCenter,
        ScopeCenterType::CarrierPoint,
        ScopeCenterType::CarrierPointAbsolute,
    ],
    tx_display: &[false, true],
    max_hold: &[
        ScopeMaxHold::Off,
        ScopeMaxHold::TenSeconds,
        ScopeMaxHold::Continuous,
    ],
    marker_positions: &[
        ScopeMarkerPosition::FilterCenter,
        ScopeMarkerPosition::CarrierPoint,
    ],
    averaging: &[0, 2, 3, 4],
    waveform_types: &[ScopeWaveformType::Fill, ScopeWaveformType::FillAndLine],
    waterfall_display: &[false, true],
    waterfall_sizes: &[0, 1, 2],
    waterfall_peak_levels: &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
    marker_auto_hide: &[false, true],
    edge_banks: &[
        crate::hal_types::ScopeEdgeBank {
            low_hz: 30_000,
            high_hz: 1_600_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 1_600_000,
            high_hz: 2_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 2_000_000,
            high_hz: 6_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 6_000_000,
            high_hz: 8_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 8_000_000,
            high_hz: 11_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 11_000_000,
            high_hz: 15_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 15_000_000,
            high_hz: 20_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 20_000_000,
            high_hz: 22_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 22_000_000,
            high_hz: 26_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 26_000_000,
            high_hz: 30_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 30_000_000,
            high_hz: 45_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 45_000_000,
            high_hz: 60_000_000,
            edge_numbers: &[1, 2, 3],
        },
    ],
    supports_waveform_colors: true,
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
    usb_baud_rates: super::profile::DEFAULT_BAUD_RATES,
    supports_auto_baud: true,
    preferred_baud_rate: 115_200,
    default_address: 0x98,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    modes: super::profile::DEFAULT_MODES,
    scope_geometry: Some(crate::models::IcomScopeGeometry {
        divisions: 15,
        bins: 689,
        full_chunk_bins: 50,
        last_chunk_bins: 39,
        bin_max: 200,
        supports_main_sub_scope: true,
    }),
    scope: Some(SCOPE),
    scope_options: SCOPE_OPTIONS,
    main_sub: Some(MAIN_SUB),
    external_preamp: None,
    attenuator_values: ATTENUATOR_VALUES,
    preamp_max_level: 2,
    agc_max: 0,
    noise_reduction_level_max: 15,
    supports_iq_output: true,
    meters: METERS,
    meter_poll_specs: super::profile::DEFAULT_METER_POLL_SPECS,
    control_capabilities: ControlCapabilities {
        supports_data_mode: true,
        filter_values: &[1, 2, 3],
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
    fn exposes_the_ic7610_catalog_profile() {
        assert_eq!(profile().model, "IC-7610");
        assert!(!DOCUMENTED_CONTROLS.is_empty());
        assert!(DOCUMENTED_FEATURES.contains(&"I/Q output"));
    }
}
