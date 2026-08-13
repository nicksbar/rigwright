//! Yaesu FTDX101MP model profile (framework only; hardware validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FTDX101MP").expect("built-in FTDX101MP profile")
}
