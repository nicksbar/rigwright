//! Declarative profiles for modern Yaesu ASCII CAT radios.
//!
//! The shared transport owns semicolon framing and response matching. Profiles
//! own facts that vary by model: identification, tuning range, accepted baud
//! rates, mode table, power range, and optional model-specific command groups.

use anyhow::{bail, Result};

use crate::{
    hal::Mode,
    hal_types::{ControlId, SwrSweepSetup},
    models::YaesuCatModel,
};

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
pub struct YaesuControlSpec {
    pub id: ControlId,
    pub command: &'static str,
    pub readable: bool,
    pub writable: bool,
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
    /// Controls shared by the model's documented CAT surface.
    pub controls: &'static [YaesuControlSpec],
    /// Model-owned maxima for indexed controls.
    pub control_maxes: &'static [(ControlId, u8)],
    /// Inclusive `PC` power setting range, when implemented.
    pub power_range_watts: Option<(u16, u16)>,
    /// Whether the `ST` split command is implemented by this profile.
    pub supports_split: bool,
    /// The model manual documents `CN`, `CT`, and `OS` CAT operations.
    pub supports_repeater_settings: bool,
    /// The model manual documents `MC`, `MR`, and `MT` memory operations.
    pub supports_memory_channels: bool,
    /// `EX` menu selector that reads the radio's CAT RTS (hardware flow
    /// control) setting, when the model documents one. The selector is the
    /// model's own `EX` menu address, not a shared value: the FTDX10 and
    /// FTDX101D/MP use the hierarchical `PP II SS` form (CAT RTS = `030313`),
    /// while the FT-991A uses the flat `PPP` menu number (CAT RTS = menu
    /// `033`). The FT-710 has no CAT RTS menu at all (its standard-port RTS
    /// is a PTT source via `RPTT SELECT`), so it leaves this `None`.
    pub cat_rts_menu: Option<&'static str>,
}

impl YaesuCatProfile {
    pub fn filter_bandwidth_hz(self, mode: Mode, filter: u8) -> Option<u32> {
        if !matches!(
            mode,
            Mode::Lsb | Mode::Usb | Mode::Cw | Mode::Rtty | Mode::RttyReverse
        ) {
            return None;
        }
        let widths = if matches!(mode, Mode::Lsb | Mode::Usb) {
            [
                0, 300, 400, 600, 850, 1100, 1200, 1500, 1650, 1800, 1950, 2100, 2250, 2400, 2450,
                2500, 2600, 2700, 2800, 2900, 3000, 3200, 3500, 4000,
            ]
        } else {
            [
                0, 50, 100, 150, 200, 250, 300, 350, 400, 450, 500, 600, 800, 1200, 1400, 1700,
                2000, 2400, 3000, 3200, 3500, 4000, 0, 0,
            ]
        };
        widths
            .get(usize::from(filter))
            .copied()
            .filter(|&hz| hz != 0)
    }

    pub fn swr_sweep_setup(self) -> Option<SwrSweepSetup> {
        self.power_range_watts.map(|(_, maximum)| SwrSweepSetup {
            carrier_mode: Mode::Rtty,
            rf_power: crate::normalize_meter_level(maximum.min(30), maximum)
                .expect("profile power range must be nonzero"),
        })
    }

    pub fn supports_control(self, id: ControlId) -> bool {
        self.controls.iter().any(|spec| spec.id == id)
            || (id == ControlId::RfPower && self.power_range_watts.is_some())
            || (id == ControlId::Split && self.supports_split)
    }

    pub fn control(self, id: ControlId) -> Option<YaesuControlSpec> {
        self.controls
            .iter()
            .copied()
            .find(|spec| spec.id == id)
            .or_else(|| {
                (id == ControlId::RfPower && self.power_range_watts.is_some()).then_some(
                    YaesuControlSpec {
                        id,
                        command: "PC",
                        readable: true,
                        writable: true,
                    },
                )
            })
            .or_else(|| {
                (id == ControlId::Split && self.supports_split).then_some(YaesuControlSpec {
                    id,
                    command: "ST",
                    readable: true,
                    writable: true,
                })
            })
    }

    pub fn supports_control_read(self, id: ControlId) -> bool {
        self.control(id).is_some_and(|spec| spec.readable)
    }

    pub fn supports_control_write(self, id: ControlId) -> bool {
        self.control(id).is_some_and(|spec| spec.writable)
    }

