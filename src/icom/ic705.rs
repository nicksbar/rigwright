//! Icom IC-705 model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-705").expect("built-in IC-705 profile")
}
