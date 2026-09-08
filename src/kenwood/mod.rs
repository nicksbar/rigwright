//! Kenwood PC-control support.
//!
//! [`KenwoodCatRadio`] owns the common persistent transport. Declarative
//! profiles describe command-family differences between individual models.

use anyhow::Result;

use crate::protocol::ascii_cat;

pub mod cat_radio;
pub mod generic;
pub mod profile;
pub mod ts2000;
pub mod ts590sg;
pub mod ts890s;

pub use cat_radio::KenwoodCatRadio;

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

    #[test]
    fn common_commands_encode_reads_writes_and_ptt_edges() {
        assert_eq!(read_frequency_a().unwrap(), b"FA;");
        assert_eq!(read_mode().unwrap(), b"MD;");
        assert_eq!(set_ptt(true).unwrap(), b"TX;");
        assert_eq!(set_ptt(false).unwrap(), b"RX;");
        assert_eq!(set_frequency_a(0).unwrap(), b"FA00000000000;");
    }
}
