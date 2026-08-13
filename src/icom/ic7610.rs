//! Icom IC-7610 model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("IC-7610").expect("built-in IC-7610 profile")
}
