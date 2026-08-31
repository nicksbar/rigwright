//! KX3 profile from the K3-family Programmer's Reference.
use super::profile::{ElecraftModeSpec, ElecraftModel, ElecraftProfile, BAUD_RATES, HF_RANGES};
use crate::hal::Mode;
pub(crate) const MODES: &[ElecraftModeSpec] = &[
    ElecraftModeSpec {
        code: '1',
        mode: Mode::Lsb,
    },
    ElecraftModeSpec {
        code: '2',
        mode: Mode::Usb,
    },
    ElecraftModeSpec {
        code: '3',
        mode: Mode::Cw,
    },
    ElecraftModeSpec {
        code: '4',
        mode: Mode::Fm,
    },
    ElecraftModeSpec {
        code: '5',
        mode: Mode::Am,
    },
    ElecraftModeSpec {
        code: '6',
        mode: Mode::Data,
    },
    ElecraftModeSpec {
        code: '7',
        mode: Mode::CwReverse,
    },
    ElecraftModeSpec {
        code: '9',
        mode: Mode::RttyReverse,
    },
];
pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::Kx3,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: MODES,
    supports_vfo_b: true,
    supports_split: true,
    supports_rit_xit: true,
    power_max_watts: Some(15),
    preamp_max: Some(1),
    attenuator_max: Some(1),
    supports_noise_blanker: true,
    supports_agc: true,
    af_gain_max: Some(255),
    rf_gain_max: Some(250),
    squelch_max: 29,
    rf_gain_is_attenuation: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kx3_contract_has_data_mode() {
        assert_eq!(PROFILE.model.model_name(), "KX3");
        assert_eq!(PROFILE.encode_mode(Mode::Data).unwrap(), '6');
        assert_eq!(PROFILE.squelch_max, 29);
    }
}
