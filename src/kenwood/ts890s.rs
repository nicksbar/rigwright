//! Kenwood TS-890S model profile (framework only; validation pending).

use super::profile::{KenwoodControlSpec, KenwoodMeterSpec};
use crate::hal_types::{ControlId, MeterId};
use crate::models::{find_model, RadioModelProfile};

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
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::NoiseBlanker,
        command: "NB1",
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
        id: ControlId::Filter,
        command: "FL0",
        response_len: 2,
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::Agc,
        command: "GC",
        response_len: 1,
        max_value: Some(3),
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
pub(crate) const METERS: &[KenwoodMeterSpec] = &[
    KenwoodMeterSpec {
        id: MeterId::Alc,
        command: "RM",
        selector: '1',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Compression,
        command: "RM",
        selector: '3',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Current,
        command: "RM",
        selector: '4',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Voltage,
        command: "RM",
        selector: '5',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Temperature,
        command: "RM",
        selector: '6',
        maximum: 70,
    },
];

pub use super::profile::TS890S_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-890S").expect("built-in TS-890S profile")
}
