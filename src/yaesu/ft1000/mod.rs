//! Yaesu FT-1000/FT-1000D legacy CAT profiles.
//!
//! These radios use the original Yaesu five-byte CAT protocol, but their
//! opcode and status-update layout is distinct from the FT-817/857 family.

use anyhow::{bail, Result};

use crate::{
    hal_types::ControlId,
    models::{find_model, RadioModelProfile, YaesuLegacyModel},
    protocol::yaesu_legacy_cat::{FrequencyModeStatus, LegacyMode, TxStatus},
};

use super::legacy_profile::{
    LegacyCatDialect, YaesuLegacyProfile, METER_METADATA, METER_POLL_SPECS,
};

const FREQUENCY_RANGES: &[(u64, u64)] = &[
    (1_800_000, 1_999_999),
    (3_500_000, 3_999_999),
    (7_000_000, 7_299_999),
    (10_100_000, 10_149_999),
    (14_000_000, 14_349_999),
    (18_068_000, 18_167_999),
    (21_000_000, 21_449_999),
    (24_890_000, 24_989_999),
    (28_000_000, 29_699_999),
];
const BAUD_RATES: &[u32] = &[4_800];
const MODES: &[LegacyMode] = &[
    LegacyMode::Lsb,
    LegacyMode::Usb,
    LegacyMode::Cw,
    LegacyMode::Am,
    LegacyMode::Fm,
    LegacyMode::Digital,
    LegacyMode::Packet,
];
const CONTROLS: &[ControlId] = &[ControlId::Split];
const READABLE_CONTROLS: &[ControlId] = &[ControlId::Split];
const WRITABLE_CONTROLS: &[ControlId] = &[ControlId::Split];

pub const FT1000_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft1000,
    frequency_ranges: FREQUENCY_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: MODES,
    controls: CONTROLS,
    readable_controls: READABLE_CONTROLS,
    writable_controls: WRITABLE_CONTROLS,
    meters: &[],
    meter_poll_specs: METER_POLL_SPECS,
    meter_metadata: METER_METADATA,
    supports_repeater_settings: false,
    documents_power_commands: false,
    dialect: LegacyCatDialect::Ft1000,
};

pub const FT1000D_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft1000D,
    frequency_ranges: FREQUENCY_RANGES,
    baud_rates: BAUD_RATES,
    writable_modes: MODES,
    controls: CONTROLS,
    readable_controls: READABLE_CONTROLS,
    writable_controls: WRITABLE_CONTROLS,
    meters: &[],
    meter_poll_specs: METER_POLL_SPECS,
    meter_metadata: METER_METADATA,
    supports_repeater_settings: false,
    documents_power_commands: false,
    dialect: LegacyCatDialect::Ft1000,
};

pub fn profile(model: YaesuLegacyModel) -> &'static RadioModelProfile {
    find_model(model.model_name()).expect("built-in FT-1000 family profile")
}

pub(crate) const fn read_frequency_mode() -> [u8; 5] {
    [0, 0, 0, 0, 0x10]
}

pub(crate) const fn read_tx_status() -> [u8; 5] {
    [0, 0, 0, 0, 0xFA]
}

pub(crate) const fn set_split(enabled: bool) -> [u8; 5] {
    [if enabled { 1 } else { 0 }, 0, 0, 0, 0x01]
}

pub(crate) const fn set_ptt(enabled: bool) -> [u8; 5] {
    [if enabled { 1 } else { 0 }, 0, 0, 0, 0x0F]
}

pub(crate) fn set_frequency(hz: u64) -> Result<[u8; 5]> {
    anyhow::ensure!(hz <= 30_000_000, "FT-1000 frequency exceeds CAT range");
    anyhow::ensure!(
        hz.is_multiple_of(10),
        "FT-1000 frequency must be aligned to 10 Hz"
    );
    let mut value = hz / 10;
    let mut frame = [0_u8; 5];
    for byte in frame[..4].iter_mut() {
        let pair = value % 100;
        *byte = ((pair / 10) as u8) << 4 | (pair % 10) as u8;
        value /= 100;
    }
    anyhow::ensure!(value == 0, "FT-1000 frequency does not fit CAT format");
    frame[4] = 0x0A;
    Ok(frame)
}

