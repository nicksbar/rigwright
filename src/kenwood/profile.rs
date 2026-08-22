//! Declarative profiles for Kenwood semicolon-terminated PC control.

use anyhow::{bail, Result};

use crate::{hal::Mode, models::KenwoodCatModel};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// `IF;` exposes RX/TX state on the older command family.
    pub supports_if_status: bool,
    pub power_range_watts: Option<(u16, u16)>,
    pub meter_max: u16,
}

impl KenwoodCatProfile {
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
    supports_if_status: true,
    power_range_watts: Some((5, 100)),
    meter_max: 30,
};

pub const TS890S_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: KenwoodCatModel::Ts890S,
    id_code: "024",
    frequency_ranges: HF_RANGE,
    baud_rates: MODERN_BAUD_RATES,
    modes: TS890_MODES,
    mode_command: KenwoodModeCommand::Om,
    split_command: KenwoodSplitCommand::Tb,
    supports_if_status: false,
    power_range_watts: Some((5, 100)),
    meter_max: 70,
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
    supports_if_status: true,
    power_range_watts: Some((5, 100)),
    meter_max: 30,
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
    }

    #[test]
    fn ts2000_ranges_do_not_leak_into_hf_only_models() {
        assert!(TS2000_PROFILE.supports_frequency(145_000_000));
        assert!(!TS590SG_PROFILE.supports_frequency(145_000_000));
        assert!(!TS890S_PROFILE.baud_rates.contains(&1_200));
    }
}
