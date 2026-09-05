//! Kenwood TS-2000 model profile (framework only; validation pending).

use super::profile::{
    KenwoodCatProfile, KenwoodControlSpec, KenwoodModeCommand, KenwoodModeSpec,
    KenwoodRitXitLayout, KenwoodSplitCommand,
};
use crate::hal::Mode;
use crate::hal_types::ControlId;
use crate::models::{find_model, RadioModelProfile};

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
const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600];
const FREQUENCY_RANGES: &[(u64, u64)] = &[
    (30_000, 60_000_000),
    (118_000_000, 174_000_000),
    (220_000_000, 512_000_000),
    (1_240_000_000, 1_300_000_000),
];

pub(crate) const CONTROLS: &[KenwoodControlSpec] = &[
    KenwoodControlSpec {
        id: ControlId::AfGain,
        command: "AG",
        response_len: 3,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::RfGain,
        command: "RG",
        response_len: 3,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::Squelch,
        command: "SQ",
        response_len: 3,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::Preamp,
        command: "PA",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::NoiseBlanker,
        command: "NB",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::NoiseReduction,
        command: "NR",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::Notch,
        command: "NT",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::Rit,
        command: "RT",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::Xit,
        command: "XT",
        response_len: 1,
        max_value: Some(1),
    },
];

pub const CAT_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: crate::models::KenwoodCatModel::Ts2000,
    id_code: "019",
    frequency_ranges: FREQUENCY_RANGES,
    baud_rates: BAUD_RATES,
    modes: MODES,
    mode_command: KenwoodModeCommand::Md {
        supports_data_flag: false,
    },
    split_command: KenwoodSplitCommand::ReceiverTransmitterVfo,
    supports_vfo: true,
    supports_split: true,
    supports_if_status: true,
    power_range_watts: Some((5, 100)),
    meter_max: 30,
    swr_meter_max: 30,
    swr_rm_selector: '1',
    controls: CONTROLS,
    preamp_values: &[0, 1],
    filter_minimum: 0,
    extra_meters: &[],
    supports_signal_meter: true,
    supports_power_meter: true,
    supports_swr_meter: true,
    rit_xit_layout: KenwoodRitXitLayout::IfStatus,
    memory: None,
    ai_on_value: "1",
    sm_payload_len: 5,
    sm_value_start: 1,
    swr_meter_selection: None,
    extra_meter_selection: None,
    repeater: Some(super::profile::STANDARD_REPEATER),
};

pub use CAT_PROFILE as TS2000_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-2000").expect("built-in TS-2000 profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ts2000_catalog_profile() {
        assert_eq!(profile().model, "TS-2000");
        assert!(!CONTROLS.is_empty());
    }
}
