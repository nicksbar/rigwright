//! Yaesu FT-817ND profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-817ND").expect("built-in FT-817ND profile")
}
