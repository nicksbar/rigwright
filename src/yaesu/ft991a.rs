//! Yaesu FT-991A model profile (framework only; hardware validation pending).

use super::profile::{
    YaesuCatProfile, COMMON_CONTROLS, COMMON_METERS, CONTROL_MAXES, CONTROL_VALUES, METER_METADATA,
    METER_POLL_SPECS, METER_SELECTORS,
};
use crate::models::{find_model, RadioModelProfile, YaesuCatModel};

const MODES: &[super::profile::YaesuModeSpec] = &[
    super::profile::YaesuModeSpec {
        code: '1',
        mode: crate::hal::Mode::Lsb,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '2',
        mode: crate::hal::Mode::Usb,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '3',
        mode: crate::hal::Mode::Cw,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '4',
        mode: crate::hal::Mode::Fm,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '5',
        mode: crate::hal::Mode::Am,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '6',
        mode: crate::hal::Mode::Rtty,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '7',
        mode: crate::hal::Mode::CwReverse,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: '8',
        mode: crate::hal::Mode::Data,
        preferred: false,
    },
    super::profile::YaesuModeSpec {
        code: '9',
        mode: crate::hal::Mode::RttyReverse,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: 'A',
        mode: crate::hal::Mode::Data,
        preferred: false,
    },
    super::profile::YaesuModeSpec {
        code: 'B',
        mode: crate::hal::Mode::Fm,
        preferred: false,
    },
    super::profile::YaesuModeSpec {
        code: 'C',
        mode: crate::hal::Mode::Data,
        preferred: true,
    },
    super::profile::YaesuModeSpec {
        code: 'D',
        mode: crate::hal::Mode::Am,
        preferred: false,
    },
    super::profile::YaesuModeSpec {
        code: 'E',
        mode: crate::hal::Mode::Data,
        preferred: false,
    },
];

pub const CAT_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ft991A,
    id_code: Some("0670"),
    frequency_ranges: &[(30_000, 470_000_000)],
    baud_rates: super::profile::CLASSIC_BAUD_RATES,
    usb_baud_rates: super::profile::CLASSIC_BAUD_RATES,
    supports_auto_baud: false,
    preferred_baud_rate: 38_400,
    modes: MODES,
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
    memory_channel_max: 117,
    memory_frequency_max_hz: 999_999_999,
    memory_offset_max_hz: 9_990,
    memory_name_max_len: 12,
    repeater_tone_index_max: 49,
    if_shift_max_hz: 1_200,
    rit_offset_max_hz: 9_999,
    vox_delay_max: 33,
    noise_blanker_level_max: 10,
    cat_rts_menu: Some("033"),
    supports_vfo_selector_query: false,
    uses_if_for_mode_read: USES_IF_FOR_MODE_READ,
};

/// The FT-991A exposes mode through its combined `IF;` status response and
/// must not retry the rejected `MD0;` query when that response is unavailable.
pub const USES_IF_FOR_MODE_READ: bool = true;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-991A").expect("built-in FT-991A profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft991a_catalog_profile() {
        assert_eq!(profile().model, "FT-991A");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuCatModel::Ft991A);
    }
}
