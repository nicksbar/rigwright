//! Yaesu FTDX101MP model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::FTDX101MP_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX101MP").expect("built-in FTDX101MP profile")
}
