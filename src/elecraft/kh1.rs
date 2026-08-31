//! KH1 limited profile from the KH1 Programmer's Reference.

use super::profile::{ElecraftModeSpec, ElecraftModel, ElecraftProfile};
use crate::hal::Mode;

const BAUD_RATES: &[u32] = &[9_600];
const FREQUENCY_RANGES: &[(u64, u64)] = &[(3_500_000, 29_700_000)];
const MODES: &[ElecraftModeSpec] = &[
    ElecraftModeSpec {
        code: '0',
        mode: Mode::Cw,
    },
    ElecraftModeSpec {
        code: '1',
        mode: Mode::Lsb,
    },
    ElecraftModeSpec {
        code: '2',
        mode: Mode::Usb,
    },
    ElecraftModeSpec {
        code: '4',
        mode: Mode::Rtty,
    },
];

pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::Kh1,
    can_get_frequency: false,
    can_set_frequency: true,
    can_get_mode: false,
    can_set_mode: true,
    can_get_ptt: false,
    can_set_ptt: false,
    frequency_scale_hz: 10,
    frequency_width: 7,
    baud_rates: BAUD_RATES,
    frequency_ranges: FREQUENCY_RANGES,
    modes: MODES,
    supports_vfo_b: false,
    supports_split: false,
    supports_rit_xit: false,
    power_max_watts: None,
    preamp_max: None,
    attenuator_max: None,
    supports_noise_blanker: false,
    noise_blanker_level_max: None,
    noise_reduction_level_max: None,
    supports_agc: false,
    supports_tuner: false,
    supports_repeater: false,
    supports_tuning_step: false,
    filter_max_hz: None,
    filter_command: "",
    af_gain_max: None,
    rf_gain_max: None,
    squelch_max: 0,
    rf_gain_is_attenuation: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_flag(value: bool, expected: bool) {
        assert_eq!(value, expected);
    }

    #[test]
    fn kh1_contract_is_fixed_baud_and_set_only() {
        assert_eq!(PROFILE.model.model_name(), "KH1");
        assert_eq!(PROFILE.baud_rates, &[9_600]);
        assert_flag(PROFILE.can_get_frequency, false);
        assert_flag(PROFILE.can_set_frequency, true);
        assert_flag(PROFILE.can_get_mode, false);
        assert_flag(PROFILE.can_set_mode, true);
    }
}
