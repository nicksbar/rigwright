//! Shared declarative profiles for Icom CI-V models.

use crate::controls::ControlId;
use crate::hal_types::{
    MeterId, MeterPollSpec, MeterPresentation, Mode, ScopeCenterType, ScopeMarkerPosition,
    ScopeMaxHold, ScopeMetadata, ScopeWaveformType, SwrSweepSetup,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEncoding {
    /// One byte: `0x00` is off and any non-zero value is on.
    Bool,
    /// One raw CI-V byte, interpreted by the model profile.
    U8,
    /// Two-byte decimal level used by Icom's `0x14` level commands.
    /// The first byte contains the hundreds digit; the second contains tens
    /// and ones as packed BCD. For example, 173 is `01 73`.
    Level255Bcd,
}

/// A model-specific control mapped to an Icom CI-V command prefix.
///
/// The resulting request is `[command_prefix..., value...]`; a get request
/// contains only the prefix. A prefix is deliberately variable length because
/// CI-V has both command-only operations (`0x0F`, `0x11`) and command families
/// with one or more subcommands (`0x14 0x01`, `0x1A ...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSpec {
    /// Protocol-neutral control exposed through the root `Radio` HAL.
    pub id: ControlId,
    /// CI-V bytes preceding the encoded control value.
    pub command_prefix: &'static [u8],
    /// Encoding used for the value following the command bytes.
    pub encoding: ControlEncoding,
}

/// CI-V controls shared by the profiled Icom families. Model modules contain
/// only exceptions and model-specific ranges.
pub const COMMON_CONTROLS: &[ControlSpec] = &[
    ControlSpec {
        id: ControlId::Rit,
        command_prefix: &[0x21, 0x01],
        encoding: ControlEncoding::Bool,
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
        id: ControlId::RfPower,
        command_prefix: &[0x14, 0x0A],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::Preamp,
        command_prefix: &[0x16, 0x02],
        encoding: ControlEncoding::U8,
    },
    ControlSpec {
        id: ControlId::Attenuator,
        command_prefix: &[0x11],
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

pub(crate) fn meter_command_prefix(id: MeterId) -> &'static [u8] {
    match id {
        MeterId::Signal => &[0x15, 0x02],
        MeterId::Power => &[0x15, 0x11],
        MeterId::Swr => &[0x15, 0x12],
        MeterId::Alc => &[0x15, 0x13],
        MeterId::Compression => &[0x15, 0x14],
        MeterId::Voltage => &[0x15, 0x15],
        MeterId::Current => &[0x15, 0x16],
        MeterId::Temperature => &[0x15, 0x17],
    }
}

/// CI-V command layout for selecting the active main/sub receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainSubSpec {
    /// Command byte used to select main or sub.
    pub set_command: u8,
    /// Subcommand for main; sub is this value plus one.
    pub set_subcommand_base: u8,
    /// Command byte used to query the active receiver.
    pub get_command: u8,
    /// Subcommand used to query the active receiver.
    pub get_subcommand: u8,
}

/// CI-V encoding for an external preamplifier control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalPreampSpec {
    /// CI-V bytes preceding the combined preamp value.
    pub command_prefix: &'static [u8],
    /// Bit selecting the external preamp in the combined value.
    pub enabled_mask: u8,
}

/// CI-V commands used by a model's scope stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeSpec {
    /// Request that scope output be enabled.
    pub enable_command: &'static [u8],
    /// Request that waveform frames be streamed.
    pub stream_command: &'static [u8],
    /// Stop waveform streaming.
    pub disable_stream_command: &'static [u8],
    /// Optional `1A 05` menu-command indices for advanced scope settings.
    pub menu: Option<ScopeMenuSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeMenuSpec {
    pub tx_display: u16,
    pub max_hold: u16,
    pub center_type: u16,
    pub marker_position: u16,
    pub vbw: u16,
    pub averaging: u16,
    pub waveform_type: u16,
    pub waterfall_display: u16,
    pub waterfall_speed: u16,
    pub waterfall_size: u16,
    pub waterfall_peak_level: u16,
    pub marker_auto_hide: u16,
    pub waveform_color_current: u16,
    pub waveform_color_line: u16,
    pub waveform_color_max_hold: u16,
}

