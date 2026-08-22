//! Kenwood TS-890S model profile (framework only; validation pending).

use crate::models::{find_model, RadioModelProfile};

pub use super::profile::TS890S_PROFILE as CAT_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-890S").expect("built-in TS-890S profile")
}
