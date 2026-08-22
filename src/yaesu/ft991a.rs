//! Yaesu FT-991A model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::FT991A_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-991A").expect("built-in FT-991A profile")
}