/// Model-owned catalog of scope and waterfall settings. Empty lists mean the
/// setting has not been validated for that model yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeOptions {
    pub span_options_hz: &'static [u64],
    pub sweep_speed_values: &'static [u8],
    pub fixed_edge_numbers: &'static [u8],
    pub center_types: &'static [ScopeCenterType],
    pub tx_display: &'static [bool],
    pub max_hold: &'static [ScopeMaxHold],
    pub marker_positions: &'static [ScopeMarkerPosition],
    pub averaging: &'static [u8],
    pub waveform_types: &'static [ScopeWaveformType],
    pub waterfall_display: &'static [bool],
    pub waterfall_sizes: &'static [u8],
    pub waterfall_peak_levels: &'static [u8],
    pub marker_auto_hide: &'static [bool],
    pub edge_banks: &'static [crate::hal_types::ScopeEdgeBank],
    pub supports_waveform_colors: bool,
}

/// Model-owned capability metadata for controls whose CI-V layout is shared
/// but whose availability or readback is not universal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlCapabilities {
    pub supports_data_mode: bool,
    pub filter_values: &'static [u8],
    pub supports_vfo: bool,
    pub vfo_readable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLayout {
    Hf,
    VhfUhf,
}

/// Complete declarative CI-V behavior for one supported Icom model.
///
/// This is the boundary between the model-neutral CI-V engine and a model
/// driver. `civ_radio.rs` performs transport and framing; this profile owns
/// addresses, supported ranges, controls, scope geometry, and optional
/// model-specific behaviors.
#[derive(Debug, Clone, Copy)]
pub struct IcomCivProfile {
    /// Model represented by this profile.
    pub model: crate::models::IcomCivModel,
    /// CI-V rates documented by the model's manual.
    pub baud_rates: &'static [u32],
    /// CI-V USB rates documented by the model's manual, when distinct from
    /// the physical CI-V/REMOTE rates.
    pub usb_baud_rates: &'static [u32],
    /// Whether the radio exposes an Auto CI-V rate option.
    pub supports_auto_baud: bool,
    /// Preferred starting rate when Auto is not available.
    pub preferred_baud_rate: u32,
    /// Factory-default CI-V address. Applications may override it.
    pub default_address: u8,
    /// Conservative CAT-tunable frequency ranges from the model manual.
    pub frequency_ranges: &'static [(u64, u64)],
    /// Controls implemented by the generic profile executor.
    pub controls: &'static [ControlSpec],
    /// Waveform frame geometry, if the model's scope stream is supported.
    pub scope_geometry: Option<crate::models::IcomScopeGeometry>,
    /// Commands required to start and stop the model's scope stream.
    pub scope: Option<ScopeSpec>,
    /// Model-specific scope settings validated against its documentation.
    pub scope_options: ScopeOptions,
    /// Main/sub behavior, when documented for the model.
    pub main_sub: Option<MainSubSpec>,
    /// External preamp behavior, when documented for the model.
    pub external_preamp: Option<ExternalPreampSpec>,
    /// Allowed attenuator settings in dB.
    pub attenuator_values: &'static [u8],
    /// Highest valid preamp level for this model.
    pub preamp_max_level: u8,
    /// Highest valid AGC preset for this model.
    pub agc_max: u8,
    /// Highest valid noise-reduction level for this model.
    pub noise_reduction_level_max: u8,
    /// Whether the model exposes documented I/Q output. This is protocol/model
    /// metadata only; it does not claim that Rigwright has an openable stream.
    pub supports_iq_output: bool,
    /// Explicit meter surface exposed by this model profile.
    pub meters: &'static [MeterId],
    /// Driver scheduling guidance for polling normalized meters.
    pub meter_poll_specs: &'static [MeterPollSpec],
    /// Availability and readback metadata for shared special controls.
    pub control_capabilities: ControlCapabilities,
    /// Memory record layout used by this model.
    pub memory_layout: MemoryLayout,
    pub supports_repeater_settings: bool,
    pub supports_memory_channels: bool,
    pub filter_bandwidths: &'static [(Mode, u8, u32)],
    pub swr_sweep_setup: Option<SwrSweepSetup>,
    pub meter_presentation: Option<fn(MeterId, u8) -> Option<MeterPresentation>>,
}

