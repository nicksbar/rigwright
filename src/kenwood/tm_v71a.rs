//! Kenwood TM-V71A main-unit profile.
//!
//! The TM-V71A PC command family is not covered by Kenwood's HF PC-control
//! reference.  This profile follows the reverse-engineered command reference
//! maintained by LA3QMA and contributors:
//! <https://github.com/LA3QMA/TM-V71_TM-D710-Kenwood>.
//!
//! The reference covers both the TM-V71 and TM-D710(G) main units.  Rigwright
//! exposes both as separate catalog profiles while sharing only the validated
//! command implementation in this module.

use super::profile::{
    KenwoodCatProfile, KenwoodCoreOps, KenwoodModeCommand, KenwoodModeSpec, KenwoodProfileIo,
    KenwoodRitXitLayout, KenwoodSplitCommand,
};
use crate::hal::Mode;
use crate::models::{find_model, RadioModelProfile};

const MODES: &[KenwoodModeSpec] = &[
    KenwoodModeSpec {
        code: '0',
        mode: Mode::Fm,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '1',
        mode: Mode::Fm,
        preferred: false,
    },
    KenwoodModeSpec {
        code: '2',
        mode: Mode::Am,
        preferred: true,
    },
];

const BAUD_RATES: &[u32] = &[9_600, 19_200, 38_400, 57_600];
const FREQUENCY_RANGES: &[(u64, u64)] = &[(118_000_000, 524_000_000), (800_000_000, 1_300_000_000)];

/// TM-V71A does not expose the HF Kenwood `SM`/`RM` meter family through this
/// command reference.  Keep optional controls and meters unavailable until a
/// model-specific, tested mapping exists.
const fn make_profile(
    model: crate::models::KenwoodCatModel,
    id_code: &'static str,
) -> KenwoodCatProfile {
    KenwoodCatProfile {
        model,
        id_code,
        frequency_ranges: FREQUENCY_RANGES,
        baud_rates: BAUD_RATES,
        modes: MODES,
        mode_command: KenwoodModeCommand::Md {
            supports_data_flag: false,
        },
        split_command: KenwoodSplitCommand::ReceiverTransmitterVfo,
        supports_vfo: false,
        supports_split: false,
        supports_if_status: false,
        power_range_watts: None,
        meter_max: 0,
        swr_meter_max: 0,
        swr_rm_selector: '0',
        controls: &[],
        preamp_values: &[],
        filter_minimum: 0,
        extra_meters: &[],
        supports_signal_meter: false,
        supports_power_meter: false,
        supports_swr_meter: false,
        rit_xit_layout: KenwoodRitXitLayout::IfStatus,
        memory: None,
        ai_on_value: "0",
        sm_payload_len: 0,
        sm_value_start: 0,
        swr_meter_selection: None,
        extra_meter_selection: None,
        repeater: None,
        core_ops: Some(&CORE_OPS),
    }
}

pub const CAT_PROFILE: KenwoodCatProfile =
    make_profile(crate::models::KenwoodCatModel::TmV71A, "TM-V71");

pub const TM_D710_PROFILE: KenwoodCatProfile =
    make_profile(crate::models::KenwoodCatModel::TmD710, "TM-D710");

static CORE_OPS: KenwoodCoreOps = KenwoodCoreOps {
    get_frequency,
    set_frequency,
    get_mode,
    set_mode,
    set_ptt,
};

struct FoRecord {
    fields: Vec<String>,
    frequency_hz: u64,
}

fn read_fo(io: &dyn KenwoodProfileIo) -> anyhow::Result<FoRecord> {
    let response = io.profile_query("FO", Some(" 0"))?;
    let text = std::str::from_utf8(&response)?;
    let payload = text
        .strip_prefix("FO")
        .and_then(|value| value.strip_suffix(';'))
        .ok_or_else(|| anyhow::anyhow!("invalid TM-V71A FO response"))?;
    let fields: Vec<String> = payload.trim().split(',').map(str::to_owned).collect();
    anyhow::ensure!(fields.len() >= 13, "TM-V71A FO response is too short");
    let frequency_hz = fields[1].parse()?;
    Ok(FoRecord {
        fields,
        frequency_hz,
    })
}

