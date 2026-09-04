//! Icom IC-705 model profile (framework only; hardware validation pending).

use super::profile::{
    ControlCapabilities, ControlEncoding, ControlSpec, IcomCivProfile, MemoryLayout, ScopeSpec,
};
use crate::controls::ControlId;
use crate::hal_types::{
    MeterId, ScopeCenterType, ScopeMarkerPosition, ScopeMaxHold, ScopeWaveformType,
};
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
    menu: Some(super::profile::ScopeMenuSpec {
        tx_display: 0x0173,
        max_hold: 0x0174,
        center_type: 0x0175,
        marker_position: 0x0176,
        vbw: 0x0177,
        averaging: 0x0178,
        waveform_type: 0x0179,
        waterfall_display: 0x0183,
        waterfall_speed: 0x0184,
        waterfall_size: 0x0185,
        waterfall_peak_level: 0x0186,
        marker_auto_hide: 0x0187,
        waveform_color_current: 0x0180,
        waveform_color_line: 0x0181,
        waveform_color_max_hold: 0x0182,
    }),
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
    waterfall_peak_levels: &[1, 2, 3, 4, 5, 6, 7, 8],
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
        crate::hal_types::ScopeEdgeBank {
            low_hz: 60_000_000,
            high_hz: 74_800_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 74_800_000,
            high_hz: 108_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 108_000_000,
            high_hz: 137_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 137_000_000,
            high_hz: 200_000_000,
            edge_numbers: &[1, 2, 3],
        },
        crate::hal_types::ScopeEdgeBank {
            low_hz: 400_000_000,
            high_hz: 470_000_000,
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
    model: crate::models::IcomCivModel::Ic705,
    baud_rates: super::profile::DEFAULT_BAUD_RATES,
    usb_baud_rates: super::profile::DEFAULT_BAUD_RATES,
    supports_auto_baud: true,
    preferred_baud_rate: 115_200,
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
    scope_options: SCOPE_OPTIONS,
    main_sub: None,
    external_preamp: None,
    attenuator_values: ATTENUATOR_VALUES,
    preamp_max_level: 2,
    agc_max: 3,
    noise_reduction_level_max: 15,
    supports_iq_output: false,
    meters: METERS,
    meter_poll_specs: super::profile::DEFAULT_METER_POLL_SPECS,
    control_capabilities: ControlCapabilities {
        supports_data_mode: true,
        filter_values: &[1, 2, 3],
        supports_vfo: true,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::VhfUhf,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    filter_bandwidths: &[],
    swr_sweep_setup: Some(super::profile::SWR_SWEEP_SETUP),
    meter_presentation: Some(super::profile::swr_meter_presentation),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ic705_catalog_profile() {
        assert_eq!(profile().model, "IC-705");
        assert!(!DOCUMENTED_CONTROLS.is_empty());
        assert!(!DOCUMENTED_FEATURES.is_empty());
    }
}
