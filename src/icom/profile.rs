//! Shared declarative profiles for Icom CI-V models.

use crate::controls::ControlId;
use crate::hal_types::MeterId;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcomCivProfile {
    /// Model represented by this profile.
    pub model: crate::models::IcomCivModel,
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
    /// Main/sub behavior, when documented for the model.
    pub main_sub: Option<MainSubSpec>,
    /// External preamp behavior, when documented for the model.
    pub external_preamp: Option<ExternalPreampSpec>,
    /// Allowed attenuator settings in dB.
    pub attenuator_values: &'static [u8],
    /// Highest valid preamp level for this model.
    pub preamp_max_level: u8,
    /// Whether the model exposes documented I/Q output. This is protocol/model
    /// metadata only; it does not claim that Rigwright has an openable stream.
    pub supports_iq_output: bool,
    /// Explicit meter surface exposed by this model profile.
    pub meters: &'static [MeterId],
    /// Availability and readback metadata for shared special controls.
    pub control_capabilities: ControlCapabilities,
    /// Memory record layout used by this model.
    pub memory_layout: MemoryLayout,
    pub supports_repeater_settings: bool,
    pub supports_memory_channels: bool,
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
}

pub fn profile_for_model(model: crate::models::IcomCivModel) -> &'static IcomCivProfile {
    match model {
        crate::models::IcomCivModel::Ic705 => &crate::icom::ic705::CIV_PROFILE,
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
            IcomCivModel::Ic7200,
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            assert_eq!(profile.model, model);
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
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let scope = profile_for_model(model).scope.expect("scope profile");
            assert_eq!(scope.enable_command, &[0x27, 0x10, 0x01]);
            assert_eq!(scope.stream_command, &[0x27, 0x11, 0x01]);
            assert_eq!(scope.disable_stream_command, &[0x27, 0x11, 0x00]);
        }

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