impl PartialEq for IcomCivProfile {
    fn eq(&self, other: &Self) -> bool {
        let meter_presentation_eq = match (self.meter_presentation, other.meter_presentation) {
            (None, None) => true,
            (Some(left), Some(right)) => std::ptr::fn_addr_eq(left, right),
            _ => false,
        };

        self.model == other.model
            && self.baud_rates == other.baud_rates
            && self.usb_baud_rates == other.usb_baud_rates
            && self.supports_auto_baud == other.supports_auto_baud
            && self.preferred_baud_rate == other.preferred_baud_rate
            && self.default_address == other.default_address
            && self.frequency_ranges == other.frequency_ranges
            && self.controls == other.controls
            && self.scope_geometry == other.scope_geometry
            && self.scope == other.scope
            && self.scope_options == other.scope_options
            && self.main_sub == other.main_sub
            && self.external_preamp == other.external_preamp
            && self.attenuator_values == other.attenuator_values
            && self.preamp_max_level == other.preamp_max_level
            && self.agc_max == other.agc_max
            && self.noise_reduction_level_max == other.noise_reduction_level_max
            && self.supports_iq_output == other.supports_iq_output
            && self.meters == other.meters
            && self.meter_poll_specs == other.meter_poll_specs
            && self.control_capabilities == other.control_capabilities
            && self.memory_layout == other.memory_layout
            && self.supports_repeater_settings == other.supports_repeater_settings
            && self.supports_memory_channels == other.supports_memory_channels
            && self.filter_bandwidths == other.filter_bandwidths
            && self.swr_sweep_setup == other.swr_sweep_setup
            && meter_presentation_eq
    }
}

impl Eq for IcomCivProfile {}

/// Conservative CI-V defaults used by current Icom profiles unless a model
/// documents a narrower serial menu.
pub const DEFAULT_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];

/// Low-power carrier setup shared by the Icom models whose CI-V manuals
/// document both RTTY operation and SWR meter readback.
pub const SWR_SWEEP_SETUP: SwrSweepSetup = SwrSweepSetup {
    carrier_mode: Mode::Rtty,
    rf_power: 77,
};

pub const DEFAULT_METER_POLL_SPECS: &[MeterPollSpec] = &[
    MeterPollSpec {
        meter: MeterId::Signal,
        interval_ms: 400,
        tx_priority: false,
    },
    MeterPollSpec {
        meter: MeterId::Power,
        interval_ms: 300,
        tx_priority: true,
    },
    MeterPollSpec {
        meter: MeterId::Swr,
        interval_ms: 300,
        tx_priority: true,
    },
    MeterPollSpec {
        meter: MeterId::Alc,
        interval_ms: 300,
        tx_priority: true,
    },
    MeterPollSpec {
        meter: MeterId::Compression,
        interval_ms: 300,
        tx_priority: true,
    },
    MeterPollSpec {
        meter: MeterId::Current,
        interval_ms: 1_500,
        tx_priority: false,
    },
    MeterPollSpec {
        meter: MeterId::Voltage,
        interval_ms: 1_500,
        tx_priority: false,
    },
    MeterPollSpec {
        meter: MeterId::Temperature,
        interval_ms: 1_500,
        tx_priority: false,
    },
];

pub const EMPTY_SCOPE_OPTIONS: ScopeOptions = ScopeOptions {
    span_options_hz: &[],
    sweep_speed_values: &[],
    fixed_edge_numbers: &[],
    center_types: &[],
    tx_display: &[],
    max_hold: &[],
    marker_positions: &[],
    averaging: &[],
    waveform_types: &[],
    waterfall_display: &[],
    waterfall_sizes: &[],
    waterfall_peak_levels: &[],
    marker_auto_hide: &[],
    edge_banks: &[],
    supports_waveform_colors: false,
};

/// CI-V SWR calibration documented by the IC-705, IC-7300, IC-7610, and
/// IC-9700 command references.
pub(crate) fn swr_meter_presentation(id: MeterId, raw: u8) -> Option<MeterPresentation> {
    (id == MeterId::Swr).then(|| {
        let anchors = [(0_u8, 1.0_f32), (48, 1.5), (80, 2.0), (120, 3.0)];
        let value = anchors
            .windows(2)
            .find(|window| raw <= window[1].0)
            .map(|window| {
                let (low_level, low_ratio) = window[0];
                let (high_level, high_ratio) = window[1];
                low_ratio
                    + f32::from(raw.saturating_sub(low_level)) / f32::from(high_level - low_level)
                        * (high_ratio - low_ratio)
            })
            .unwrap_or(3.0);
        MeterPresentation {
            value,
            unit: ":1",
            precision: 2,
            upper_bound: Some(3.0),
        }
    })
}

