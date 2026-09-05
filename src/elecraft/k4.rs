//! K4 profile from the K4 Programmer's Reference.
use super::profile::{
    ElecraftIdentifyStrategy, ElecraftMeterStrategy, ElecraftModel, ElecraftProfile,
    ElecraftSignalMeterStrategy, ElecraftTxMeterStrategy, ElecraftTxStateStrategy,
    ElecraftVfoMovementStrategy, HF_RANGES, K4_ATTENUATOR_VALUES, K4_BAUD_RATES,
};
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
    // RA$ accepts 0/3/6/9/12/15/18/21 dB plus an enabled flag.
    attenuator_max: Some(21),
    supports_notch: true,
    supports_manual_notch: true,
    supports_noise_blanker: true,
    noise_blanker_level_max: Some(15),
    noise_reduction_level_max: Some(10),
    supports_agc: true,
    agc_max: Some(3),
    supports_tuner: true,
    supports_repeater: true,
    supports_tuning_step: true,
    filter_max_hz: Some(9_999),
    filter_command: "BW",
    af_gain_max: Some(60),
    rf_gain_max: Some(60),
    squelch_max: 40,
    rf_gain_is_attenuation: true,
    identify_strategy: ElecraftIdentifyStrategy::Id,
    auto_info_max: Some(3),
    tx_state_strategy: ElecraftTxStateStrategy::Tqx,
    tx_meter_strategy: ElecraftTxMeterStrategy::K4,
    vfo_movement_strategy: ElecraftVfoMovementStrategy::CurrentStep,
    signal_meter_strategy: ElecraftSignalMeterStrategy::K4 { maximum: 42 },
    power_meter_strategy: Some(ElecraftMeterStrategy {
        command: "PO",
        prefix: "PO",
        maximum: 1100,
    }),
    alc_meter_strategy: None,
    swr_meter_strategy: None,
    memory_channel_max: None,
    repeater_offset_max_hz: Some(99_999_000),
    rit_offset_max_hz: Some(9_999),
    attenuator_values: K4_ATTENUATOR_VALUES,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal_types::MeterId;

    fn assert_true(value: bool) {
        assert!(value);
    }

    #[test]
    fn k4_contract_has_extended_baud_and_attenuation() {
        assert_eq!(PROFILE.model.model_name(), "K4");
        assert!(PROFILE.validate_baud(115_200).is_ok());
        assert_eq!(PROFILE.rf_gain_max, Some(60));
        assert_true(PROFILE.rf_gain_is_attenuation);
        assert_eq!(PROFILE.meter_metadata(MeterId::Signal).unwrap().raw_max, 42);
        assert!(!PROFILE.supports_meter(MeterId::Swr));
    }
}
