//! Five-byte binary CAT framing used by legacy Yaesu all-mode radios.

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyMode {
    Lsb,
    Usb,
    Cw,
    CwReverse,
    Am,
    Fm,
    Digital,
    Packet,
    /// Receive-status code documented for broadcast FM.
    Wfm,
    /// Narrow FM, documented by FT-857D/FT-897D.
    FmNarrow,
    /// Narrow CW receive-status code documented by FT-857D.
    CwNarrow,
}

impl LegacyMode {
    const fn code(self) -> u8 {
        match self {
            Self::Lsb => 0x00,
            Self::Usb => 0x01,
            Self::Cw => 0x02,
            Self::CwReverse => 0x03,
            Self::Am => 0x04,
            Self::Fm => 0x08,
            Self::Digital => 0x0A,
            Self::Packet => 0x0C,
            Self::Wfm => 0x06,
            Self::FmNarrow => 0x88,
            Self::CwNarrow => 0x82,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0x00 => Self::Lsb,
            0x01 => Self::Usb,
            0x02 => Self::Cw,
            0x03 => Self::CwReverse,
            0x04 => Self::Am,
            0x08 => Self::Fm,
            0x0A => Self::Digital,
            0x0C => Self::Packet,
            0x06 => Self::Wfm,
            0x88 => Self::FmNarrow,
            0x82 => Self::CwNarrow,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrequencyModeStatus {
    pub frequency_hz: u64,
    pub mode: LegacyMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxStatus {
    /// Four-bit S-meter value reported by the radio.
    pub s_meter: u8,
    pub discriminator_centered: bool,
    pub tone_code_matched: bool,
    pub squelch_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxStatus {
    /// Four-bit power-meter value reported by the radio.
    pub power_meter: u8,
    pub split_enabled: bool,
    pub high_swr: bool,
    pub transmitting: bool,
}

pub const fn read_frequency_and_mode() -> [u8; 5] {
    [0, 0, 0, 0, 0x03]
}

pub const fn read_rx_status() -> [u8; 5] {
    [0, 0, 0, 0, 0xE7]
}

pub const fn read_tx_status() -> [u8; 5] {
    [0, 0, 0, 0, 0xF7]
}

pub fn set_frequency(hz: u64) -> Result<[u8; 5]> {
    if hz > 999_999_990 {
        bail!("frequency exceeds the eight-digit legacy CAT field");
    }
    if !hz.is_multiple_of(10) {
        bail!("legacy Yaesu CAT frequency must be aligned to 10 Hz");
    }

    let units = hz / 10;
    let mut frame = [0_u8; 5];
    for (index, byte) in frame[..4].iter_mut().enumerate() {
        let divisor = 10_u64.pow(6 - (index as u32 * 2));
        let pair = (units / divisor) % 100;
        *byte = ((pair / 10) as u8) << 4 | (pair % 10) as u8;
    }
    frame[4] = 0x01;
    Ok(frame)
}

pub const fn set_mode(mode: LegacyMode) -> [u8; 5] {
    [mode.code(), 0, 0, 0, 0x07]
}

pub const fn set_ptt(enabled: bool) -> [u8; 5] {
    [0, 0, 0, 0, if enabled { 0x08 } else { 0x88 }]
}

pub const fn set_lock(enabled: bool) -> [u8; 5] {
    [0, 0, 0, 0, if enabled { 0x00 } else { 0x80 }]
}

pub const fn toggle_vfo() -> [u8; 5] {
    [0, 0, 0, 0, 0x81]
}

pub const fn set_split(enabled: bool) -> [u8; 5] {
    [0, 0, 0, 0, if enabled { 0x02 } else { 0x82 }]
}

pub const fn set_clarifier(enabled: bool) -> [u8; 5] {
    [0, 0, 0, 0, if enabled { 0x05 } else { 0x85 }]
}

/// Set the classic Yaesu clarifier offset. The CAT field is signed and has
/// 10-Hz resolution, represented as four decimal BCD digits in P3/P4.
pub fn set_clarifier_offset(offset_hz: i32) -> Result<[u8; 5]> {
    if offset_hz % 10 != 0 || !(-99_990..=99_990).contains(&offset_hz) {
        bail!("classic Yaesu clarifier offset must be aligned to 10 Hz and fit ±99.99 kHz");
    }
    let magnitude = (offset_hz.unsigned_abs() / 10) as u16;
    let [p3, p4] = bcd_pair(magnitude)?;
    Ok([if offset_hz >= 0 { 0 } else { 1 }, 0, p3, p4, 0xF5])
}

pub const fn set_repeater_shift(shift: crate::hal_types::RepeaterShift) -> [u8; 5] {
    let value = match shift {
        crate::hal_types::RepeaterShift::Minus => 0x09,
        crate::hal_types::RepeaterShift::Plus => 0x49,
        crate::hal_types::RepeaterShift::Simplex => 0x89,
    };
    [value, 0, 0, 0, 0x09]
}

pub fn set_repeater_offset_frequency(offset_hz: u32) -> Result<[u8; 5]> {
    if offset_hz > 99_999_999 {
        bail!("classic Yaesu repeater offset must fit the CAT frequency field");
    }
    // Unlike the VFO-frequency command, the manual's repeater example uses
    // all eight BCD digits directly: 05 43 21 00 = 5.4321 MHz.
    let units = offset_hz;
    let mut frame = [0_u8; 5];
    for (index, byte) in frame[..4].iter_mut().enumerate() {
        let divisor = 10_u32.pow(6 - (index as u32 * 2));
        let pair = (units / divisor) % 100;
        *byte = ((pair / 10) as u8) << 4 | (pair % 10) as u8;
    }
    frame[4] = 0xF9;
    Ok(frame)
}

pub fn set_ctcss_tones(tx_tenths_hz: u32, rx_tenths_hz: u32) -> Result<[u8; 5]> {
    if tx_tenths_hz > 9999 || rx_tenths_hz > 9999 {
        bail!("classic Yaesu CTCSS tone must fit four decimal digits");
    }
    Ok([
        bcd_byte((tx_tenths_hz / 100) as u16)?,
        bcd_byte((tx_tenths_hz % 100) as u16)?,
        bcd_byte((rx_tenths_hz / 100) as u16)?,
        bcd_byte((rx_tenths_hz % 100) as u16)?,
        0x0B,
    ])
}

pub fn set_dcs_codes(tx_code: u16, rx_code: u16) -> Result<[u8; 5]> {
    if tx_code > 999 || rx_code > 999 {
        bail!("classic Yaesu DCS code must fit three decimal digits");
    }
    Ok([
        bcd_byte(tx_code / 100)?,
        bcd_byte(tx_code % 100)?,
        bcd_byte(rx_code / 100)?,
        bcd_byte(rx_code % 100)?,
        0x0C,
    ])
}

pub const fn set_ctcss_dcs_mode(mode: u8) -> [u8; 5] {
    [mode, 0, 0, 0, 0x0A]
}

fn bcd_pair(value: u16) -> Result<[u8; 2]> {
    if value > 9999 {
        bail!("value exceeds four decimal BCD digits");
    }
    Ok([bcd_byte(value / 100)?, bcd_byte(value % 100)?])
}

fn bcd_byte(value: u16) -> Result<u8> {
    if value > 99 {
        bail!("value exceeds two decimal BCD digits");
    }
    Ok(((value / 10) as u8) << 4 | (value % 10) as u8)
}

pub fn decode_frequency_and_mode(response: &[u8]) -> Result<FrequencyModeStatus> {
    if response.len() != 5 {
        bail!("legacy Yaesu frequency/mode response must contain five bytes");
    }

    let mut units = 0_u64;
    for byte in &response[..4] {
        let high = byte >> 4;
        let low = byte & 0x0F;
        if high > 9 || low > 9 {
            bail!("legacy Yaesu response contains invalid BCD");
        }
        units = units * 100 + u64::from(high) * 10 + u64::from(low);
    }

    let mode = LegacyMode::from_code(response[4])
        .ok_or_else(|| anyhow::anyhow!("unknown legacy Yaesu mode: {:#04x}", response[4]))?;
    Ok(FrequencyModeStatus {
        frequency_hz: units * 10,
        mode,
    })
}

pub fn decode_rx_status(response: &[u8]) -> Result<RxStatus> {
    let value = one_byte_status(response, "RX")?;
    Ok(RxStatus {
        s_meter: value & 0x0F,
        discriminator_centered: value & 0x20 == 0,
        tone_code_matched: value & 0x40 == 0,
        squelch_open: value & 0x80 == 0,
    })
}

pub fn decode_tx_status(response: &[u8]) -> Result<TxStatus> {
    let value = one_byte_status(response, "TX")?;
    Ok(TxStatus {
        power_meter: value & 0x0F,
        split_enabled: value & 0x20 == 0,
        high_swr: value & 0x40 != 0,
        transmitting: value & 0x80 == 0,
    })
}

fn one_byte_status(response: &[u8], name: &str) -> Result<u8> {
    if response.len() != 1 {
        bail!("legacy Yaesu {name} status response must contain one byte");
    }
    Ok(response[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ft857d_frequency_frame() {
        assert_eq!(
            set_frequency(14_074_000).unwrap(),
            [0x01, 0x40, 0x74, 0x00, 0x01]
        );
    }

    #[test]
    fn builds_mode_ptt_and_split_frames() {
        assert_eq!(set_mode(LegacyMode::Digital), [0x0A, 0, 0, 0, 0x07]);
        assert_eq!(set_ptt(true), [0, 0, 0, 0, 0x08]);
        assert_eq!(set_ptt(false), [0, 0, 0, 0, 0x88]);
        assert_eq!(set_lock(true), [0, 0, 0, 0, 0x00]);
        assert_eq!(set_lock(false), [0, 0, 0, 0, 0x80]);
        assert_eq!(toggle_vfo(), [0, 0, 0, 0, 0x81]);
        assert_eq!(set_split(true), [0, 0, 0, 0, 0x02]);
        assert_eq!(set_split(false), [0, 0, 0, 0, 0x82]);
        assert_eq!(read_rx_status(), [0, 0, 0, 0, 0xE7]);
        assert_eq!(read_tx_status(), [0, 0, 0, 0, 0xF7]);
    }

    #[test]
    fn builds_documented_clarifier_and_repeater_frames() {
        assert_eq!(set_clarifier(true), [0, 0, 0, 0, 0x05]);
        assert_eq!(set_clarifier(false), [0, 0, 0, 0, 0x85]);
        assert_eq!(
            set_clarifier_offset(12_340).unwrap(),
            [0, 0, 0x12, 0x34, 0xF5]
        );
        assert_eq!(
            set_clarifier_offset(-12_340).unwrap(),
            [1, 0, 0x12, 0x34, 0xF5]
        );
        assert_eq!(
            set_repeater_shift(crate::hal_types::RepeaterShift::Plus),
            [0x49, 0, 0, 0, 0x09]
        );
        assert_eq!(
            set_repeater_offset_frequency(5_432_100).unwrap(),
            [0x05, 0x43, 0x21, 0x00, 0xF9]
        );
        assert_eq!(
            set_ctcss_tones(885, 1000).unwrap(),
            [0x08, 0x85, 0x10, 0x00, 0x0B]
        );
        assert_eq!(
            set_dcs_codes(23, 371).unwrap(),
            [0x00, 0x23, 0x03, 0x71, 0x0C]
        );
    }

    #[test]
    fn decodes_frequency_and_mode_response() {
        let status = decode_frequency_and_mode(&[0x01, 0x40, 0x74, 0x00, 0x0A]).unwrap();
        assert_eq!(status.frequency_hz, 14_074_000);
        assert_eq!(status.mode, LegacyMode::Digital);
    }

    #[test]
    fn rejects_lossy_frequency_and_invalid_bcd() {
        assert!(set_frequency(14_074_005).is_err());
        assert!(decode_frequency_and_mode(&[0x01, 0x4A, 0x74, 0x00, 0x01]).is_err());
    }

    #[test]
    fn decodes_documented_rx_and_tx_status_bits() {
        let rx = decode_rx_status(&[0b0000_1011]).unwrap();
        assert_eq!(rx.s_meter, 11);
        assert!(rx.discriminator_centered);
        assert!(rx.tone_code_matched);
        assert!(rx.squelch_open);

        let tx = decode_tx_status(&[0b0100_0110]).unwrap();
        assert_eq!(tx.power_meter, 6);
        assert!(tx.split_enabled);
        assert!(tx.high_swr);
        assert!(tx.transmitting);

        let idle_split_off = decode_tx_status(&[0b1010_0000]).unwrap();
        assert!(!idle_split_off.transmitting);
        assert!(!idle_split_off.split_enabled);
        assert!(decode_tx_status(&[]).is_err());
    }

    #[test]
    fn decodes_read_only_and_narrow_mode_codes() {
        assert_eq!(
            decode_frequency_and_mode(&[0, 0, 0, 0, 0x06]).unwrap().mode,
            LegacyMode::Wfm
        );
        assert_eq!(
            decode_frequency_and_mode(&[0, 0, 0, 0, 0x88]).unwrap().mode,
            LegacyMode::FmNarrow
        );
    }
}
