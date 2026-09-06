//! Yaesu FT-857D profile using five-byte binary CAT (validation pending).

use super::legacy_profile::YaesuLegacyProfile;
use crate::models::{find_model, RadioModelProfile, YaesuLegacyModel};

const RANGES: &[(u64, u64)] = &[
    (100_000, 56_000_000),
    (76_000_000, 108_000_000),
    (118_000_000, 164_000_000),
    (420_000_000, 470_000_000),
];

pub const CAT_PROFILE: YaesuLegacyProfile = YaesuLegacyProfile {
    model: YaesuLegacyModel::Ft857D,
    frequency_ranges: RANGES,
    baud_rates: super::legacy_profile::BAUD_RATES,
    writable_modes: super::legacy_profile::MOBILE_MODES,
    controls: super::legacy_profile::CONTROLS,
    readable_controls: super::legacy_profile::READABLE_CONTROLS,
    writable_controls: super::legacy_profile::WRITABLE_CONTROLS,
    meters: super::legacy_profile::METERS,
    meter_poll_specs: super::legacy_profile::METER_POLL_SPECS,
    meter_metadata: super::legacy_profile::METER_METADATA,
    supports_repeater_settings: true,
    documents_power_commands: false,
};

pub fn profile() -> &'static RadioModelProfile {
    find_model("FT-857D").expect("built-in FT-857D profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ft857d_catalog_profile() {
        assert_eq!(profile().model, "FT-857D");
        assert_eq!(CAT_PROFILE.model, crate::models::YaesuLegacyModel::Ft857D);
    }
}
