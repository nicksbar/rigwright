//! Yaesu FT-857D profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::legacy_profile::FT857D_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-857D").expect("built-in FT-857D profile")
}
