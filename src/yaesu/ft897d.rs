//! Yaesu FT-897D profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::legacy_profile::FT897D_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-897D").expect("built-in FT-897D profile")
}
