//! Yaesu FT-857D profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};
pub use crate::protocol::yaesu_legacy_cat::{
    decode_frequency_and_mode, read_frequency_and_mode, set_frequency, set_mode, set_ptt,
    set_split, FrequencyModeStatus, LegacyMode,
};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-857D").expect("built-in FT-857D profile")
}
