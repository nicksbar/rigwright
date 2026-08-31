//! K3S profile from the K3-family Programmer's Reference.
use super::profile::{ElecraftModel, ElecraftProfile, BAUD_RATES, HF_RANGES};
pub const PROFILE: ElecraftProfile = ElecraftProfile {
    model: ElecraftModel::K3s,
    baud_rates: BAUD_RATES,
    frequency_ranges: HF_RANGES,
    modes: super::kx3::MODES,
    supports_vfo_b: false,
    supports_split: true,
    supports_rit_xit: true,
    power_max_watts: Some(110),
    preamp_max: Some(2),
    attenuator_max: Some(15),
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
    fn k3s_contract_is_separate_from_k3_lookup() {
        assert_eq!(PROFILE.model.model_name(), "K3S");
        assert_eq!(PROFILE.modes, super::super::kx3::MODES);
        assert_eq!(PROFILE.squelch_max, 29);
    }
}
