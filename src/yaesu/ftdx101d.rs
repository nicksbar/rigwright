//! Yaesu FTDX101D model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::FTDX101D_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX101D").expect("built-in FTDX101D profile")
}
