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
pub struct KenwoodMeterSelection {
    pub command: &'static str,
    pub parameter_suffix: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KenwoodRitXitLayout {
    IfStatus,
    RfAndFunctionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KenwoodRepeaterSpec {
    pub tone_command: &'static str,
    pub tone_mode_command: &'static str,
    pub tone_index_max: u8,
    pub off_value: &'static str,
    pub encode_value: &'static str,
    pub encode_decode_value: &'static str,
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
    pub preamp_values: &'static [u8],
    /// Minimum legal value for the profiled IF filter control.
    pub filter_minimum: u8,
    pub extra_meters: &'static [KenwoodMeterSpec],
    pub supports_signal_meter: bool,
    pub supports_power_meter: bool,
    pub supports_swr_meter: bool,
    pub rit_xit_layout: KenwoodRitXitLayout,
    pub memory: Option<KenwoodMemorySpec>,
    pub ai_on_value: &'static str,
    pub sm_payload_len: usize,
    pub sm_value_start: usize,
    pub swr_meter_selection: Option<KenwoodMeterSelection>,
    pub extra_meter_selection: Option<KenwoodMeterSelection>,
    pub repeater: Option<KenwoodRepeaterSpec>,
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
        const AGC: &[u8] = &[0, 1, 2, 3];
        if !self.supports_control(id) {
            return None;
        }
        match id {
            ControlId::Preamp => Some(self.preamp_values),
            ControlId::NoiseBlanker
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
        match id {
            MeterId::Signal => self.supports_signal_meter,
            MeterId::Power => self.supports_power_meter,
            MeterId::Swr => self.supports_swr_meter,
            _ => self.meter(id).is_some(),
        }
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

    pub fn supports_repeater_settings(self) -> bool {
        self.repeater.is_some()
    }
}

pub const STANDARD_REPEATER: KenwoodRepeaterSpec = KenwoodRepeaterSpec {
    tone_command: "CN",
    tone_mode_command: "CT",
    tone_index_max: 41,
    off_value: "0",
    encode_value: "1",
    encode_decode_value: "2",
};

pub(crate) fn parse_memory_field<T>(value: &str, label: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .trim()
        .parse()
        .map_err(|error| anyhow::anyhow!("invalid {label}: {error}"))
}

pub(crate) fn memory_char(payload: &str, context: &str) -> Result<char> {
    let mut chars = payload.chars();
    let value = chars
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing {context}"))?;
    if chars.next().is_some() {
        bail!("unexpected {context}: {payload}");
    }
    Ok(value)
}

/* Legacy model command tables retained in git history; model modules own them now.
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
pub fn profile_for_model(model: KenwoodCatModel) -> &'static KenwoodCatProfile {
    match model {
        KenwoodCatModel::Generic => &crate::kenwood::generic::CAT_PROFILE,
        KenwoodCatModel::Ts590Sg => &crate::kenwood::ts590sg::CAT_PROFILE,
        KenwoodCatModel::Ts890S => &crate::kenwood::ts890s::CAT_PROFILE,
        KenwoodCatModel::Ts2000 => &crate::kenwood::ts2000::CAT_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kenwood::{
        ts2000::CAT_PROFILE as TS2000_PROFILE, ts590sg::CAT_PROFILE as TS590SG_PROFILE,
        ts890s::CAT_PROFILE as TS890S_PROFILE,
    };

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
