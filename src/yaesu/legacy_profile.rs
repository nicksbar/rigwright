//! Declarative profiles for classic five-byte Yaesu CAT radios.

use crate::protocol::yaesu_legacy_cat::{FrequencyModeStatus, LegacyMode, TxStatus};
use crate::{
    hal_types::{MeterId, MeterMetadata, MeterPollSpec},
    models::YaesuLegacyModel,
    ControlId,
};
use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YaesuLegacyProfile {
    pub model: YaesuLegacyModel,
    /// Conservative receive/tuning ranges from the model operating manual.
    pub frequency_ranges: &'static [(u64, u64)],
    /// CAT menu rates. Classic CAT always uses 8 data bits, no parity, and two
    /// stop bits.
    pub baud_rates: &'static [u32],
    /// Modes accepted by the documented `07` set command.
    pub writable_modes: &'static [LegacyMode],
    pub controls: &'static [ControlId],
    pub readable_controls: &'static [ControlId],
    pub writable_controls: &'static [ControlId],
    pub meters: &'static [MeterId],
    pub meter_poll_specs: &'static [MeterPollSpec],
    pub meter_metadata: &'static [MeterMetadata],
    pub supports_repeater_settings: bool,
    /// FT-817ND/FT-818 document radio power commands; these remain deliberately
    /// outside the protocol-neutral HAL because remote power-off is hazardous.
    pub documents_power_commands: bool,
    /// Command/status dialect selected by this profile.
    pub dialect: LegacyCatDialect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyCatDialect {
    Classic,
    Ft1000,
}

impl LegacyCatDialect {
    pub(crate) fn frequency_mode_request(self) -> ([u8; 5], usize) {
        match self {
            Self::Classic => (
                crate::protocol::yaesu_legacy_cat::read_frequency_and_mode(),
                5,
            ),
            Self::Ft1000 => (super::ft1000::read_frequency_mode(), 1_636),
        }
    }

    pub(crate) fn decode_frequency_mode(self, response: &[u8]) -> Result<FrequencyModeStatus> {
        match self {
            Self::Classic => crate::protocol::yaesu_legacy_cat::decode_frequency_and_mode(response),
            Self::Ft1000 => super::ft1000::decode_frequency_mode(response),
        }
    }

    pub(crate) fn set_mode(self, mode: LegacyMode) -> [u8; 5] {
        match self {
            Self::Classic => crate::protocol::yaesu_legacy_cat::set_mode(mode),
            Self::Ft1000 => super::ft1000::set_mode(mode),
        }
    }

    pub(crate) fn tx_status_request(self) -> ([u8; 5], usize) {
        match self {
            Self::Classic => (crate::protocol::yaesu_legacy_cat::read_tx_status(), 1),
            Self::Ft1000 => (super::ft1000::read_tx_status(), 5),
        }
    }

    pub(crate) fn decode_tx_status(self, response: &[u8]) -> Result<TxStatus> {
        match self {
            Self::Classic => crate::protocol::yaesu_legacy_cat::decode_tx_status(response),
            Self::Ft1000 => super::ft1000::decode_tx_status(response),
        }
    }

    pub(crate) fn set_split(self, enabled: bool) -> [u8; 5] {
        match self {
            Self::Classic => crate::protocol::yaesu_legacy_cat::set_split(enabled),
            Self::Ft1000 => super::ft1000::set_split(enabled),
        }
    }

    pub(crate) fn set_frequency(self, hz: u64) -> Result<[u8; 5]> {
        match self {
            Self::Classic => crate::protocol::yaesu_legacy_cat::set_frequency(hz),
            Self::Ft1000 => super::ft1000::set_frequency(hz),
        }
    }

    pub(crate) fn set_ptt(self, enabled: bool) -> [u8; 5] {
        match self {
            Self::Classic => crate::protocol::yaesu_legacy_cat::set_ptt(enabled),
            Self::Ft1000 => super::ft1000::set_ptt(enabled),
        }
    }

    pub(crate) fn response_length(self, frame: [u8; 5]) -> usize {
        match self {
            Self::Classic => match frame[4] {
                0xE7 | 0xF7 => 1,
                0x03 => 5,
                _ => 0,
            },
            Self::Ft1000 => match frame[4] {
                0x10 => 1_636,
                0xFA | 0xF7 => 5,
                _ => 0,
            },
        }
    }

    pub(crate) fn serial_timeout_ms(self) -> u64 {
        match self {
            Self::Classic => 1_200,
            Self::Ft1000 => 5_000,
        }
    }
}

impl YaesuLegacyProfile {
    pub fn supports_frequency(self, hz: u64) -> bool {
        self.frequency_ranges
            .iter()
            .any(|&(low, high)| (low..=high).contains(&hz))
    }

    pub fn supports_mode(self, mode: LegacyMode) -> bool {
        self.writable_modes.contains(&mode)
    }

    pub fn supports_control(self, id: ControlId) -> bool {
        self.controls.contains(&id)
    }

    pub fn supports_control_read(self, id: ControlId) -> bool {
        self.readable_controls.contains(&id)
    }

    pub fn supports_control_write(self, id: ControlId) -> bool {
        self.writable_controls.contains(&id)
    }

    pub fn supports_meter(self, id: MeterId) -> bool {
        self.meters.contains(&id)
    }

    pub fn meter_poll_spec(self, id: MeterId) -> Option<MeterPollSpec> {
        if !self.supports_meter(id) {
            return None;
        }
        self.meter_poll_specs
            .iter()
            .copied()
            .find(|spec| spec.meter == id)
    }

    pub fn meter_metadata(self, id: MeterId) -> Option<MeterMetadata> {
        if !self.supports_meter(id) {
            return None;
        }
        self.meter_metadata
            .iter()
            .copied()
            .find(|spec| spec.meter == id)
    }
}

pub(super) const BAUD_RATES: &[u32] = &[4_800, 9_600, 38_400];
pub(super) const BASE_MODES: &[LegacyMode] = &[
    LegacyMode::Lsb,
    LegacyMode::Usb,
    LegacyMode::Cw,
    LegacyMode::CwReverse,
    LegacyMode::Am,
    LegacyMode::Fm,
    LegacyMode::Digital,
    LegacyMode::Packet,
];
pub(super) const MOBILE_MODES: &[LegacyMode] = &[
    LegacyMode::Lsb,
    LegacyMode::Usb,
    LegacyMode::Cw,
    LegacyMode::CwReverse,
    LegacyMode::Am,
    LegacyMode::Fm,
    LegacyMode::FmNarrow,
    LegacyMode::Digital,
    LegacyMode::Packet,
];
pub(super) const CONTROLS: &[ControlId] = &[ControlId::Split, ControlId::Rit];
pub(super) const READABLE_CONTROLS: &[ControlId] = &[ControlId::Split];
pub(super) const WRITABLE_CONTROLS: &[ControlId] = &[ControlId::Split, ControlId::Rit];
pub(super) const METERS: &[MeterId] = &[MeterId::Signal, MeterId::Power];
pub(super) const METER_POLL_SPECS: &[MeterPollSpec] = &[
    MeterPollSpec {
        meter: MeterId::Signal,
        interval_ms: 400,
        tx_priority: false,
    },
    MeterPollSpec {
        meter: MeterId::Power,
        interval_ms: 300,
        tx_priority: true,
    },
];
pub(super) const METER_METADATA: &[MeterMetadata] = &[
    MeterMetadata {
        meter: MeterId::Signal,
        raw_min: 0,
        raw_max: 15,
        raw_width: 1,
    },
    MeterMetadata {
        meter: MeterId::Power,
        raw_min: 0,
        raw_max: 15,
        raw_width: 1,
    },
];

const GENERIC_RANGES: &[(u64, u64)] = &[(100_000, 470_000_000)];

pub const GENERIC_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Generic,
    frequency_ranges: GENERIC_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: BASE_MODES,
    controls: CONTROLS,
    readable_controls: READABLE_CONTROLS,
    writable_controls: WRITABLE_CONTROLS,
    meters: METERS,
    meter_poll_specs: METER_POLL_SPECS,
    meter_metadata: METER_METADATA,
    supports_repeater_settings: false,
    documents_power_commands: false,
    dialect: LegacyCatDialect::Classic,
};