impl IcomCivProfile {
    pub fn supports_frequency(self, hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&hz))
    }
    pub fn control(self, id: ControlId) -> Option<&'static ControlSpec> {
        self.controls
            .iter()
            .find(|spec| spec.id == id)
            .or_else(|| COMMON_CONTROLS.iter().find(|spec| spec.id == id))
    }

    pub fn supports_meter(self, id: MeterId) -> bool {
        self.meters.contains(&id)
    }

    pub fn supports_control(self, id: ControlId) -> bool {
        self.control(id).is_some()
            || (id == ControlId::DataMode && self.control_capabilities.supports_data_mode)
            || (id == ControlId::Filter && !self.control_capabilities.filter_values.is_empty())
            || id == ControlId::RawCiV
            || (id == ControlId::Vfo && self.control_capabilities.supports_vfo)
            || (id == ControlId::MainSub && self.main_sub.is_some())
            || (id == ControlId::ExternalPreamp && self.external_preamp.is_some())
    }

    pub fn filter_bandwidth_hz(self, mode: Mode, filter: u8) -> Option<u32> {
        self.filter_bandwidths
            .iter()
            .find(|&&(entry_mode, entry_filter, _)| entry_mode == mode && entry_filter == filter)
            .map(|&(_, _, bandwidth)| bandwidth)
    }

    pub fn meter_presentation(self, id: MeterId, raw: u8) -> Option<MeterPresentation> {
        self.meter_presentation.and_then(|present| present(id, raw))
    }

    pub fn control_max(self, id: ControlId) -> Option<u8> {
        match id {
            ControlId::Preamp => Some(self.preamp_max_level),
            ControlId::Agc => Some(self.agc_max),
            ControlId::NoiseReductionLevel => Some(self.noise_reduction_level_max),
            _ => None,
        }
    }

    pub fn supported_control_values(self, id: ControlId) -> Option<&'static [u8]> {
        match id {
            ControlId::Attenuator => Some(self.attenuator_values),
            ControlId::Filter => Some(self.control_capabilities.filter_values),
            _ => None,
        }
    }

    pub fn scope_metadata(self) -> Option<ScopeMetadata> {
        self.scope_geometry.map(|geometry| ScopeMetadata {
            waveform_bins: geometry.bins,
            waveform_divisions: geometry.divisions as u8,
            span_options_hz: self.scope_options.span_options_hz,
            sweep_speed_values: self.scope_options.sweep_speed_values,
            fixed_edge_numbers: self.scope_options.fixed_edge_numbers,
            reference_level_range_tenths_db: Some((-200, 200, 5)),
            supports_hold: true,
            supports_vbw: true,
            center_type_options: self.scope_options.center_types,
            tx_display_options: self.scope_options.tx_display,
            max_hold_options: self.scope_options.max_hold,
            marker_position_options: self.scope_options.marker_positions,
            averaging_options: self.scope_options.averaging,
            waveform_type_options: self.scope_options.waveform_types,
            waterfall_display_options: self.scope_options.waterfall_display,
            waterfall_size_options: self.scope_options.waterfall_sizes,
            waterfall_peak_level_options: self.scope_options.waterfall_peak_levels,
            marker_auto_hide_options: self.scope_options.marker_auto_hide,
            edge_banks: self.scope_options.edge_banks,
            supports_waveform_colors: self.scope_options.supports_waveform_colors,
        })
    }
}

