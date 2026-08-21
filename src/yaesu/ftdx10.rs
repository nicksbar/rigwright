//! Yaesu FTDX10 CAT profile and command helpers.
//!
//! The FTDX10 uses Yaesu's semicolon-terminated ASCII CAT protocol. The
//! model-specific commands below deliberately remain pure builders/parsers so
//! applications can use them without depending on a particular serial
//! transport.

use anyhow::{bail, Context, Result};

use crate::models::{find_model, RadioModelProfile};

pub const MIN_FREQUENCY_HZ: u64 = 30_000;
pub const MAX_FREQUENCY_HZ: u64 = 75_000_000;

/// Build a documented FTDX10 CAT command. This is the escape hatch for the
/// complete model command table, including commands not yet represented by a
/// protocol-neutral `RadioHal` control.
pub fn command(command: &str, parameters: Option<&str>) -> Result<Vec<u8>> {
    if command.len() != 2 || !command.bytes().all(|b| b.is_ascii_alphabetic()) {
        bail!("FTDX10 CAT command must contain exactly two letters");
    }
    let command = command.to_ascii_uppercase();
    let parameters = parameters.unwrap_or_default();
    if parameters.bytes().any(|b| b == b';' || b < 0x20) {
        bail!("FTDX10 CAT parameters contain an invalid character");
    }
    Ok(format!("{command}{parameters};").into_bytes())
}

pub fn read(command_name: &str) -> Result<Vec<u8>> {
    command(command_name, None)
}

pub fn parse_response<'a>(response: &'a [u8], command_name: &str) -> Result<&'a str> {
    let text = std::str::from_utf8(response).context("FTDX10 CAT response is not ASCII")?;
    let command_name = command_name.to_ascii_uppercase();
    text.strip_prefix(&command_name)
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected FTDX10 CAT response")
}

pub fn set_vfo_b_frequency(hz: u64) -> Result<Vec<u8>> {
    if !(MIN_FREQUENCY_HZ..=MAX_FREQUENCY_HZ).contains(&hz) {
        bail!("FTDX10 frequency is outside the CAT range: {hz} Hz");
    }
    command("FB", Some(&format!("{hz:09}")))
}

pub fn read_vfo_b_frequency() -> Result<Vec<u8>> {
    read("FB")
}

pub fn read_information() -> Result<Vec<u8>> {
    read("IF")
}

pub fn read_s_meter() -> Result<Vec<u8>> {
    read("SM")
}

pub fn read_received_meter(meter: u8) -> Result<Vec<u8>> {
    command("RM", Some(&meter.to_string()))
}

pub fn set_filter_width(width: u16) -> Result<Vec<u8>> {
    if width > 99 {
        bail!("FTDX10 filter width must be 0..=99");
    }
    command("SH", Some(&format!("000{width:02}")))
}

pub fn set_agc(function: u8) -> Result<Vec<u8>> {
    if function > 3 {
        bail!("FTDX10 AGC function must be 0..=3");
    }
    command("GT", Some(&format!("0{function}000")))
}

pub fn set_noise_reduction(enabled: bool, level: u8) -> Result<Vec<u8>> {
    if level > 15 {
        bail!("FTDX10 noise-reduction level must be 0..=15");
    }
    command(
        "NR",
        Some(&format!("{}{:02}", if enabled { 1 } else { 0 }, level)),
    )
}

pub fn set_frequency_a(hz: u64) -> Result<Vec<u8>> {
    if !(MIN_FREQUENCY_HZ..=MAX_FREQUENCY_HZ).contains(&hz) {
        bail!("FTDX10 frequency is outside the CAT range: {hz} Hz");
    }
    Ok(format!("FA{hz:09};").into_bytes())
}

pub fn read_frequency_a() -> &'static [u8] {
    b"FA;"
}

pub fn parse_frequency_a(response: &[u8]) -> Result<u64> {
    let text = std::str::from_utf8(response).context("FTDX10 frequency response is not ASCII")?;
    let value = text
        .strip_prefix("FA")
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected FTDX10 FA response")?;
    value.parse().context("invalid FTDX10 frequency")
}

