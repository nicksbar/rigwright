//! Yaesu FT-710 model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::FT710_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-710").expect("built-in FT-710 profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft710_catalog_profile() {
        assert_eq!(profile().model, "FT-710");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuCatModel::Ft710);
    }
}
