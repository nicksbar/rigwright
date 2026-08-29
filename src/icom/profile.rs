//! Shared declarative profiles for Icom CI-V models.

use crate::controls::ControlId;

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
    /// Whether the model exposes documented I/Q output.
    pub supports_iq_output: bool,
}

impl IcomCivProfile {
    pub fn supports_frequency(self, hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&hz))
    }
    pub fn control(self, id: ControlId) -> Option<&'static ControlSpec> {
        self.controls.iter().find(|spec| spec.id == id)
    }
}

pub fn profile_for_model(model: crate::models::IcomCivModel) -> &'static IcomCivProfile {
    match model {
        crate::models::IcomCivModel::Ic705 => &crate::icom::ic705::CIV_PROFILE,
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
            IcomCivModel::Ic7300,
            IcomCivModel::Ic7610,
            IcomCivModel::Ic9700,
        ] {
            let profile = profile_for_model(model);
            assert_eq!(profile.model, model);
            assert!(profile.scope.is_some());
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
            assert_eq!(
                profile.control(ControlId::IpPlus).unwrap().command_prefix,
                &[0x1A, 0x07]
            );
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
