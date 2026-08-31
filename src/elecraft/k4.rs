//! K4 profile from the K4 Programmer's Reference.
use super::profile::{ElecraftModel, ElecraftProfile, HF_RANGES, K4_BAUD_RATES};
pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K4,
    can_get_frequency: true,
    can_set_frequency: true,
    can_get_mode: true,
    can_set_mode: true,
    can_get_ptt: true,
    can_set_ptt: true,
    frequency_scale_hz: 1,
    frequency_width: 11,
    baud_rates: K4_BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: super::kx3::MODES,
    supports_vfo_b: true,
    supports_split: true,
    supports_rit_xit: true,
    power_max_watts: Some(110),
    antenna_max: Some(3),
    preamp_max: Some(2),
    attenuator_max: Some(30),
    supports_notch: true,
    supports_manual_notch: true,
    supports_noise_blanker: true,
    noise_blanker_level_max: Some(15),
    noise_reduction_level_max: Some(10),
    supports_agc: true,
    supports_tuner: true,
    supports_repeater: true,
    supports_tuning_step: true,
    filter_max_hz: Some(9_999),
    filter_command: "BW",
    af_gain_max: Some(60),
    rf_gain_max: Some(60),
    squelch_max: 40,
    rf_gain_is_attenuation: true,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_true(value: bool) {
        assert!(value);
    }

    #[test]
    fn k4_contract_has_extended_baud_and_attenuation() {
        assert_eq!(PROFILE.model.model_name(), "K4");
        assert!(PROFILE.validate_baud(115_200).is_ok());
        assert_eq!(PROFILE.rf_gain_max, Some(60));
        assert_true(PROFILE.rf_gain_is_attenuation);
    }
}
