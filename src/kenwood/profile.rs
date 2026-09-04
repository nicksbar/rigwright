//! Declarative profiles for Kenwood semicolon-terminated PC control.

use anyhow::{bail, Result};

use crate::{
    hal::Mode,
    hal_types::{ControlId, MeterId, MeterMetadata, MeterPollSpec, SwrSweepSetup},
    models::KenwoodCatModel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KenwoodControlSpec {
    pub id: ControlId,
    pub command: &'static str,
    pub response_len: usize,
    pub max_value: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KenwoodMeterSpec {
    pub id: MeterId,
    pub command: &'static str,
    pub selector: char,
    pub maximum: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KenwoodRitXitLayout {
    IfStatus,
    RfAndFunctionState,
}

#[derive(Clone, Copy)]
pub struct KenwoodMemorySpec {
    pub channel_max: u16,
    pub select_vfo: u8,
    pub select_command: &'static str,
    pub read_command: &'static str,
    pub write_command: &'static str,
    pub read_parameters: fn(u16) -> String,
    pub decode: fn(&str, &KenwoodCatProfile) -> Result<crate::MemoryChannel>,
    pub encode: fn(crate::MemoryChannel, &KenwoodCatProfile) -> Result<String>,
}

impl std::fmt::Debug for KenwoodMemorySpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KenwoodMemorySpec")
            .field("channel_max", &self.channel_max)
            .field("select_vfo", &self.select_vfo)
            .field("select_command", &self.select_command)
            .field("read_command", &self.read_command)
            .field("write_command", &self.write_command)
            .finish_non_exhaustive()
    }
}

fn ts590_read_parameters(channel: u16) -> String {
    format!("0{channel:03}")
}

fn ts890_read_parameters(channel: u16) -> String {
    format!("{channel:03}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KenwoodModeSpec {
    pub code: char,
    pub mode: Mode,
    pub preferred: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KenwoodModeCommand {
    /// Classic one-digit `MD` command. `DA` adds an exact data flag on the
    /// TS-590SG; the TS-2000 has no equivalent documented flag.
    Md { supports_data_flag: bool },
    /// TS-890S `OMP1P2`, where P1 selects the displayed VFO side.
    Om,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KenwoodSplitCommand {
    /// Split is represented by different `FR` and `FT` VFO selections.
    ReceiverTransmitterVfo,
    /// TS-890S provides the direct `TB` split command.
    Tb,
}

#[derive(Debug, Clone, Copy)]
pub struct KenwoodCatProfile {
    pub model: KenwoodCatModel,
    /// Three-digit payload returned by `ID;`.
    pub id_code: &'static str,
    /// Conservative PC-control tuning ranges. Region-specific transmit limits
    /// remain the radio/operator's responsibility.
    pub frequency_ranges: &'static [(u64, u64)],
    pub baud_rates: &'static [u32],
    pub modes: &'static [KenwoodModeSpec],
    pub mode_command: KenwoodModeCommand,
    pub split_command: KenwoodSplitCommand,
    pub supports_vfo: bool,
    pub supports_split: bool,
    /// `IF;` exposes RX/TX state on the older command family.
    pub supports_if_status: bool,
    pub power_range_watts: Option<(u16, u16)>,
    pub meter_max: u16,
    /// Documented maximum for the RM SWR meter-dot value.
    pub swr_meter_max: u16,
    /// RM selector returned when the radio is displaying SWR.
    pub swr_rm_selector: char,
    pub controls: &'static [KenwoodControlSpec],
    pub extra_meters: &'static [KenwoodMeterSpec],
    pub rit_xit_layout: KenwoodRitXitLayout,
    pub memory: Option<KenwoodMemorySpec>,
    pub ai_on_value: &'static str,
    pub sm_payload_len: usize,
    pub sm_value_start: usize,
    pub swr_meter_requires_selection: bool,
}

impl KenwoodCatProfile {
    pub const fn preferred_baud_rate(self) -> u32 {
        self.baud_rates[self.baud_rates.len() - 1]
    }

    pub fn supports_control_read(self, id: ControlId) -> bool {
        self.supports_control(id)
    }

    pub fn supports_control_write(self, id: ControlId) -> bool {
        self.supports_control(id)
    }

    pub fn supported_control_values(self, id: ControlId) -> Option<&'static [u8]> {
        const BINARY: &[u8] = &[0, 1];
        const PREAMP_TS890: &[u8] = &[0, 1, 2];
        const AGC: &[u8] = &[0, 1, 2, 3];
        if !self.supports_control(id) {
            return None;
        }
        match id {
            ControlId::Preamp if self.model == KenwoodCatModel::Ts890S => Some(PREAMP_TS890),
            ControlId::Preamp
            | ControlId::NoiseBlanker
            | ControlId::NoiseReduction
            | ControlId::Notch
            | ControlId::Rit
            | ControlId::Xit => Some(BINARY),
            ControlId::Agc => Some(AGC),
            _ => None,
        }
    }

    pub fn swr_sweep_setup(self) -> Option<SwrSweepSetup> {
        self.power_range_watts.map(|(_, maximum)| SwrSweepSetup {
            carrier_mode: Mode::Rtty,
            rf_power: crate::normalize_meter_level(maximum.min(30), maximum)
                .expect("profile power range must be nonzero"),
        })
    }

    pub fn supports_frequency(self, hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&hz))
    }

    pub fn encode_mode(self, mode: Mode) -> Result<char> {
        self.modes
            .iter()
            .find(|spec| spec.mode == mode && spec.preferred)
            .or_else(|| self.modes.iter().find(|spec| spec.mode == mode))
            .map(|spec| spec.code)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{} does not have a profiled CAT mapping for {mode:?}",
                    self.model.model_name()
                )
            })
    }

    pub fn decode_mode(self, code: char) -> Result<Mode> {
        let code = code.to_ascii_uppercase();
        self.modes
            .iter()
            .find(|spec| spec.code == code)
            .map(|spec| spec.mode)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "unsupported {} CAT mode code: {code}",
                    self.model.model_name()
                )
            })
    }

    pub fn validate_power(self, watts: u16) -> Result<()> {
        let Some((minimum, maximum)) = self.power_range_watts else {
            bail!(
                "RF power control is not profiled for {}",
                self.model.model_name()
            );
        };
        if !(minimum..=maximum).contains(&watts) {
            bail!(
                "{} RF power must be {minimum}..={maximum} W (mode and band may impose a lower maximum)",
                self.model.model_name()
            );
        }
        Ok(())
    }

    pub fn control(self, id: ControlId) -> Option<KenwoodControlSpec> {
        self.controls.iter().copied().find(|spec| spec.id == id)
    }

    pub fn meter(self, id: MeterId) -> Option<KenwoodMeterSpec> {
        self.extra_meters.iter().copied().find(|spec| spec.id == id)
    }

    pub fn supports_meter(self, id: MeterId) -> bool {
        matches!(id, MeterId::Signal | MeterId::Power | MeterId::Swr) || self.meter(id).is_some()
    }

    pub fn meter_poll_spec(self, id: MeterId) -> Option<MeterPollSpec> {
        self.supports_meter(id).then_some(MeterPollSpec {
            meter: id,
            interval_ms: if matches!(id, MeterId::Signal) {
                400
            } else {
                300
            },
            tx_priority: !matches!(id, MeterId::Signal),
        })
    }

    pub fn meter_metadata(self, id: MeterId) -> Option<MeterMetadata> {
        if !self.supports_meter(id) {
            return None;
        }
        let maximum = match id {
            MeterId::Signal | MeterId::Power => self.meter_max,
            MeterId::Swr => self.swr_meter_max,
            _ => self.meter(id)?.maximum,
        };
        let raw_width = match id {
            MeterId::Signal | MeterId::Power => (self.sm_payload_len - self.sm_value_start) as u8,
            _ => 2,
        };
        Some(MeterMetadata {
            meter: id,
            raw_min: 0,
            raw_max: maximum,
            raw_width,
        })
    }

    pub fn supports_control(self, id: ControlId) -> bool {
        self.control(id).is_some()
            || (id == ControlId::RfPower && self.power_range_watts.is_some())
            || (id == ControlId::Vfo && self.supports_vfo)
            || (id == ControlId::Split && self.supports_split)
    }

    pub fn control_max(self, id: ControlId) -> Option<u8> {
        self.control(id)
            .and_then(|spec| spec.max_value)
            .or_else(|| {
                (id == ControlId::RfPower && self.power_range_watts.is_some()).then_some(u8::MAX)
            })
    }
}

