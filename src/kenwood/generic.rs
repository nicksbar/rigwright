//! Conservative profile for an unidentified Kenwood semicolon CAT radio.

use super::profile::{
    KenwoodCatProfile, KenwoodModeCommand, KenwoodModeSpec, KenwoodRitXitLayout,
    KenwoodSplitCommand,
};
use crate::hal::Mode;
use crate::models::KenwoodCatModel;

const MODES: &[KenwoodModeSpec] = &[
    KenwoodModeSpec {
        code: '1',
        mode: Mode::Lsb,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '2',
        mode: Mode::Usb,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '2',
        mode: Mode::Data,
        preferred: false,
    },
    KenwoodModeSpec {
        code: '3',
        mode: Mode::Cw,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '4',
        mode: Mode::Fm,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '5',
        mode: Mode::Am,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '6',
        mode: Mode::Rtty,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '7',
        mode: Mode::CwReverse,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '9',
        mode: Mode::RttyReverse,
        preferred: true,
    },
];

/// Rates and ranges deliberately use conservative values shared by the
/// documented Kenwood profiles. Model-specific extensions require a model
/// profile instead of being guessed here.
const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];
const FREQUENCY_RANGES: &[(u64, u64)] = &[(30_000, 60_000_000)];

pub const CAT_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: KenwoodCatModel::Generic,
    id_code: "",
    frequency_ranges: FREQUENCY_RANGES,
    baud_rates: BAUD_RATES,
    modes: MODES,
    mode_command: KenwoodModeCommand::Md {
        supports_data_flag: false,
    },
    split_command: KenwoodSplitCommand::ReceiverTransmitterVfo,
    supports_vfo: true,
    supports_split: true,
    supports_if_status: false,
    power_range_watts: None,
    meter_max: 30,
    swr_meter_max: 30,
    swr_rm_selector: '1',
    controls: &[],
    preamp_values: &[],
    filter_minimum: 0,
    extra_meters: &[],
    supports_signal_meter: true,
    supports_power_meter: true,
    supports_swr_meter: false,
    rit_xit_layout: KenwoodRitXitLayout::IfStatus,
    memory: None,
    ai_on_value: "1",
    // TS-590SG/TS-2000-style SM response is the conservative common default.
    sm_payload_len: 5,
    sm_value_start: 1,
    swr_meter_selection: None,
    extra_meter_selection: None,
    repeater: None,
};
