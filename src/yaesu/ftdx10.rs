//! FTDX10-specific CAT commands (hardware validated).
//!
//! Common frequency-A, mode, PTT, RF-power, split, framing, and raw-query
//! behavior belongs to `YaesuCatRadio`. This module contains only typed helpers
//! for useful FTDX10 commands that are not represented by the generic driver.

use anyhow::{bail, Result};

use super::profile::{
    YaesuCatProfile, CLASSIC_BAUD_RATES, COMMON_CONTROLS, COMMON_METERS, CONTROL_MAXES,
    CONTROL_VALUES, METER_METADATA, METER_POLL_SPECS, METER_SELECTORS, MODERN_HF_MODES,
};
use crate::{
    models::{find_model, RadioModelProfile, YaesuCatModel},
    protocol::ascii_cat,
};

pub const CAT_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx10,
    id_code: Some("0761"),
    frequency_ranges: super::profile::HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    usb_baud_rates: CLASSIC_BAUD_RATES,
    supports_auto_baud: false,
    preferred_baud_rate: 38_400,
    modes: MODERN_HF_MODES,
    controls: COMMON_CONTROLS,
    filter_width_prefix: "00",
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
    cat_rts_menu: Some("030310"),
    supports_vfo_selector_query: true,
    uses_if_for_mode_read: false,
};

pub fn set_vfo_b_frequency(hz: u64) -> Result<Vec<u8>> {
    if !CAT_PROFILE.supports_frequency(hz) {
        bail!("FTDX10 frequency is outside the CAT range: {hz} Hz");
    }
    ascii_cat::encode("FB", Some(&format!("{hz:09}")))
}

pub fn read_vfo_b_frequency() -> Result<Vec<u8>> {
    ascii_cat::encode("FB", None)
}

pub fn read_information() -> Result<Vec<u8>> {
    ascii_cat::encode("IF", None)
}

pub fn read_s_meter() -> Result<Vec<u8>> {
    ascii_cat::encode("SM", Some("0"))
}

pub fn read_received_meter(meter: u8) -> Result<Vec<u8>> {
    if meter > 8 || meter == 2 {
        bail!("FTDX10 received-meter selector must be 0, 1, or 3..=8");
    }
    ascii_cat::encode("RM", Some(&meter.to_string()))
}

pub fn set_filter_width(width: u16) -> Result<Vec<u8>> {
    if width > 23 {
        bail!("FTDX10 filter-width table index must be 0..=23");
    }
    ascii_cat::encode("SH", Some(&format!("00{width:02}")))
}

pub fn set_agc(function: u8) -> Result<Vec<u8>> {
    if function > 4 {
        bail!("FTDX10 AGC set value must be 0..=4");
    }
    ascii_cat::encode("GT", Some(&format!("0{function}")))
}

pub fn set_noise_reduction(enabled: bool) -> Result<Vec<u8>> {
    ascii_cat::encode("NR", Some(if enabled { "01" } else { "00" }))
}

pub fn set_noise_reduction_level(level: u8) -> Result<Vec<u8>> {
    if !(1..=15).contains(&level) {
        bail!("FTDX10 noise-reduction level must be 1..=15");
    }
    ascii_cat::encode("RL", Some(&format!("0{level:02}")))
}

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX10").expect("built-in FTDX10 profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ftdx10_specific_commands() {
        assert_eq!(set_vfo_b_frequency(14_250_000).unwrap(), b"FB014250000;");
        assert_eq!(read_vfo_b_frequency().unwrap(), b"FB;");
        assert_eq!(read_information().unwrap(), b"IF;");
        assert_eq!(read_s_meter().unwrap(), b"SM0;");
        assert_eq!(read_received_meter(1).unwrap(), b"RM1;");
        assert_eq!(set_filter_width(12).unwrap(), b"SH0012;");
        assert_eq!(set_agc(4).unwrap(), b"GT04;");
        assert_eq!(set_noise_reduction(true).unwrap(), b"NR01;");
        assert_eq!(set_noise_reduction_level(7).unwrap(), b"RL007;");
    }

    #[test]
    fn rejects_invalid_model_specific_values() {
        assert!(set_vfo_b_frequency(75_000_001).is_err());
        assert!(read_received_meter(2).is_err());
        assert!(set_filter_width(24).is_err());
        assert!(set_agc(5).is_err());
        assert!(set_noise_reduction_level(0).is_err());
        assert!(set_noise_reduction_level(16).is_err());
    }
}
