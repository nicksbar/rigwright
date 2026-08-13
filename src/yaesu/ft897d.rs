//! Yaesu FT-897D profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-897D").expect("built-in FT-897D profile")
}
