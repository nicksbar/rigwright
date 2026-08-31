//! KX2 profile from the K3-family Programmer's Reference.
use super::profile::{ElecraftModel, ElecraftProfile, BAUD_RATES, HF_RANGES};
pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::Kx2,
    can_get_frequency: true,
    can_set_frequency: true,
    can_get_mode: true,
    can_set_mode: true,
    can_get_ptt: true,
    can_set_ptt: true,
    frequency_scale_hz: 1,
    frequency_width: 11,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: super::kx3::MODES,
    supports_vfo_b: true,
    supports_split: true,
    supports_rit_xit: true,
    power_max_watts: Some(15),
    preamp_max: Some(1),
    attenuator_max: Some(1),
    supports_noise_blanker: true,
    supports_agc: true,
    supports_tuner: false,
    supports_repeater: false,
    supports_tuning_step: false,
    filter_max_hz: Some(9_999),
    filter_command: "BW",
    af_gain_max: Some(255),
    rf_gain_max: Some(250),
    squelch_max: 29,
    rf_gain_is_attenuation: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_false(value: bool) {
        assert!(!value);
    }

    #[test]
    fn kx2_contract_uses_k3_family_controls() {
        assert_eq!(PROFILE.model.model_name(), "KX2");
        assert_eq!(PROFILE.af_gain_max, Some(255));
        assert_eq!(PROFILE.rf_gain_max, Some(250));
        assert_false(PROFILE.rf_gain_is_attenuation);
    }
}
