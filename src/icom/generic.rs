//! Conservative profile for an unidentified Icom CI-V radio.
//!
//! This profile intentionally exposes only the protocol intersection that is
//! safe to use without knowing the radio model. Model-specific controls,
//! ranges, memory, repeater, scope, and advanced meters remain unavailable
//! until a concrete profile is selected.

use super::profile::{ControlCapabilities, IcomCivProfile, MemoryLayout};
use crate::hal_types::MeterId;

const FREQUENCY_RANGES: &[(u64, u64)] = &[(1_000, 9_999_999_999)];
const METERS: &[MeterId] = &[MeterId::Signal];

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Generic,
    baud_rates: super::profile::DEFAULT_BAUD_RATES,
    usb_baud_rates: super::profile::DEFAULT_BAUD_RATES,
    supports_auto_baud: false,
    preferred_baud_rate: 9_600,
    default_address: 0xE0,
    frequency_ranges: FREQUENCY_RANGES,
    controls: &[],
    modes: super::profile::DEFAULT_MODES,
    scope_geometry: None,
    scope: None,
    scope_options: super::profile::EMPTY_SCOPE_OPTIONS,
    main_sub: None,
    external_preamp: None,
    attenuator_values: &[],
    preamp_max_level: 0,
    agc_max: 0,
    noise_reduction_level_max: 0,
    supports_iq_output: false,
    meters: METERS,
    meter_poll_specs: &[],
    control_capabilities: ControlCapabilities {
        supports_data_mode: false,
        filter_values: &[],
        supports_vfo: false,
        vfo_readable: false,
    },
    memory_layout: MemoryLayout::Hf,
    supports_repeater_settings: false,
    supports_memory_channels: false,
    filter_bandwidths: &[],
    swr_sweep_setup: None,
    meter_presentation: None,
    scope_ack_optional: false,
    usb_detection: &[],
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_profile_exposes_only_safe_intersection() {
        assert!(CIV_PROFILE.supports_frequency(14_074_000));
        assert!(CIV_PROFILE.supports_meter(MeterId::Signal));
        assert!(!CIV_PROFILE.supports_meter(MeterId::Power));
        assert!(!CIV_PROFILE.supports_control(crate::controls::ControlId::RfPower));
        assert_eq!(
            CIV_PROFILE.model.model_name(),
            crate::models::GENERIC_ICOM_MODEL
        );
    }
}
