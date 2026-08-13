//! Yaesu FTDX10 model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX10").expect("built-in FTDX10 profile")
}