pub fn profile_for_model(model: crate::models::IcomCivModel) -> &'static IcomCivProfile {
    match model {
        crate::models::IcomCivModel::Ic705 => &crate::icom::ic705::CIV_PROFILE,
        crate::models::IcomCivModel::Ic718 => &crate::icom::ic718::CIV_PROFILE,
        crate::models::IcomCivModel::Ic7200 => &crate::icom::ic7200::CIV_PROFILE,
        crate::models::IcomCivModel::Ic7300 => &crate::icom::ic7300::CIV_PROFILE,
        crate::models::IcomCivModel::Ic7610 => &crate::icom::ic7610::CIV_PROFILE,
        crate::models::IcomCivModel::Ic9700 => &crate::icom::ic9700::CIV_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::IcomCivModel;

    #[test]
    fn every_supported_model_has_profile_and_scope_commands() {
        for model in [
            IcomCivModel::Ic705,
            IcomCivModel::Ic718,
            IcomCivModel::Ic7200,
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            assert_eq!(profile.model, model);
            assert!(!profile.baud_rates.is_empty());
            assert!(
                profile.baud_rates.contains(&profile.preferred_baud_rate)
                    || profile
                        .usb_baud_rates
                        .contains(&profile.preferred_baud_rate)
            );
            assert!(profile.noise_reduction_level_max > 0);
            if profile.supports_control(ControlId::Agc) {
                assert!(profile.agc_max > 0);
            }
            assert!(!profile.controls.is_empty());
        }
    }

    #[test]
    fn model_behavior_metadata_matches_documented_differences() {
        assert!(profile_for_model(IcomCivModel::Ic705).main_sub.is_none());
        assert!(profile_for_model(IcomCivModel::Ic7300).main_sub.is_none());
        assert!(profile_for_model(IcomCivModel::Ic7610).main_sub.is_some());
        assert!(profile_for_model(IcomCivModel::Ic9700).main_sub.is_some());
        assert!(profile_for_model(IcomCivModel::Ic7610)
            .external_preamp
            .is_none());
        assert!(profile_for_model(IcomCivModel::Ic9700)
            .external_preamp
            .is_some());
    }

    #[test]
    fn swr_sweep_is_enabled_only_for_profiles_with_documented_ci_v_swr() {
        for model in [
            IcomCivModel::Ic705,
            IcomCivModel::Ic7200,
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            assert!(profile.swr_sweep_setup.is_some());
            assert!(profile.meter_presentation(MeterId::Swr, 48).is_some());
        }
        let ic718 = profile_for_model(IcomCivModel::Ic718);
        assert!(ic718.swr_sweep_setup.is_none());
        assert!(ic718.meter_presentation(MeterId::Swr, 48).is_none());
    }

    #[test]
    fn every_profile_declares_special_controls_meters_and_memory_layout() {
        for model in [
            IcomCivModel::Ic705,
            IcomCivModel::Ic7200,
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            assert!(profile.control_capabilities.supports_vfo);
            assert!(!profile.control_capabilities.filter_values.is_empty());
            assert!(profile.supports_meter(MeterId::Signal));
            assert!(profile.supports_meter(MeterId::Power));
            assert!(profile.supports_meter(MeterId::Swr));
            assert!(profile.supports_control(ControlId::DataMode));
            assert!(profile.supports_control(ControlId::Filter));
            assert!(profile.supports_control(ControlId::Vfo));
            if model != IcomCivModel::Ic7200 {
                assert!(profile.supports_repeater_settings);
            }
            assert!(profile.supports_memory_channels);
            for spec in profile.controls {
                assert!(
                    !COMMON_CONTROLS.iter().any(|common| common.id == spec.id),
                    "{model:?} redundantly overrides common control {:?}",
                    spec.id
                );
            }
        }
        assert_eq!(
            profile_for_model(IcomCivModel::Ic705).memory_layout,
            MemoryLayout::VhfUhf
        );
        assert_eq!(
            profile_for_model(IcomCivModel::Ic7300).memory_layout,
            MemoryLayout::Hf
        );
    }

    #[test]
    fn profile_commands_match_documented_ci_v_operations() {
        for model in [
            IcomCivModel::Ic705,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let scope = profile_for_model(model).scope.expect("scope profile");
            assert_eq!(scope.enable_command, &[0x27, 0x10, 0x01]);
            assert_eq!(scope.stream_command, &[0x27, 0x11, 0x01]);
            assert_eq!(scope.disable_stream_command, &[0x27, 0x11, 0x00]);
        }

        let ic7300 = profile_for_model(IcomCivModel::Ic7300)
            .scope
            .expect("IC-7300 scope profile");
        assert_eq!(ic7300.enable_command, &[0x27, 0x10, 0x01]);
        assert_eq!(ic7300.stream_command, &[0x27, 0x11, 0x01]);
        assert_eq!(ic7300.disable_stream_command, &[0x27, 0x11, 0x00]);

        let ic7610 = profile_for_model(IcomCivModel::Ic7610)
            .main_sub
            .expect("IC-7610 main/sub profile");
        assert_eq!(
            (ic7610.set_command, ic7610.set_subcommand_base),
            (0x07, 0xD0)
        );
        assert_eq!((ic7610.get_command, ic7610.get_subcommand), (0x07, 0xD2));

        let ic9700 = profile_for_model(IcomCivModel::Ic9700)
            .external_preamp
            .expect("IC-9700 external preamp profile");
        assert_eq!(
            (ic9700.command_prefix, ic9700.enabled_mask),
            (&[0x16, 0x02][..], 0x02)
        );

        for model in [
            IcomCivModel::Ic705,
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            assert_eq!(
                profile.control(ControlId::Split).unwrap().command_prefix,
                &[0x0F]
            );
            assert_eq!(
                profile
                    .control(ControlId::Attenuator)
                    .unwrap()
                    .command_prefix,
                &[0x11]
            );
            if model != IcomCivModel::Ic705 {
                assert_eq!(
                    profile.control(ControlId::IpPlus).unwrap().command_prefix,
                    &[0x1A, 0x07]
                );
            } else {
                assert!(profile.control(ControlId::IpPlus).is_none());
            }
            assert_eq!(
                profile.control(ControlId::Notch).unwrap().command_prefix,
                &[0x16, 0x41]
            );
            assert_eq!(
                profile
                    .control(ControlId::ManualNotch)
                    .unwrap()
                    .command_prefix,
                &[0x16, 0x48]
            );
        }
    }

    #[test]
    fn every_profile_inherits_common_control_and_meter_commands() {
        let common = [
            ControlId::Rit,
            ControlId::AfGain,
            ControlId::RfGain,
            ControlId::Squelch,
            ControlId::RfPower,
            ControlId::Preamp,
            ControlId::Attenuator,
            ControlId::NoiseBlanker,
            ControlId::NoiseReduction,
            ControlId::Notch,
            ControlId::ManualNotch,
            ControlId::Tuner,
            ControlId::Split,
        ];
        for model in [
            IcomCivModel::Ic705,
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            for id in common {
                assert!(profile.control(id).is_some(), "{model:?} missing {id:?}");
            }
        }
        assert_eq!(meter_command_prefix(MeterId::Signal), &[0x15, 0x02]);
        assert_eq!(meter_command_prefix(MeterId::Swr), &[0x15, 0x12]);
        assert_eq!(meter_command_prefix(MeterId::Temperature), &[0x15, 0x17]);
    }

    #[test]
    fn manual_driven_ranges_modes_and_scope_geometry_are_model_specific() {
        let ic705 = profile_for_model(IcomCivModel::Ic705);
        assert!(ic705.supports_frequency(145_000_000));
        assert!(ic705.supports_frequency(450_000_000));
        assert!(crate::icom::modes::supports_mode(
            IcomCivModel::Ic705,
            crate::hal::BaseMode::Wfm
        ));

        let ic7300 = profile_for_model(IcomCivModel::Ic7300);
        assert_eq!(ic7300.attenuator_values, &[0, 20]);
        assert!(crate::icom::modes::supports_mode(
            IcomCivModel::Ic7300,
            crate::hal::BaseMode::Fm
        ));

        let ic7610 = profile_for_model(IcomCivModel::Ic7610);
        assert!(!ic7610.supports_frequency(70_000_000));
        assert_eq!(ic7610.scope_geometry.unwrap().bins, 689);

        let ic9700 = profile_for_model(IcomCivModel::Ic9700);
        assert!(ic9700.supports_frequency(1_296_000_000));
        assert!(!crate::icom::modes::supports_mode(
            IcomCivModel::Ic9700,
            crate::hal::BaseMode::Wfm
        ));
    }

    #[test]
    fn control_profiles_reject_unsupported_controls() {
        assert!(profile_for_model(IcomCivModel::Ic705)
            .control(ControlId::ExternalPreamp)
            .is_none());
        assert!(profile_for_model(IcomCivModel::Ic705)
            .control(ControlId::MainSub)
            .is_none());
        assert!(profile_for_model(IcomCivModel::Ic9700)
            .control(ControlId::ExternalPreamp)
            .is_some());
        assert!(profile_for_model(IcomCivModel::Ic7300)
            .control(ControlId::Tuner)
            .is_some());
    }
}