    pub fn control_max(self, id: ControlId) -> Option<u8> {
        self.control_maxes
            .iter()
            .find_map(|&(control, maximum)| (control == id).then_some(maximum))
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
const COMMON_CONTROLS: &[YaesuControlSpec] = &[
    control(ControlId::AfGain, "AG"),
    control(ControlId::RfGain, "RG"),
    control(ControlId::Squelch, "SQ"),
    control(ControlId::Preamp, "PA"),
    control(ControlId::Attenuator, "RA"),
    control(ControlId::NoiseBlanker, "NB"),
    control(ControlId::Notch, "BC"),
    control(ControlId::ManualNotch, "BP"),
    control(ControlId::Filter, "SH"),
    control(ControlId::Agc, "GT"),
    control(ControlId::NoiseReduction, "NR"),
    control(ControlId::NoiseReductionLevel, "RL"),
    control(ControlId::Rit, "RT"),
    control(ControlId::Xit, "XT"),
    control(ControlId::Tuner, "AC"),
    control(ControlId::Vfo, "VS"),
];

const fn control(id: ControlId, command: &'static str) -> YaesuControlSpec {
    YaesuControlSpec {
        id,
        command,
        readable: true,
        writable: true,
    }
}
const CONTROL_MAXES: &[(ControlId, u8)] = &[
    (ControlId::Preamp, 2),
    (ControlId::Attenuator, 3),
    (ControlId::Filter, 23),
    (ControlId::Agc, 4),
    (ControlId::NoiseReductionLevel, 15),
];

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
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    power_range_watts: Some((5, 100)),
    supports_split: true,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    // The FT-710 manual documents no CAT RTS menu; RTS on its standard COM
    // port is a PTT source configured by `RPTT SELECT`, not CAT flow control.
    cat_rts_menu: None,
};

pub const FTDX10_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx10,
    id_code: "0761",
    frequency_ranges: HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: MODERN_HF_MODES,
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    power_range_watts: Some((5, 100)),
    supports_split: true,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    // FTDX10 CAT RTS is menu 03-03-10, read as hierarchical `EX030310;`.
    cat_rts_menu: Some("030310"),
};

pub const FTDX101D_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx101D,
    id_code: "0681",
    frequency_ranges: HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: MODERN_HF_MODES,
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    power_range_watts: Some((5, 100)),
    supports_split: true,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    // FTDX101D CAT RTS is menu 03-03-13, read as hierarchical `EX030313;`.
    cat_rts_menu: Some("030313"),
};

pub const FTDX101MP_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ftdx101Mp,
    id_code: "0682",
    frequency_ranges: HF_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: MODERN_HF_MODES,
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    power_range_watts: Some((5, 200)),
    supports_split: true,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    // FTDX101MP CAT RTS is menu 03-03-13, read as hierarchical `EX030313;`.
    cat_rts_menu: Some("030313"),
};

pub const FT991A_PROFILE: YaesuCatProfile = YaesuCatProfile {
    model: YaesuCatModel::Ft991A,
    id_code: "0670",
    frequency_ranges: FT991A_RANGE,
    baud_rates: CLASSIC_BAUD_RATES,
    modes: FT991A_MODES,
    controls: COMMON_CONTROLS,
    control_maxes: CONTROL_MAXES,
    power_range_watts: Some((5, 100)),
    // The FT-991A CAT manual lists ST (SPLIT) as supported.
    supports_split: true,
    supports_repeater_settings: true,
    supports_memory_channels: true,
    // FT-991A CAT RTS is the flat menu 033, read as `EX033;` (not the
    // hierarchical selectors used by the FTDX10/FTDX101 family).
    cat_rts_menu: Some("033"),
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

    #[test]
    fn modern_profiles_expose_documented_repeater_and_memory_surfaces() {
        for model in [
            YaesuCatModel::Ft710,
            YaesuCatModel::Ft991A,
            YaesuCatModel::Ftdx10,
            YaesuCatModel::Ftdx101D,
            YaesuCatModel::Ftdx101Mp,
        ] {
            let profile = profile_for_model(model);
            assert!(profile.supports_repeater_settings);
            assert!(profile.supports_memory_channels);
        }
    }

    #[test]
    fn ft991a_exposes_documented_split_control() {
        let profile = profile_for_model(YaesuCatModel::Ft991A);
        assert!(profile.supports_split);
        assert!(profile.supports_control(ControlId::Split));
    }

    #[test]
    fn cat_rts_menu_selectors_are_unique_to_each_models_ex_layout() {
        // FT-710 documents no CAT RTS menu; its standard-port RTS is PTT.
        assert_eq!(profile_for_model(YaesuCatModel::Ft710).cat_rts_menu, None);
        // FT-991A uses the flat 3-digit menu number for CAT RTS (menu 033).
        assert_eq!(
            profile_for_model(YaesuCatModel::Ft991A).cat_rts_menu,
            Some("033")
        );
        // FTDX10 uses 03-03-10; FTDX101D/MP use 03-03-13.
        assert_eq!(
            profile_for_model(YaesuCatModel::Ftdx10).cat_rts_menu,
            Some("030310")
        );
        for model in [YaesuCatModel::Ftdx101D, YaesuCatModel::Ftdx101Mp] {
            assert_eq!(
                profile_for_model(model).cat_rts_menu,
                Some("030313"),
                "{} uses the hierarchical EX selector",
                model.model_name()
            );
        }
    }

    #[test]
    fn common_controls_own_their_documented_cat_commands() {
        let profile = profile_for_model(YaesuCatModel::Ft710);
        assert_eq!(profile.control(ControlId::AfGain).unwrap().command, "AG");
        assert_eq!(
            profile.control(ControlId::ManualNotch).unwrap().command,
            "BP"
        );
        assert_eq!(profile.control(ControlId::Tuner).unwrap().command, "AC");
        assert_eq!(profile.control(ControlId::RfPower).unwrap().command, "PC");
        assert_eq!(profile.control(ControlId::Split).unwrap().command, "ST");
        assert!(profile.supports_control_read(ControlId::RfPower));
        assert!(profile.supports_control_write(ControlId::Split));
    }
}
