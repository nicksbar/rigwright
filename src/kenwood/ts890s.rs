//! Kenwood TS-890S model profile (framework only; validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-890S").expect("built-in TS-890S profile")
}