/// FTDX10 mode values.  The CAT mode command also carries a one-digit VFO
/// selector; this helper always targets the main (VFO-A) band.
pub fn set_mode(mode: char) -> Result<Vec<u8>> {
    let mode = mode.to_ascii_uppercase();
    if !matches!(mode, '1'..='9' | 'A'..='F') {
        bail!("unsupported FTDX10 CAT mode: {mode}");
    }
    Ok(format!("MD0{mode};").into_bytes())
}

pub fn read_mode() -> &'static [u8] {
    b"MD0;"
}

pub fn parse_mode(response: &[u8]) -> Result<char> {
    let text = std::str::from_utf8(response).context("FTDX10 mode response is not ASCII")?;
    let value = text
        .strip_prefix("MD")
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected FTDX10 MD response")?;
    value
        .chars()
        .last()
        .filter(|mode| matches!(mode, '1'..='9' | 'A'..='F' | 'a'..='f'))
        .map(|mode| mode.to_ascii_uppercase())
        .context("invalid FTDX10 mode")
}

pub fn set_power_percent(percent: u8) -> Result<Vec<u8>> {
    if !(5..=100).contains(&percent) {
        bail!("FTDX10 CAT power must be between 5 and 100 percent");
    }
    Ok(format!("PC{percent:03};").into_bytes())
}

pub fn read_power_percent() -> &'static [u8] {
    b"PC;"
}

pub fn parse_power_percent(response: &[u8]) -> Result<u8> {
    parse_three_digit(response, "PC", "power")
}

pub fn set_split(enabled: bool) -> Vec<u8> {
    format!("ST{};", if enabled { 1 } else { 0 }).into_bytes()
}

pub fn read_split() -> &'static [u8] {
    b"ST;"
}

/// Spectrum-scope spans documented by the FTDX10 CAT manual.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeSpan {
    Khz1,
    Khz2,
    Khz5,
    Khz10,
    Khz20,
    Khz50,
    Khz100,
    Khz200,
    Khz500,
    Mhz1,
}

impl ScopeSpan {
    fn code(self) -> u8 {
        match self {
            Self::Khz1 => 0,
            Self::Khz2 => 1,
            Self::Khz5 => 2,
            Self::Khz10 => 3,
            Self::Khz20 => 4,
            Self::Khz50 => 5,
            Self::Khz100 => 6,
            Self::Khz200 => 7,
            Self::Khz500 => 8,
            Self::Mhz1 => 9,
        }
    }
}

/// Build an FTDX10 `SS` spectrum-scope configuration command.
///
/// The manual defines one command family with a fixed P1 field and a
/// sub-command in P2.  This covers the controls that can be represented
/// without radio-specific UI state. The CAT manual documents scope setup, but
/// not a waveform sample stream, so these helpers do not claim to retrieve
/// spectrum bins.
pub fn set_scope_speed(speed: u8) -> Result<Vec<u8>> {
    if speed > 4 {
        bail!("FTDX10 scope speed must be 0..=4");
    }
    Ok(format!("SS00{speed}0000;").into_bytes())
}

pub fn set_scope_peak(level: u8) -> Result<Vec<u8>> {
    if level > 4 {
        bail!("FTDX10 scope peak level must be 0..=4");
    }
    Ok(format!("SS01{level}0000;").into_bytes())
}

pub fn set_scope_marker(enabled: bool) -> Vec<u8> {
    format!("SS02{}0000;", if enabled { 1 } else { 0 }).into_bytes()
}

pub fn set_scope_color(color: char) -> Result<Vec<u8>> {
    let color = color.to_ascii_uppercase();
    if !matches!(color, '0'..='9' | 'A') {
        bail!("FTDX10 scope color must be 0..=9 or A");
    }
    Ok(format!("SS03{color}0000;").into_bytes())
}

pub fn set_scope_level_tenths_db(tenths_db: i16) -> Result<Vec<u8>> {
    if !(-300..=300).contains(&tenths_db) || tenths_db % 5 != 0 {
        bail!("FTDX10 scope level must be -30.0..=30.0 dB in 0.5 dB steps");
    }
    let sign = if tenths_db < 0 { '-' } else { '+' };
    Ok(format!("SS04{sign}{:04};", tenths_db.unsigned_abs()).into_bytes())
}

