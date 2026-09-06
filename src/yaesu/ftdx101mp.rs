//! Yaesu FTDX101MP model profile (framework only; hardware validation pending).

use super::profile::{
    YaesuCatProfile, CLASSIC_BAUD_RATES, COMMON_CONTROLS, CONTROL_MAXES, CONTROL_VALUES,
    METER_METADATA, METER_POLL_SPECS, METER_SELECTORS, MODERN_HF_MODES,
};
use crate::models::{find_model, RadioModelProfile, YaesuCatModel};

pub const CAT_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx101Mp,
    id_code: Some("0682"),
    frequency_ranges: super::profile::HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    usb_baud_rates: CLASSIC_BAUD_RATES,
    supports_auto_baud: false,
    preferred_baud_rate: 38_400,
    modes: MODERN_HF_MODES,
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    control_values: CONTROL_VALUES,
    meters: super::ftdx101::METERS,
    meter_poll_specs: METER_POLL_SPECS,
    meter_metadata: METER_METADATA,
    meter_selectors: METER_SELECTORS,
    power_range_watts: Some((5, 200)),
    supports_split: true,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    memory_channel_max: 99,
    memory_frequency_max_hz: 999_999_999,
    memory_offset_max_hz: 9_990,
    memory_name_max_len: 12,
    repeater_tone_index_max: 49,
    if_shift_max_hz: 1_200,
    rit_offset_max_hz: 9_999,
    vox_delay_max: 33,
    noise_blanker_level_max: 10,
    cat_rts_menu: Some("030313"),
    supports_vfo_selector_query: true,
    uses_if_for_mode_read: false,
};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX101MP").expect("built-in FTDX101MP profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ftdx101mp_catalog_profile() {
        assert_eq!(profile().model, "FTDX101MP");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuCatModel::Ftdx101Mp);
    }
}
