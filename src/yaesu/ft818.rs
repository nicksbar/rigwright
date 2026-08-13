//! Yaesu FT-818 profile using five-byte binary CAT (validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-818").expect("built-in FT-818 profile")
}
