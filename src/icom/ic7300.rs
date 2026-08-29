//! IC-7300-specific CI-V scope profile and payload builders.

use super::profile::{ControlEncoding, ControlSpec, IcomCivProfile, ScopeSpec};
use crate::controls::ControlId;
use anyhow::Result;

const FREQUENCY_RANGES: &[(u64, u64)] = &[(30_000, 74_800_000)];
const CONTROLS: &[ControlSpec] = &[
    ControlSpec {
        id: ControlId::Xit,
        command_prefix: &[0x21, 0x02],
        encoding: ControlEncoding::Bool,
    },
    ControlSpec {
        id: ControlId::NoiseReductionLevel,
        command_prefix: &[0x14, 0x06],
        encoding: ControlEncoding::Level255Bcd,
    },
    ControlSpec {
        id: ControlId::ManualNotchPosition,
        command_prefix: &[0x14, 0x0D],
        encoding: ControlEncoding::Level255Bcd,
    },
    // `0x16 0x12`: automatic gain control preset selection.
    ControlSpec {
        id: ControlId::Agc,
        command_prefix: &[0x16, 0x12],
        encoding: ControlEncoding::U8,
    },
];
/// IC-7300 documented attenuator settings, in dB.
const ATTENUATOR_VALUES: &[u8] = &[0, 20];
/// CI-V scope command family used by the IC-7300 scope stream.
const SCOPE: ScopeSpec = ScopeSpec {
    enable_command: &[0x27, 0x10, 0x01],
    stream_command: &[0x27, 0x11, 0x01],
    disable_stream_command: &[0x27, 0x11, 0x00],
};

pub const CIV_PROFILE: IcomCivProfile = IcomCivProfile {
    model: crate::models::IcomCivModel::Ic7300,
    default_address: 0x94,
    frequency_ranges: FREQUENCY_RANGES,
    controls: CONTROLS,
    scope_geometry: Some(crate::models::IcomScopeGeometry {
        divisions: 11,
        bins: 475,
        full_chunk_bins: 50,
        last_chunk_bins: 25,
        bin_max: 160,
        supports_main_sub_scope: false,
    }),
    scope: Some(SCOPE),
    main_sub: None,
    external_preamp: None,
    attenuator_values: ATTENUATOR_VALUES,
    preamp_max_level: 1,
    supports_iq_output: false,
};

/// IC-7300-specific scope configuration operations.
///
/// These are intentionally implemented outside the generic CI-V engine. The
/// generic driver exposes only profile-neutral scope streaming; applications
/// must opt into this model driver to use these documented controls.
impl super::civ_radio::IcomCiVRadio {
    pub async fn set_scope_sweep_speed(&self, speed: u8) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_sweep_speed_payload(speed)?)
    }

    pub async fn set_scope_hold(&self, hold: bool) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_hold_payload(hold))
    }

    pub async fn set_scope_reference_level_tenths_db(&self, tenths_db: i16) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_reference_level_payload(tenths_db)?)
    }

    pub async fn set_scope_center_fixed_mode(&self, fixed_mode: bool) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_mode_payload(fixed_mode))
    }

    pub async fn set_scope_fixed_edge_number(&self, edge_number: u8) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_edge_number_payload(edge_number)?)
    }

    pub async fn set_scope_fixed_edge_frequencies(
        &self,
        edge_number: u8,
        lower_hz: u64,
        upper_hz: u64,
    ) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_fixed_edges_payload(edge_number, lower_hz, upper_hz)?)
    }

    pub async fn set_scope_span_hz(&self, span_hz: u64) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_span_payload(span_hz)?)
    }

    pub async fn set_scope_vbw_wide(&self, wide: bool) -> Result<()> {
        self.require_model(crate::models::IcomCivModel::Ic7300)?;
        self.transact_ack(&scope_vbw_payload(wide))
    }
}

pub(crate) fn scope_mode_payload(fixed_mode: bool) -> [u8; 4] {
    [0x27, 0x14, 0x00, u8::from(fixed_mode)]
}
pub(crate) fn scope_edge_number_payload(edge_number: u8) -> Result<[u8; 4]> {
    anyhow::ensure!(
        (1..=4).contains(&edge_number),
        "IC-7300 scope edge number must be in 1..=4"
    );
    Ok([0x27, 0x16, 0x00, edge_number])
}
pub(crate) fn scope_sweep_speed_payload(speed: u8) -> Result<[u8; 4]> {
    anyhow::ensure!(speed <= 2, "IC-7300 scope sweep speed must be in 0..=2");
    Ok([0x27, 0x1A, 0x00, speed])
}
pub(crate) fn scope_hold_payload(hold: bool) -> [u8; 4] {
    [0x27, 0x17, 0x00, u8::from(hold)]
}

