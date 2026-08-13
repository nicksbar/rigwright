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
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrequencyModeStatus {
    pub frequency_hz: u64,
    pub mode: LegacyMode,
}

pub const fn read_frequency_and_mode() -> [u8; 5] {
    [0, 0, 0, 0, 0x03]
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

pub const fn set_split(enabled: bool) -> [u8; 5] {
    [0, 0, 0, 0, if enabled { 0x02 } else { 0x82 }]
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
        assert_eq!(set_split(true), [0, 0, 0, 0, 0x02]);
        assert_eq!(set_split(false), [0, 0, 0, 0, 0x82]);
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
}