const fn mode(code: char, mode: Mode, preferred: bool) -> KenwoodModeSpec {
    KenwoodModeSpec {
        code,
        mode,
        preferred,
    }
}

const STANDARD_MODES: &[KenwoodModeSpec] = &[
    mode('1', Mode::Lsb, true),
    mode('2', Mode::Usb, true),
    // On models without an explicit data flag, generic digital operation uses
    // USB. Decoding remains USB because the radio cannot report the intent.
    mode('2', Mode::Data, false),
    mode('3', Mode::Cw, true),
    mode('4', Mode::Fm, true),
    mode('5', Mode::Am, true),
    mode('6', Mode::Rtty, true),
    mode('7', Mode::CwReverse, true),
    mode('9', Mode::RttyReverse, true),
];

const TS890_MODES: &[KenwoodModeSpec] = &[
    mode('1', Mode::Lsb, true),
    mode('2', Mode::Usb, true),
    mode('3', Mode::Cw, true),
    mode('4', Mode::Fm, true),
    mode('5', Mode::Am, true),
    mode('6', Mode::Rtty, true),
    mode('7', Mode::CwReverse, true),
    mode('9', Mode::RttyReverse, true),
    mode('A', Mode::Data, false), // PSK
    mode('B', Mode::Data, false), // PSK-R
    mode('C', Mode::Data, false), // LSB-D
    mode('D', Mode::Data, true),  // USB-D
    mode('E', Mode::Data, false), // FM-D
    mode('F', Mode::Data, false), // AM-D
];