fn write_fo(io: &dyn KenwoodProfileIo, fields: &[String]) -> anyhow::Result<()> {
    anyhow::ensure!(fields.len() >= 13, "TM-V71A FO record is too short");
    io.profile_send_set("FO", &format!(" {}", fields.join(",")))
}

fn get_frequency(io: &dyn KenwoodProfileIo, _profile: &KenwoodCatProfile) -> anyhow::Result<u64> {
    Ok(read_fo(io)?.frequency_hz)
}

fn set_frequency(
    io: &dyn KenwoodProfileIo,
    profile: &KenwoodCatProfile,
    hz: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        profile.supports_frequency(hz),
        "frequency is outside TM-V71A range"
    );
    let mut record = read_fo(io)?;
    record.fields[1] = format!("{hz:010}");
    write_fo(io, &record.fields)
}

fn get_mode(io: &dyn KenwoodProfileIo, profile: &KenwoodCatProfile) -> anyhow::Result<Mode> {
    let record = read_fo(io)?;
    profile.decode_mode(
        record.fields[12]
            .chars()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing TM-V71A mode"))?,
    )
}

fn set_mode(
    io: &dyn KenwoodProfileIo,
    profile: &KenwoodCatProfile,
    mode: Mode,
) -> anyhow::Result<()> {
    let mut record = read_fo(io)?;
    record.fields[12] = profile.encode_mode(mode)?.to_string();
    write_fo(io, &record.fields)
}

fn set_ptt(
    io: &dyn KenwoodProfileIo,
    _profile: &KenwoodCatProfile,
    enabled: bool,
) -> anyhow::Result<()> {
    io.profile_send_set(if enabled { "TX" } else { "RX" }, "")
}

pub fn profile() -> &'static RadioModelProfile {
    find_model("TM-V71A").expect("built-in TM-V71A profile")
}

pub fn d710_profile() -> &'static RadioModelProfile {
    find_model("TM-D710").expect("built-in TM-D710 profile")
}

pub use CAT_PROFILE as TM_V71A_PROFILE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_reverse_engineered_tm_v71a_contract() {
        assert_eq!(profile().model, "TM-V71A");
        assert_eq!(CAT_PROFILE.preferred_baud_rate(), 57_600);
        assert!(CAT_PROFILE.supports_frequency(145_000_000));
        assert!(CAT_PROFILE.supports_frequency(430_000_000));
        assert!(!CAT_PROFILE.supports_frequency(700_000_000));
        assert_eq!(CAT_PROFILE.encode_mode(Mode::Fm).unwrap(), '0');
        assert_eq!(CAT_PROFILE.encode_mode(Mode::Am).unwrap(), '2');
        assert!(CAT_PROFILE.encode_mode(Mode::Usb).is_err());
        assert!(!CAT_PROFILE.supports_control(crate::ControlId::RfPower));
        assert!(!CAT_PROFILE.supports_meter(crate::MeterId::Signal));
    }

    #[test]
    fn exposes_the_reverse_engineered_tm_d710_contract() {
        assert_eq!(d710_profile().model, "TM-D710");
        assert_eq!(
            TM_D710_PROFILE.model,
            crate::models::KenwoodCatModel::TmD710
        );
        assert_eq!(TM_D710_PROFILE.id_code, "TM-D710");
        assert_eq!(TM_D710_PROFILE.preferred_baud_rate(), 57_600);
        assert!(TM_D710_PROFILE.supports_frequency(145_000_000));
        assert!(TM_D710_PROFILE.core_ops.is_some());
    }
}
