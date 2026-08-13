//! Yaesu CAT framing and common commands.

use anyhow::Result;

use crate::protocol::ascii_cat;

pub mod ft710;
pub mod ft817nd;
pub mod ft818;
pub mod ft857d;
pub mod ft897d;
pub mod ft991a;
pub mod ftdx10;
pub mod ftdx101d;
pub mod ftdx101mp;

pub fn read_frequency_a() -> Result<Vec<u8>> {
    ascii_cat::encode("FA", None)
}
pub fn set_frequency_a(hz: u64) -> Result<Vec<u8>> {
    ascii_cat::encode("FA", Some(&format!("{hz:09}")))
}
pub fn read_mode() -> Result<Vec<u8>> {
    ascii_cat::encode("MD", None)
}
pub fn set_ptt(enabled: bool) -> Result<Vec<u8>> {
    ascii_cat::encode("TX", Some(if enabled { "1" } else { "0" }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ft991a_manual_frequency_example() {
        assert_eq!(set_frequency_a(14_250_000).unwrap(), b"FA014250000;");
    }
}