const MODERN_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];
const TS2000_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600];
const HF_RANGE: &[(u64, u64)] = &[(30_000, 60_000_000)];
const TS2000_RANGES: &[(u64, u64)] = &[
    (30_000, 60_000_000),
    (118_000_000, 174_000_000),
    (220_000_000, 512_000_000),
    (1_240_000_000, 1_300_000_000),
];

/* model command tables moved to ts590sg.rs, ts890s.rs, and ts2000.rs */
/*
const TS590_CONTROLS: &[KenwoodControlSpec] = &[
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

const TS890_CONTROLS: &[KenwoodControlSpec] = &[
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

const TS2000_CONTROLS: &[KenwoodControlSpec] = &[
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

const TS590_METERS: &[KenwoodMeterSpec] = &[
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
const TS890_METERS: &[KenwoodMeterSpec] = &[
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
const NO_EXTRA_METERS: &[KenwoodMeterSpec] = &[];
*/

pub const TS590SG_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: KenwoodCatModel::Ts590Sg,
    id_code: "023",
    frequency_ranges: HF_RANGE,
    baud_rates: MODERN_BAUD_RATES,
    modes: STANDARD_MODES,
    mode_command: KenwoodModeCommand::Md {
        supports_data_flag: true,
    },
    split_command: KenwoodSplitCommand::ReceiverTransmitterVfo,
    supports_vfo: true,
    supports_split: true,
    supports_if_status: true,
    power_range_watts: Some((5, 100)),
    meter_max: 30,
    swr_meter_max: 30,
    swr_rm_selector: '1',
    controls: crate::kenwood::ts590sg::CONTROLS,
    extra_meters: crate::kenwood::ts590sg::METERS,
    rit_xit_layout: KenwoodRitXitLayout::IfStatus,
    memory: Some(KenwoodMemorySpec {
        channel_max: 119,
        select_vfo: 2,
        select_command: "MC",
        read_command: "MR",
        write_command: "MW",
        read_parameters: ts590_read_parameters,
        decode: crate::kenwood::cat_radio::decode_ts590_memory,
        encode: crate::kenwood::cat_radio::encode_ts590_memory,
    }),
    ai_on_value: "2",
    sm_payload_len: 5,
    sm_value_start: 1,
    swr_meter_requires_selection: false,
};

