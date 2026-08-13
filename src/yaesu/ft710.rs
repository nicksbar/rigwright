//! Yaesu FT-710 model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-710").expect("built-in FT-710 profile")
}
