//! K2 profile from the KIO2 Programmer's Reference.
use super::profile::{ElecraftModeSpec, ElecraftModel, ElecraftProfile, BAUD_RATES, HF_RANGES};
use crate::hal::Mode;
const MODES: &[ElecraftModeSpec] = &[
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
        code: '6',
        mode: Mode::Rtty,
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
    model: ElecraftModel::K2,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: MODES,
    supports_vfo_b: true,
    supports_split: true,
    supports_rit_xit: true,
    power_max_watts: Some(15),
    af_gain_max: None,
    rf_gain_max: None,
    squelch_max: 250,
    rf_gain_is_attenuation: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k2_contract_is_distinct() {
        assert_eq!(PROFILE.model.model_name(), "K2");
        assert_eq!(PROFILE.squelch_max, 250);
        assert!(PROFILE.encode_mode(Mode::Rtty).is_ok());
        assert!(PROFILE.encode_mode(Mode::Am).is_err());
    }
}