pub const TS890S_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: KenwoodCatModel::Ts890S,
    id_code: "024",
    frequency_ranges: HF_RANGE,
    baud_rates: MODERN_BAUD_RATES,
    modes: TS890_MODES,
    mode_command: KenwoodModeCommand::Om,
    split_command: KenwoodSplitCommand::Tb,
    supports_vfo: true,
    supports_split: true,
    supports_if_status: false,
    power_range_watts: Some((5, 100)),
    meter_max: 70,
    swr_meter_max: 70,
    swr_rm_selector: '2',
    controls: crate::kenwood::ts890s::CONTROLS,
    extra_meters: crate::kenwood::ts890s::METERS,
    rit_xit_layout: KenwoodRitXitLayout::RfAndFunctionState,
    memory: Some(KenwoodMemorySpec {
        channel_max: 119,
        select_vfo: 3,
        select_command: "MN",
        read_command: "MA0",
        write_command: "MA0",
        read_parameters: ts890_read_parameters,
        decode: crate::kenwood::cat_radio::decode_ts890_memory,
        encode: crate::kenwood::cat_radio::encode_ts890_memory,
    }),
    ai_on_value: "2",
    sm_payload_len: 4,
    sm_value_start: 0,
    swr_meter_requires_selection: true,
};

pub const TS2000_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: KenwoodCatModel::Ts2000,
    id_code: "019",
    frequency_ranges: TS2000_RANGES,
    baud_rates: TS2000_BAUD_RATES,
    modes: STANDARD_MODES,
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
    controls: crate::kenwood::ts2000::CONTROLS,
    extra_meters: &[],
    rit_xit_layout: KenwoodRitXitLayout::IfStatus,
    memory: None,
    ai_on_value: "1",
    sm_payload_len: 5,
    sm_value_start: 1,
    swr_meter_requires_selection: false,
};

