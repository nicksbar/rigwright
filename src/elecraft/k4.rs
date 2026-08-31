//! K4 profile from the K4 Programmer's Reference.
use super::profile::{ElecraftModel, ElecraftProfile, HF_RANGES, K4_BAUD_RATES};
pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K4,
    baud_rates: K4_BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: super::kx3::MODES,
    supports_vfo_b: true,
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
