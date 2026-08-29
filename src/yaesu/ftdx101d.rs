//! Yaesu FTDX101D model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::FTDX101D_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX101D").expect("built-in FTDX101D profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ftdx101d_catalog_profile() {
        assert_eq!(profile().model, "FTDX101D");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuCatModel::Ftdx101D);
    }
}