pub fn profile_for_model(model: YaesuLegacyModel) -> &'static YaesuLegacyProfile {
    match model {
        YaesuLegacyModel::Generic => &GENERIC_PROFILE,
        YaesuLegacyModel::Ft1000 => &super::ft1000::FT1000_PROFILE,
        YaesuLegacyModel::Ft1000D => &super::ft1000::FT1000D_PROFILE,
        YaesuLegacyModel::Ft817Nd => &super::ft817nd::CAT_PROFILE,
        YaesuLegacyModel::Ft818 => &super::ft818::CAT_PROFILE,
        YaesuLegacyModel::Ft857D => &super::ft857d::CAT_PROFILE,
        YaesuLegacyModel::Ft897D => &super::ft897d::CAT_PROFILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaesu::{
        ft1000::{FT1000D_PROFILE, FT1000_PROFILE},
        ft817nd::CAT_PROFILE as FT817ND_PROFILE,
        ft818::CAT_PROFILE as FT818_PROFILE,
        ft857d::CAT_PROFILE as FT857D_PROFILE,
        ft897d::CAT_PROFILE as FT897D_PROFILE,
    };

    #[test]
    fn model_ranges_remain_individual() {
        assert!(!FT817ND_PROFILE.supports_frequency(40_000_000));
        assert!(FT818_PROFILE.supports_frequency(40_000_000));
        assert!(!FT857D_PROFILE.supports_frequency(110_000_000));
        assert!(FT857D_PROFILE.supports_frequency(145_000_000));
    }

    #[test]
    fn only_mobile_profiles_write_narrow_fm() {
        assert!(!FT817ND_PROFILE.supports_mode(LegacyMode::FmNarrow));
        assert!(FT857D_PROFILE.supports_mode(LegacyMode::FmNarrow));
        for profile in [
            FT817ND_PROFILE,
            FT818_PROFILE,
            FT857D_PROFILE,
            FT897D_PROFILE,
        ] {
            assert!(profile.supports_control(ControlId::Split));
            assert!(profile.supports_control(ControlId::Rit));
            assert!(profile.supports_control_read(ControlId::Split));
            assert!(!profile.supports_control_read(ControlId::Rit));
        }
    }

    #[test]
    fn legacy_profiles_expose_meter_metadata_and_polling() {
        for profile in [
            FT817ND_PROFILE,
            FT818_PROFILE,
            FT857D_PROFILE,
            FT897D_PROFILE,
        ] {
            assert!(profile.supports_meter(MeterId::Signal));
            assert!(profile.supports_meter(MeterId::Power));
            assert_eq!(
                profile
                    .meter_poll_spec(MeterId::Signal)
                    .unwrap()
                    .interval_ms,
                400
            );
            assert_eq!(profile.meter_metadata(MeterId::Power).unwrap().raw_max, 15);
        }
    }

    #[test]
    fn generic_profile_does_not_claim_repeater_support() {
        let profile = profile_for_model(YaesuLegacyModel::Generic);
        assert!(!profile.supports_repeater_settings);
        assert!(GENERIC_PROFILE.supports_control(ControlId::Split));
        assert!(GENERIC_PROFILE.supports_meter(MeterId::Signal));
        assert!(!GENERIC_PROFILE.supports_meter(MeterId::Temperature));
        assert!(GENERIC_PROFILE
            .meter_poll_spec(MeterId::Temperature)
            .is_none());
        assert!(GENERIC_PROFILE
            .meter_metadata(MeterId::Temperature)
            .is_none());
    }

    #[test]
    fn ft1000_profiles_are_separate_and_conservative() {
        assert_eq!(profile_for_model(YaesuLegacyModel::Ft1000), &FT1000_PROFILE);
        assert_eq!(
            profile_for_model(YaesuLegacyModel::Ft1000D),
            &FT1000D_PROFILE
        );
        for profile in [FT1000_PROFILE, FT1000D_PROFILE] {
            assert_eq!(profile.dialect, LegacyCatDialect::Ft1000);
            assert_eq!(profile.baud_rates, &[4_800]);
            assert!(profile.supports_mode(LegacyMode::Digital));
            assert!(profile.supports_control(ControlId::Split));
            assert!(!profile.supports_control(ControlId::Rit));
            assert!(!profile.supports_meter(MeterId::Signal));
            assert!(!profile.supports_repeater_settings);
        }
    }

    #[test]
    fn dialect_dispatch_covers_classic_and_ft1000_wire_contracts() {
        let classic = LegacyCatDialect::Classic;
        let ft1000 = LegacyCatDialect::Ft1000;
        assert_eq!(classic.frequency_mode_request().1, 5);
        assert_eq!(ft1000.frequency_mode_request().1, 1_636);
        assert_eq!(
            classic
                .decode_frequency_mode(&[0x01, 0x40, 0x74, 0x00, 0x01])
                .unwrap()
                .frequency_hz,
            14_074_000
        );
        let mut update = vec![0_u8; 1_636];
        update[5..8].copy_from_slice(&[0x15, 0xBE, 0x68]);
        update[11] = 1;
        assert_eq!(
            ft1000.decode_frequency_mode(&update).unwrap().frequency_hz,
            14_250_000
        );
        assert_eq!(classic.set_mode(LegacyMode::Usb)[4], 0x07);
        assert_eq!(ft1000.set_mode(LegacyMode::Usb)[4], 0x0C);
        assert_eq!(classic.tx_status_request().1, 1);
        assert_eq!(ft1000.tx_status_request().1, 5);
        assert!(!classic.decode_tx_status(&[0x80]).unwrap().transmitting);
        assert!(
            ft1000
                .decode_tx_status(&[0x80, 0, 0, 0, 0])
                .unwrap()
                .transmitting
        );
        assert_eq!(classic.set_split(true), [0, 0, 0, 0, 0x02]);
        assert_eq!(ft1000.set_split(true), [0x01, 0, 0, 0, 0x01]);
        assert!(classic.set_frequency(14_074_000).is_ok());
        assert!(ft1000.set_frequency(14_250_000).is_ok());
        assert_eq!(classic.set_ptt(true)[4], 0x08);
        assert_eq!(ft1000.set_ptt(true)[4], 0x0F);
        assert_eq!(classic.response_length([0, 0, 0, 0, 0xE7]), 1);
        assert_eq!(classic.response_length([0, 0, 0, 0, 0x03]), 5);
        assert_eq!(classic.response_length([0; 5]), 0);
        assert_eq!(ft1000.response_length([0, 0, 0, 0, 0x10]), 1_636);
        assert_eq!(ft1000.response_length([0, 0, 0, 0, 0xFA]), 5);
        assert_eq!(ft1000.response_length([0; 5]), 0);
        assert_eq!(classic.serial_timeout_ms(), 1_200);
        assert_eq!(ft1000.serial_timeout_ms(), 5_000);
    }
}
