//! K4 notch-control framing and normalized position conversion.

use anyhow::{bail, Context, Result};

pub(crate) fn parse_enabled(response: &[u8], prefix: &str) -> Result<bool> {
    let text = std::str::from_utf8(response)?;
    let value = text
        .strip_prefix(prefix)
        .and_then(|value| value.strip_suffix(';').or(Some(value)))
        .context("unexpected Elecraft notch response")?;
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => bail!("invalid Elecraft notch enable state: {value}"),
    }
}

pub(crate) fn parse_manual(response: &[u8]) -> Result<(u16, bool)> {
    let text = std::str::from_utf8(response)?;
    let value = text
        .strip_prefix("NM$")
        .and_then(|value| value.strip_suffix(';').or(Some(value)))
        .context("unexpected Elecraft manual-notch response")?;
    anyhow::ensure!(value.len() == 5, "invalid Elecraft manual-notch response");
    let position = value[..4].parse::<u16>()?;
    anyhow::ensure!(
        (150..=5000).contains(&position),
        "Elecraft notch position out of range"
    );
    let enabled = match &value[4..] {
        "0" => false,
        "1" => true,
        _ => bail!("invalid Elecraft manual-notch enable state"),
    };
    Ok((position, enabled))
}

pub(crate) fn normalize_position(position: u16) -> u8 {
    ((u32::from(position.saturating_sub(150)) * 255) / 4850) as u8
}

pub(crate) fn denormalize_position(value: u8) -> u16 {
    (150 + (u32::from(value) * 4850 / 255) as u16).clamp(150, 5000)
}

pub(crate) fn encode_manual(position: u16, enabled: bool) -> String {
    format!("{position:04}{}", if enabled { 1 } else { 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_notch_round_trip_preserves_endpoints() {
        assert_eq!(parse_manual(b"NM$01501;").unwrap(), (150, true));
        assert_eq!(normalize_position(150), 0);
        assert_eq!(denormalize_position(255), 5000);
        assert_eq!(encode_manual(5000, false), "50000");
    }

    #[test]
    fn malformed_notch_frames_are_rejected() {
        assert!(parse_enabled(b"NA$2;", "NA$").is_err());
        assert!(parse_manual(b"NM$01401;").is_err());
        assert!(parse_manual(b"NM$50002;").is_err());
        assert!(parse_manual(b"NM$5000x;").is_err());
        assert!(parse_manual(b"NM$50000").is_ok());
    }
}