pub(crate) fn scope_reference_level_payload(tenths_db: i16) -> Result<[u8; 6]> {
    if !(-200..=200).contains(&tenths_db) || tenths_db % 5 != 0 {
        anyhow::bail!("IC-7300 scope reference level must be -20.0..=20.0 dB in 0.5 dB steps");
    }
    let magnitude = tenths_db.unsigned_abs();
    let whole_db = (magnitude / 10) as u8;
    let half_db = if magnitude % 10 == 5 { 0x50 } else { 0x00 };
    Ok([
        0x27,
        0x19,
        0x00,
        decimal_to_bcd(whole_db),
        half_db,
        u8::from(tenths_db < 0),
    ])
}

pub(crate) fn scope_vbw_payload(wide: bool) -> [u8; 4] {
    [0x27, 0x1D, 0x00, u8::from(wide)]
}

pub(crate) fn scope_span_payload(span_hz: u64) -> Result<Vec<u8>> {
    const ALLOWED: &[u64] = &[
        2_500, 5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000,
    ];
    if !ALLOWED.contains(&span_hz) {
        anyhow::bail!("unsupported IC-7300 center scope span: {span_hz} Hz");
    }
    let mut payload = vec![0x27, 0x15, 0x00];
    payload.extend_from_slice(&super::civ_radio::encode_civ_frequency_bcd(span_hz));
    Ok(payload)
}

pub(crate) fn scope_fixed_edges_payload(
    edge_number: u8,
    lower_hz: u64,
    upper_hz: u64,
) -> Result<Vec<u8>> {
    if lower_hz >= upper_hz {
        anyhow::bail!("scope lower edge must be below upper edge");
    }
    if !(1..=4).contains(&edge_number) {
        anyhow::bail!("IC-7300 scope edge number must be in 1..=4");
    }
    let range = scope_frequency_range(lower_hz, upper_hz)
        .ok_or_else(|| anyhow::anyhow!("scope edges do not fit one IC-7300 frequency range"))?;
    let mut payload = vec![0x27, 0x1E, decimal_to_bcd(range), edge_number];
    payload.extend_from_slice(&super::civ_radio::encode_civ_frequency_bcd(lower_hz));
    payload.extend_from_slice(&super::civ_radio::encode_civ_frequency_bcd(upper_hz));
    Ok(payload)
}

fn scope_frequency_range(lower_hz: u64, upper_hz: u64) -> Option<u8> {
    const RANGES: &[(u64, u64)] = &[
        (30_000, 1_600_000),
        (1_600_000, 2_000_000),
        (2_000_000, 6_000_000),
        (6_000_000, 8_000_000),
        (8_000_000, 11_000_000),
        (11_000_000, 15_000_000),
        (15_000_000, 20_000_000),
        (20_000_000, 22_000_000),
        (22_000_000, 26_000_000),
        (26_000_000, 30_000_000),
        (30_000_000, 45_000_000),
        (45_000_000, 60_000_000),
        (60_000_000, 74_800_000),
    ];
    RANGES
        .iter()
        .position(|&(low, high)| lower_hz >= low && upper_hz <= high)
        .map(|i| i as u8 + 1)
}
pub(crate) fn decimal_to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_documented_scope_commands() {
        assert_eq!(scope_mode_payload(true), [0x27, 0x14, 0x00, 0x01]);
        assert_eq!(
            scope_edge_number_payload(2).unwrap(),
            [0x27, 0x16, 0x00, 0x02]
        );
        assert_eq!(
            scope_edge_number_payload(4).unwrap(),
            [0x27, 0x16, 0x00, 0x04]
        );
        assert_eq!(
            scope_sweep_speed_payload(0).unwrap(),
            [0x27, 0x1A, 0x00, 0x00]
        );
        assert_eq!(scope_hold_payload(true), [0x27, 0x17, 0x00, 0x01]);
        assert_eq!(
            scope_reference_level_payload(105).unwrap(),
            [0x27, 0x19, 0x00, 0x10, 0x50, 0x00]
        );
        assert!(scope_edge_number_payload(0).is_err());
        assert!(scope_sweep_speed_payload(3).is_err());
        assert!(scope_fixed_edges_payload(5, 14_000_000, 14_350_000).is_err());
        assert_eq!(
            scope_reference_level_payload(-200).unwrap(),
            [0x27, 0x19, 0x00, 0x20, 0x00, 0x01]
        );
        assert_eq!(scope_vbw_payload(true), [0x27, 0x1D, 0x00, 0x01]);
        assert_eq!(
            scope_span_payload(2_500).unwrap(),
            vec![0x27, 0x15, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            scope_span_payload(500_000).unwrap(),
            vec![0x27, 0x15, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00]
        );
        assert_eq!(
            scope_fixed_edges_payload(1, 14_000_000, 14_350_000).unwrap(),
            vec![
                0x27, 0x1E, 0x06, 0x01, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x35, 0x14, 0x00
            ]
        );
        assert_eq!(
            scope_fixed_edges_payload(1, 28_000_000, 29_700_000)
                .unwrap()
                .get(2),
            Some(&0x10)
        );
    }
}
