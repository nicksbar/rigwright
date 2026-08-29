//! Yaesu FT-897D profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::legacy_profile::FT897D_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-897D").expect("built-in FT-897D profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft897d_catalog_profile() {
        assert_eq!(profile().model, "FT-897D");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuLegacyModel::Ft897D);
    }
}
