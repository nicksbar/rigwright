//! Yaesu radio support.
//!
//! Modern radios use [`YaesuCatRadio`] plus declarative model profiles. The
//! older five-byte binary CAT family remains a separate protocol backend.

pub mod cat_radio;
pub mod ft710;
pub mod ft817nd;
pub mod ft818;
pub mod ft857d;
pub mod ft897d;
pub mod ft991a;
pub mod ftdx10;
pub mod ftdx101d;
pub mod ftdx101mp;
pub mod legacy_profile;
pub mod legacy_radio;
pub mod profile;

pub use cat_radio::YaesuCatRadio;
pub use legacy_radio::LegacyYaesuRadio;