pub(crate) const fn set_mode(mode: LegacyMode) -> [u8; 5] {
    let value = match mode {
        LegacyMode::Lsb => 0x00,
        LegacyMode::Usb => 0x01,
        LegacyMode::Cw => 0x02,
        LegacyMode::CwReverse => 0x03,
        LegacyMode::Am => 0x04,
        LegacyMode::Fm => 0x06,
        LegacyMode::Digital => 0x08,
        LegacyMode::Packet => 0x0A,
        LegacyMode::Wfm | LegacyMode::FmNarrow | LegacyMode::CwNarrow => 0x00,
    };
    [value, 0, 0, 0, 0x0C]
}

pub(crate) fn decode_frequency_mode(response: &[u8]) -> Result<FrequencyModeStatus> {
    anyhow::ensure!(
        response.len() == 1_636,
        "FT-1000 status update must contain 1636 bytes"
    );
    let record = &response[4..20];
    let value = u32::from_be_bytes([0, record[1], record[2], record[3]]);
    anyhow::ensure!(
        (10_000..=3_000_000).contains(&value),
        "invalid FT-1000 frequency record"
    );
    let mode = match record[7] {
        0 => LegacyMode::Lsb,
        1 => LegacyMode::Usb,
        2 => LegacyMode::Cw,
        3 => LegacyMode::Am,
        4 => LegacyMode::Fm,
        5 => LegacyMode::Digital,
        6 => LegacyMode::Packet,
        other => bail!("unknown FT-1000 status mode {other:#04x}"),
    };
    Ok(FrequencyModeStatus {
        frequency_hz: u64::from(value) * 10,
        mode,
    })
}

pub(crate) fn decode_tx_status(response: &[u8]) -> Result<TxStatus> {
    anyhow::ensure!(
        response.len() == 5,
        "FT-1000 flags response must contain five bytes"
    );
    Ok(TxStatus {
        power_meter: 0,
        split_enabled: response[0] & 0x01 != 0,
        high_swr: false,
        transmitting: response[0] & 0x80 != 0 || response[2] & 0x01 != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_separate_and_use_ft1000_dialect() {
        assert_eq!(FT1000_PROFILE.model, YaesuLegacyModel::Ft1000);
        assert_eq!(FT1000D_PROFILE.model, YaesuLegacyModel::Ft1000D);
        assert_eq!(FT1000_PROFILE.baud_rates, &[4_800]);
        assert_eq!(FT1000_PROFILE.dialect, LegacyCatDialect::Ft1000);
    }

    #[test]
    fn encodes_documented_five_byte_commands() {
        assert_eq!(
            set_frequency(14_250_000).unwrap(),
            [0x00, 0x50, 0x42, 0x01, 0x0A]
        );
        assert_eq!(set_mode(LegacyMode::Usb), [0x01, 0, 0, 0, 0x0C]);
        assert_eq!(set_ptt(true), [0x01, 0, 0, 0, 0x0F]);
    }

    #[test]
    fn decodes_documented_update_record() {
        let mut response = vec![0_u8; 1_636];
        response[5..8].copy_from_slice(&[0x15, 0xBE, 0x68]);
        response[11] = 1;
        let status = decode_frequency_mode(&response).unwrap();
        assert_eq!(status.frequency_hz, 14_250_000);
        assert_eq!(status.mode, LegacyMode::Usb);
    }

    #[test]
    fn decodes_flags_for_cat_ptt_and_split() {
        let status = decode_tx_status(&[0x81, 0, 0, 0, 0]).unwrap();
        assert!(status.transmitting);
        assert!(status.split_enabled);
        assert_eq!(status.power_meter, 0);
    }
}
