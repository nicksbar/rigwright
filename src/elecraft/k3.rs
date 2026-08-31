//! K3 profile from the K3-family Programmer's Reference.
use super::profile::{ElecraftModel, ElecraftProfile, BAUD_RATES, HF_RANGES};
pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K3,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: super::kx3::MODES,
    supports_vfo_b: true,
    af_gain_max: Some(255),
    rf_gain_max: Some(250),
    squelch_max: 29,
    rf_gain_is_attenuation: false,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k3_contract_is_named_and_profiled() {
        assert_eq!(PROFILE.model.model_name(), "K3");
        assert!(PROFILE.supports_frequency(14_074_000));
        assert_eq!(PROFILE.squelch_max, 29);
    }
}
