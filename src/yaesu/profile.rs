//! Declarative profiles for modern Yaesu ASCII CAT radios.
//!
//! The shared transport owns semicolon framing and response matching. Profiles
//! own facts that vary by model: identification, tuning range, accepted baud
//! rates, mode table, power range, and optional model-specific command groups.

use anyhow::{bail, Result};

use crate::{hal::Mode, models::YaesuCatModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YaesuModeSpec {
    /// One hexadecimal-looking CAT mode character used after the receiver
    /// selector in `MDP1P2`.
    pub code: char,
    /// Protocol-neutral mode exposed by the root HAL.
    pub mode: Mode,
    /// Preferred encoding when several Yaesu modes collapse to one HAL mode.
    pub preferred: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YaesuCatProfile {
    pub model: YaesuCatModel,
    /// Four-character payload returned by `ID;`.
    pub id_code: &'static str,
    /// Conservative CAT-tunable receive ranges, in Hz.
    pub frequency_ranges: &'static [(u64, u64)],
    /// Baud rates exposed by the radio's CAT menu.
    pub baud_rates: &'static [u32],
    /// Model-specific `MD` codes.
    pub modes: &'static [YaesuModeSpec],
    /// Inclusive `PC` power setting range, when implemented.
    pub power_range_watts: Option<(u16, u16)>,
    /// Whether the `ST` split command is implemented by this profile.
    pub supports_split: bool,
}

impl YaesuCatProfile {
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
                "{} RF power must be {minimum}..={maximum} W",
                self.model.model_name()
            );
        }
        Ok(())
    }
}

const HF_RANGE: &[(u64, u64)] = &[(30_000, 75_000_000)];
const FT991A_RANGE: &[(u64, u64)] = &[(30_000, 470_000_000)];
const CLASSIC_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400];
const FT710_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 115_200];

const MODERN_HF_MODES: &[YaesuModeSpec] = &[
    mode('1', Mode::Lsb, true),
    mode('2', Mode::Usb, true),
    mode('3', Mode::Cw, true),
    mode('4', Mode::Fm, true),
    mode('5', Mode::Am, true),
    mode('6', Mode::Rtty, true),
    mode('7', Mode::CwReverse, true),
    mode('8', Mode::Data, false), // DATA-L
    mode('9', Mode::RttyReverse, true),
    mode('A', Mode::Data, false), // DATA-FM
    mode('B', Mode::Fm, false),   // FM-N
    mode('C', Mode::Data, true),  // DATA-U
    mode('D', Mode::Am, false),   // AM-N
    mode('E', Mode::Data, false), // PSK
    mode('F', Mode::Data, false), // DATA-FM-N
];

const FT991A_MODES: &[YaesuModeSpec] = &[
    mode('1', Mode::Lsb, true),
    mode('2', Mode::Usb, true),
    mode('3', Mode::Cw, true),
    mode('4', Mode::Fm, true),
    mode('5', Mode::Am, true),
    mode('6', Mode::Rtty, true),
    mode('7', Mode::CwReverse, true),
    mode('8', Mode::Data, false),
    mode('9', Mode::RttyReverse, true),
    mode('A', Mode::Data, false),
    mode('B', Mode::Fm, false),
    mode('C', Mode::Data, true),
    mode('D', Mode::Am, false),
    mode('E', Mode::Data, false), // C4FM has no narrower root-HAL variant.
];

const fn mode(code: char, mode: Mode, preferred: bool) -> YaesuModeSpec {
    YaesuModeSpec {
        code,
        mode,
        preferred,
    }
}

pub const FT710_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ft710,
    id_code: "0800",
    frequency_ranges: HF_RANGE,
    baud_rates: FT710_BAUD_RATES,
    modes: MODERN_HF_MODES,
    power_range_watts: Some((5, 100)),
    supports_split: true,
};

pub const FTDX10_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx10,
    id_code: "0761",
    frequency_ranges: HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: MODERN_HF_MODES,
    power_range_watts: Some((5, 100)),
    supports_split: true,
};

pub const FTDX101D_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx101D,
    id_code: "0681",
    frequency_ranges: HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: MODERN_HF_MODES,
    power_range_watts: Some((5, 100)),
    supports_split: true,
};

pub const FTDX101MP_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx101Mp,
    id_code: "0682",
    frequency_ranges: HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: MODERN_HF_MODES,
    power_range_watts: Some((5, 200)),
    supports_split: true,
};

pub const FT991A_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ft991A,
    id_code: "0670",
    frequency_ranges: FT991A_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: FT991A_MODES,
    power_range_watts: Some((5, 100)),
    supports_split: false,
};

pub fn profile_for_model(model: YaesuCatModel) -> &'static YaesuCatProfile {
    match model {
        YaesuCatModel::Ft710 => &FT710_PROFILE,
        YaesuCatModel::Ft991A => &FT991A_PROFILE,
        YaesuCatModel::Ftdx10 => &FTDX10_PROFILE,
        YaesuCatModel::Ftdx101D => &FTDX101D_PROFILE,
        YaesuCatModel::Ftdx101Mp => &FTDX101MP_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_identification_codes_are_model_specific() {
        assert_eq!(profile_for_model(YaesuCatModel::Ft710).id_code, "0800");
        assert_eq!(profile_for_model(YaesuCatModel::Ftdx10).id_code, "0761");
        assert_eq!(profile_for_model(YaesuCatModel::Ftdx101D).id_code, "0681");
        assert_eq!(profile_for_model(YaesuCatModel::Ftdx101Mp).id_code, "0682");
        assert_eq!(profile_for_model(YaesuCatModel::Ft991A).id_code, "0670");
    }

    #[test]
    fn data_u_is_the_preferred_generic_data_mapping() {
        for model in [
            YaesuCatModel::Ft710,
            YaesuCatModel::Ft991A,
            YaesuCatModel::Ftdx10,
            YaesuCatModel::Ftdx101D,
            YaesuCatModel::Ftdx101Mp,
        ] {
            let profile = profile_for_model(model);
            assert_eq!(profile.encode_mode(Mode::Data).unwrap(), 'C');
            assert_eq!(profile.decode_mode('8').unwrap(), Mode::Data);
        }
    }

    #[test]
    fn ranges_and_power_limits_do_not_leak_between_models() {
        assert!(!profile_for_model(YaesuCatModel::Ftdx10).supports_frequency(145_000_000));
        assert!(profile_for_model(YaesuCatModel::Ft991A).supports_frequency(145_000_000));
        assert!(profile_for_model(YaesuCatModel::Ftdx101D)
            .validate_power(200)
            .is_err());
        assert!(profile_for_model(YaesuCatModel::Ftdx101Mp)
            .validate_power(200)
            .is_ok());
    }
}