pub fn set_scope_span(span: ScopeSpan) -> Vec<u8> {
    format!("SS05{}0000;", span.code()).into_bytes()
}

pub fn set_scope_mode(mode: char) -> Result<Vec<u8>> {
    let mode = mode.to_ascii_uppercase();
    if !matches!(mode, '0'..='9' | 'A'..='B') {
        bail!("unsupported FTDX10 scope mode: {mode}");
    }
    Ok(format!("SS06{mode}0000;").into_bytes())
}

pub fn set_scope_fft(attenuation: u8, osc_level: u8, osc_time: u8) -> Result<Vec<u8>> {
    if attenuation > 2 || osc_level > 2 || osc_time > 5 {
        bail!("invalid FTDX10 AF-FFT/oscilloscope setting");
    }
    Ok(format!("SS07{attenuation}{osc_level}{osc_time}00;").into_bytes())
}

pub fn set_scope_hold(hold: bool) -> Vec<u8> {
    format!("SS08{}0000;", if hold { 1 } else { 0 }).into_bytes()
}

pub fn read_scope_setting(setting: u8) -> Result<Vec<u8>> {
    if setting > 8 {
        bail!("FTDX10 scope setting must be 0..=8");
    }
    Ok(format!("SS0{setting};").into_bytes())
}

pub fn parse_split(response: &[u8]) -> Result<bool> {
    let text = std::str::from_utf8(response).context("FTDX10 split response is not ASCII")?;
    let value = text
        .strip_prefix("ST")
        .and_then(|value| value.strip_suffix(';'))
        .and_then(|value| value.parse::<u8>().ok())
        .context("invalid FTDX10 split response")?;
    match value {
        0 => Ok(false),
        1 | 2 => Ok(true),
        _ => bail!("invalid FTDX10 split response value: {value}"),
    }
}

fn parse_three_digit(response: &[u8], command: &str, label: &str) -> Result<u8> {
    let text = std::str::from_utf8(response).context("FTDX10 CAT response is not ASCII")?;
    let value = text
        .strip_prefix(command)
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected FTDX10 CAT response")?;
    value
        .parse()
        .with_context(|| format!("invalid FTDX10 {label}"))
}

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX10").expect("built-in FTDX10 profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_parses_frequency() {
        assert_eq!(set_frequency_a(14_250_000).unwrap(), b"FA014250000;");
        assert_eq!(parse_frequency_a(b"FA014250000;").unwrap(), 14_250_000);
    }

    #[test]
    fn builds_and_parses_mode() {
        assert_eq!(set_mode('c').unwrap(), b"MD0C;");
        assert_eq!(parse_mode(b"MD0C;").unwrap(), 'C');
    }

    #[test]
    fn builds_and_parses_power_and_split() {
        assert_eq!(set_power_percent(50).unwrap(), b"PC050;");
        assert_eq!(parse_power_percent(b"PC050;").unwrap(), 50);
        assert_eq!(set_split(true), b"ST1;");
        assert!(parse_split(b"ST2;").unwrap());
        assert!(!parse_split(b"ST0;").unwrap());
    }

    #[test]
    fn builds_generic_and_scope_commands() {
        assert_eq!(command("pc", Some("050")).unwrap(), b"PC050;");
        assert_eq!(read("IF").unwrap(), b"IF;");
        assert_eq!(parse_response(b"PC050;", "pc").unwrap(), "050");
        assert_eq!(set_scope_speed(2).unwrap(), b"SS0020000;");
        assert_eq!(set_scope_span(ScopeSpan::Khz100), b"SS0560000;");
        assert_eq!(set_scope_hold(true), b"SS0810000;");
    }

    #[test]
    fn rejects_invalid_power_and_frequency() {
        assert!(set_power_percent(4).is_err());
        assert!(set_power_percent(101).is_err());
        assert!(set_frequency_a(MAX_FREQUENCY_HZ + 1).is_err());
    }
}
