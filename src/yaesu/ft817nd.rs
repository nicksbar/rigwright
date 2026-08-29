//! Yaesu FT-817ND profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::legacy_profile::FT817ND_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-817ND").expect("built-in FT-817ND profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft817nd_catalog_profile() {
        assert_eq!(profile().model, "FT-817ND");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuLegacyModel::Ft817Nd);
    }
}
