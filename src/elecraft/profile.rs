//! Shared Elecraft profile contracts and model lookup.

use crate::{hal::Mode, hal_types::ControlId};
use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElecraftModel {
    K2,
    Kx2,
    Kx3,
    K3,
    K3s,
    K4,
    Kh1,
}

impl ElecraftModel {
    pub const fn model_name(self) -> &'static str {
        match self {
            Self::K2 => "K2",
            Self::Kx2 => "KX2",
            Self::Kx3 => "KX3",
            Self::K3 => "K3",
            Self::K3s => "K3S",
            Self::K4 => "K4",
            Self::Kh1 => "KH1",
        }
    }
    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "K2" => Some(Self::K2),
            "KX2" => Some(Self::Kx2),
            "KX3" => Some(Self::Kx3),
            "K3" => Some(Self::K3),
            "K3S" => Some(Self::K3s),
            "K4" => Some(Self::K4),
            "KH1" => Some(Self::Kh1),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElecraftModeSpec {
    pub code: char,
    pub mode: Mode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElecraftProfile {
    pub model: ElecraftModel,
    pub can_get_frequency: bool,
    pub can_set_frequency: bool,
    pub can_get_mode: bool,
    pub can_set_mode: bool,
    pub can_get_ptt: bool,
    pub can_set_ptt: bool,
    pub frequency_scale_hz: u64,
    pub frequency_width: usize,
    pub baud_rates: &'static [u32],
    pub frequency_ranges: &'static [(u64, u64)],
    pub modes: &'static [ElecraftModeSpec],
    pub supports_vfo_b: bool,
    pub supports_split: bool,
    pub supports_rit_xit: bool,
    pub power_max_watts: Option<u16>,
    pub antenna_max: Option<u8>,
    pub preamp_max: Option<u8>,
    pub attenuator_max: Option<u8>,
    pub supports_notch: bool,
    pub supports_manual_notch: bool,
    pub supports_noise_blanker: bool,
    pub noise_blanker_level_max: Option<u8>,
    pub noise_reduction_level_max: Option<u8>,
    pub supports_agc: bool,
    /// Highest documented AGC selector, when AGC is available.
    pub agc_max: Option<u8>,
    pub supports_tuner: bool,
    pub supports_repeater: bool,
    pub supports_tuning_step: bool,
    pub filter_max_hz: Option<u16>,
    pub filter_command: &'static str,
    pub af_gain_max: Option<u16>,
    pub rf_gain_max: Option<u16>,
    pub squelch_max: u16,
    pub rf_gain_is_attenuation: bool,
}

impl ElecraftProfile {
    pub fn supports_frequency(self, frequency_hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&frequency_hz))
    }
    pub fn encode_mode(self, mode: Mode) -> Result<char> {
        self.modes
            .iter()
            .find(|spec| spec.mode == mode)
            .map(|spec| spec.code)
            .ok_or_else(|| anyhow::anyhow!("{} does not support {mode:?}", self.model.model_name()))
    }
    pub fn decode_mode(self, code: char) -> Result<Mode> {
        self.modes
            .iter()
            .find(|spec| spec.code == code.to_ascii_uppercase())
            .map(|spec| spec.mode)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} returned unsupported mode {code}",
                    self.model.model_name()
                )
            })
    }
    pub fn validate_baud(self, baud_rate: u32) -> Result<()> {
        if !self.baud_rates.contains(&baud_rate) {
            bail!(
                "unsupported Elecraft baud rate {baud_rate} for {}; documented rates: {:?}",
                self.model.model_name(),
                self.baud_rates
            );
        }
        Ok(())
    }

    pub const fn supports_control(self, id: ControlId) -> bool {
        match id {
            ControlId::AfGain => self.af_gain_max.is_some(),
            ControlId::RfGain => self.rf_gain_max.is_some(),
            ControlId::Preamp => self.preamp_max.is_some(),
            ControlId::Attenuator => self.attenuator_max.is_some(),
            ControlId::Notch => self.supports_notch,
            ControlId::ManualNotch | ControlId::ManualNotchPosition => self.supports_manual_notch,
            ControlId::Antenna => self.antenna_max.is_some(),
            ControlId::NoiseBlanker => self.supports_noise_blanker,
            ControlId::NoiseReduction | ControlId::NoiseReductionLevel => {
                self.noise_reduction_level_max.is_some()
            }
            ControlId::Agc => self.supports_agc,
            ControlId::Filter => self.filter_max_hz.is_some(),
            ControlId::Tuner => self.supports_tuner,
            ControlId::TuningStep => self.supports_tuning_step,
            ControlId::Squelch
            | ControlId::RfPower
            | ControlId::Vfo
            | ControlId::Split
            | ControlId::Rit
            | ControlId::Xit => match id {
                ControlId::RfPower => self.power_max_watts.is_some(),
                ControlId::Vfo => self.supports_vfo_b,
                ControlId::Split => self.supports_split,
                ControlId::Rit | ControlId::Xit => self.supports_rit_xit,
                ControlId::Squelch => self.squelch_max > 0,
                _ => true,
            },
            _ => false,
        }
    }
}

pub(crate) const HF_RANGES: &[(u64, u64)] = &[(100_000, 54_000_000)];
pub(crate) const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400];
pub(crate) const K4_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];

pub const fn profile_for_model(model: ElecraftModel) -> ElecraftProfile {
    match model {
        ElecraftModel::K2 => super::k2::PROFILE,
        ElecraftModel::Kx2 => super::kx2::PROFILE,
        ElecraftModel::Kx3 => super::kx3::PROFILE,
        ElecraftModel::K3 => super::k3::PROFILE,
        ElecraftModel::K3s => super::k3s::PROFILE,
        ElecraftModel::K4 => super::k4::PROFILE,
        ElecraftModel::Kh1 => super::kh1::PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elecraft::{k2, k3, k4};
    #[test]
    fn model_lookup_and_profile_contract_are_explicit() {
        assert_eq!(
            ElecraftModel::from_model_name("k3s"),
            Some(ElecraftModel::K3s)
        );
        assert!(k4::PROFILE.validate_baud(115_200).is_ok());
        assert!(k3::PROFILE.validate_baud(115_200).is_err());
        assert!(k2::PROFILE.encode_mode(Mode::Rtty).is_ok());
        assert!(k2::PROFILE.encode_mode(Mode::Am).is_err());
    }

    #[test]
    fn every_profile_linear_control_range_reaches_both_hal_endpoints() {
        for model in [
            ElecraftModel::K2,
            ElecraftModel::Kx2,
            ElecraftModel::Kx3,
            ElecraftModel::K3,
            ElecraftModel::K3s,
            ElecraftModel::K4,
            ElecraftModel::Kh1,
        ] {
            let profile = profile_for_model(model);
            for maximum in [
                profile.af_gain_max,
                profile.rf_gain_max,
                Some(profile.squelch_max),
                profile.power_max_watts,
            ]
            .into_iter()
            .flatten()
            .filter(|maximum| *maximum > 0)
            {
                assert_eq!(crate::normalize_meter_level(maximum, maximum), Some(255));
                assert_eq!(crate::denormalize_meter_level(0, maximum), Some(0));
                assert_eq!(crate::denormalize_meter_level(255, maximum), Some(maximum));
            }
        }
    }
}
