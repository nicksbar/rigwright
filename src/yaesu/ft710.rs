//! Yaesu FT-710 model profile (framework only; hardware validation pending).

use super::profile::{
    YaesuCatProfile, COMMON_CONTROLS, COMMON_METERS, CONTROL_MAXES, CONTROL_VALUES, METER_METADATA,
    METER_POLL_SPECS, METER_SELECTORS, MODERN_HF_MODES,
};

const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 115_200];
use crate::models::{find_model, RadioModelProfile, YaesuCatModel};

pub const CAT_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ft710,
    id_code: Some("0800"),
    frequency_ranges: super::profile::HF_RANGE,
    baud_rates: BAUD_RATES,
    usb_baud_rates: BAUD_RATES,
    supports_auto_baud: false,
    preferred_baud_rate: 115_200,
    modes: MODERN_HF_MODES,
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    control_values: CONTROL_VALUES,
    meters: COMMON_METERS,
    meter_poll_specs: METER_POLL_SPECS,
    meter_metadata: METER_METADATA,
    meter_selectors: METER_SELECTORS,
    power_range_watts: Some((5, 100)),
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
    cat_rts_menu: None,
    supports_vfo_selector_query: true,
    uses_if_for_mode_read: false,
};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-710").expect("built-in FT-710 profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft710_catalog_profile() {
        assert_eq!(profile().model, "FT-710");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuCatModel::Ft710);
    }
}
