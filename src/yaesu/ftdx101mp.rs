//! Yaesu FTDX101MP model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::FTDX101MP_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX101MP").expect("built-in FTDX101MP profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ftdx101mp_catalog_profile() {
        assert_eq!(profile().model, "FTDX101MP");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuCatModel::Ftdx101Mp);
    }
}
