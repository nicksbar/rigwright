//! Icom IC-9700 model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-9700").expect("built-in IC-9700 profile")
}