pub fn profile_for_model(model: KenwoodCatModel) -> &'static KenwoodCatProfile {
    match model {
        KenwoodCatModel::Ts590Sg => &TS590SG_PROFILE,
        KenwoodCatModel::Ts890S => &TS890S_PROFILE,
        KenwoodCatModel::Ts2000 => &TS2000_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identification_and_command_families_are_model_specific() {
        assert_eq!(TS590SG_PROFILE.id_code, "023");
        assert_eq!(TS890S_PROFILE.id_code, "024");
        assert_eq!(TS2000_PROFILE.id_code, "019");
        assert_eq!(TS890S_PROFILE.mode_command, KenwoodModeCommand::Om);
        assert_eq!(
            TS590SG_PROFILE.mode_command,
            KenwoodModeCommand::Md {
                supports_data_flag: true
            }
        );
    }

    #[test]
    fn ts890_has_exact_data_modes_and_a_larger_meter() {
        assert_eq!(TS890S_PROFILE.encode_mode(Mode::Data).unwrap(), 'D');
        assert_eq!(TS890S_PROFILE.decode_mode('C').unwrap(), Mode::Data);
        assert_eq!(TS890S_PROFILE.meter_max, 70);
        assert_eq!(TS590SG_PROFILE.meter_max, 30);
        assert_eq!(TS890S_PROFILE.swr_meter_max, 70);
        assert_eq!(TS2000_PROFILE.swr_meter_max, 30);
        assert_eq!(TS890S_PROFILE.swr_rm_selector, '2');
        assert_eq!(
            TS890S_PROFILE
                .meter_metadata(MeterId::Temperature)
                .unwrap()
                .raw_max,
            70
        );
        assert_eq!(
            TS890S_PROFILE
                .meter_poll_spec(MeterId::Signal)
                .unwrap()
                .interval_ms,
            400
        );
    }

    #[test]
    fn ts2000_ranges_do_not_leak_into_hf_only_models() {
        assert!(TS2000_PROFILE.supports_frequency(145_000_000));
        assert!(!TS590SG_PROFILE.supports_frequency(145_000_000));
        assert!(!TS890S_PROFILE.baud_rates.contains(&1_200));
    }

    #[test]
    fn every_profile_owns_its_optional_control_and_meter_contract() {
        for profile in [TS590SG_PROFILE, TS890S_PROFILE, TS2000_PROFILE] {
            assert!(!profile.controls.is_empty());
            for (index, control) in profile.controls.iter().enumerate() {
                assert!(control.response_len > 0);
                assert!(profile.controls[index + 1..]
                    .iter()
                    .all(|other| other.id != control.id));
            }
            assert!(profile
                .extra_meters
                .iter()
                .all(|meter| meter.command == "RM" && meter.maximum > 0));
        }
        assert!(TS590SG_PROFILE.control(ControlId::Filter).is_some());
        assert!(TS890S_PROFILE.control(ControlId::Agc).is_some());
        assert!(TS2000_PROFILE.control(ControlId::Filter).is_none());
        for profile in [TS590SG_PROFILE, TS890S_PROFILE, TS2000_PROFILE] {
            assert!(profile.supports_control(ControlId::Vfo));
            assert!(profile.supports_control(ControlId::Split));
        }
    }

    #[test]
    fn profile_value_boundaries_and_mode_mappings_are_model_specific() {
        for profile in [TS590SG_PROFILE, TS890S_PROFILE, TS2000_PROFILE] {
            assert!(profile.supports_frequency(30_000));
            assert!(!profile.supports_frequency(29_999));
            assert!(profile.validate_power(5).is_ok());
            assert!(profile.validate_power(100).is_ok());
            assert!(profile.validate_power(4).is_err());
            assert!(profile.validate_power(101).is_err());
            assert!(profile.encode_mode(Mode::Lsb).is_ok());
            assert!(profile.decode_mode('1').is_ok());
            assert!(profile.decode_mode('Z').is_err());
            assert!(profile.control(ControlId::AfGain).is_some());
            assert!(profile.meter(MeterId::Signal).is_none());
            assert!(profile.supports_control(ControlId::Split));
            assert!(profile.supports_control(ControlId::Vfo));
            assert!(!profile.supports_control(ControlId::RawCiV));
        }
        assert!(TS2000_PROFILE.validate_power(100).is_ok());
        assert!(TS2000_PROFILE.supports_frequency(1_250_000_000));
        assert!(!TS2000_PROFILE.supports_frequency(1_301_000_000));
        assert!(TS590SG_PROFILE.meter(MeterId::Alc).is_some());
        assert!(TS890S_PROFILE.meter(MeterId::Temperature).is_some());
        assert!(TS890S_PROFILE.encode_mode(Mode::Data).is_ok());
        assert!(TS590SG_PROFILE.encode_mode(Mode::Wfm).is_err());
        assert!(TS590SG_PROFILE.validate_power(1).is_err());
    }

    #[test]
    fn control_direction_values_and_preferred_baud_are_profile_owned() {
        assert_eq!(TS890S_PROFILE.preferred_baud_rate(), 115_200);
        assert_eq!(TS2000_PROFILE.preferred_baud_rate(), 57_600);
        assert_eq!(
            TS890S_PROFILE.supported_control_values(ControlId::Preamp),
            Some(&[0, 1, 2][..])
        );
        assert!(TS590SG_PROFILE.supports_control_read(ControlId::Filter));
        assert!(TS590SG_PROFILE.supports_control_write(ControlId::Filter));
    }

    #[test]
    fn memory_command_surfaces_are_profile_owned() {
        let ts590 = TS590SG_PROFILE.memory.unwrap();
        assert_eq!(ts590.channel_max, 119);
        assert_eq!(ts590.select_command, "MC");
        assert_eq!((ts590.read_parameters)(7), "0007");

        let ts890 = TS890S_PROFILE.memory.unwrap();
        assert_eq!(ts890.select_command, "MN");
        assert_eq!((ts890.read_parameters)(7), "007");
        assert!(TS2000_PROFILE.memory.is_none());
    }
}
