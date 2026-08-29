//! Kenwood TS-590SG model profile (framework only; validation pending).

use super::profile::{KenwoodControlSpec, KenwoodMeterSpec};
use crate::hal_types::{ControlId, MeterId};
use crate::models::{find_model, RadioModelProfile};

pub(crate) const CONTROLS: &[KenwoodControlSpec] = &[
    KenwoodControlSpec {
        id: ControlId::AfGain,
        command: "AG",
        response_len: 4,
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
        response_len: 4,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::Preamp,
        command: "PA",
        response_len: 2,
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
        response_len: 2,
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::Filter,
        command: "FL",
        response_len: 1,
        max_value: Some(2),
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
        selector: '3',
        maximum: 30,
    },
    KenwoodMeterSpec {
        id: MeterId::Compression,
        command: "RM",
        selector: '2',
        maximum: 30,
    },
];

pub use super::profile::TS590SG_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-590SG").expect("built-in TS-590SG profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ts590sg_catalog_profile() {
        assert_eq!(profile().model, "TS-590SG");
        assert!(!CONTROLS.is_empty());
        assert!(!METERS.is_empty());
    }
}
