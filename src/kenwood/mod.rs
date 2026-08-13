//! Kenwood PC-control framing and common commands.

use anyhow::Result;

use crate::protocol::ascii_cat;

pub mod ts2000;
pub mod ts590sg;
pub mod ts890s;

pub fn read_frequency_a() -> Result<Vec<u8>> {
    ascii_cat::encode("FA", None)
}
pub fn set_frequency_a(hz: u64) -> Result<Vec<u8>> {
    ascii_cat::encode("FA", Some(&format!("{hz:011}")))
}
pub fn read_mode() -> Result<Vec<u8>> {
    ascii_cat::encode("MD", None)
}
pub fn set_ptt(enabled: bool) -> Result<Vec<u8>> {
    ascii_cat::encode(if enabled { "TX" } else { "RX" }, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts590_frequency_has_eleven_digits() {
        assert_eq!(set_frequency_a(14_074_000).unwrap(), b"FA00014074000;");
    }
}
