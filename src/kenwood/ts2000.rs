//! Kenwood TS-2000 model profile (framework only; validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-2000").expect("built-in TS-2000 profile")
}
