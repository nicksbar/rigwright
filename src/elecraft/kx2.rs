//! KX2 profile from the K3-family Programmer's Reference.
use super::profile::{
    ElecraftIdentifyStrategy, ElecraftMeterStrategy, ElecraftModel, ElecraftProfile,
    ElecraftSignalMeterStrategy, ElecraftTxMeterStrategy, ElecraftTxStateStrategy,
    ElecraftVfoMovementStrategy, BAUD_RATES, EMPTY_VALUES, HF_RANGES,
};
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
    antenna_max: Some(2),
    preamp_max: Some(1),
    attenuator_max: Some(1),
    supports_notch: false,
    supports_manual_notch: false,
    supports_noise_blanker: true,
    noise_blanker_level_max: None,
    noise_reduction_level_max: None,
    supports_agc: true,
    agc_max: Some(3),
    supports_tuner: false,
    supports_repeater: false,
    supports_tuning_step: false,
    filter_max_hz: Some(9_999),
    filter_command: "BW",
    af_gain_max: Some(255),
    rf_gain_max: Some(250),
    squelch_max: 29,
    rf_gain_is_attenuation: false,
    identify_strategy: ElecraftIdentifyStrategy::Id,
    auto_info_max: Some(3),
    tx_state_strategy: ElecraftTxStateStrategy::Tq,
    tx_meter_strategy: ElecraftTxMeterStrategy::None,
    vfo_movement_strategy: ElecraftVfoMovementStrategy::StepIndexed { maximum: 9 },
    signal_meter_strategy: ElecraftSignalMeterStrategy::Sm { maximum: 30 },
    power_meter_strategy: Some(ElecraftMeterStrategy {
        command: "BG",
        prefix: "BG",
        maximum: 12,
    }),
    alc_meter_strategy: None,
    swr_meter_strategy: Some(ElecraftMeterStrategy {
        command: "SW",
        prefix: "SW",
        maximum: 999,
    }),
    memory_channel_max: Some(999),
    repeater_offset_max_hz: None,
    rit_offset_max_hz: Some(9_999),
    attenuator_values: EMPTY_VALUES,
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
