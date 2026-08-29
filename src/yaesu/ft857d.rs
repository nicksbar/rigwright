//! Yaesu FT-857D profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::legacy_profile::FT857D_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-857D").expect("built-in FT-857D profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft857d_catalog_profile() {
        assert_eq!(profile().model, "FT-857D");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuLegacyModel::Ft857D);
    }
}
