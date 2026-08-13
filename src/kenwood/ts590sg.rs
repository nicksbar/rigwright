//! Kenwood TS-590SG model profile (framework only; validation pending).

use crate::models::{find_model, RadioModelProfile};

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-590SG").expect("built-in TS-590SG profile")
}
