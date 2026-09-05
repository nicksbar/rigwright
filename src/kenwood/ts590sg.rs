//! Kenwood TS-590SG model profile (framework only; validation pending).

use anyhow::{bail, Result};

use super::profile::{
    memory_char, parse_memory_field, KenwoodCatProfile, KenwoodControlSpec, KenwoodMemorySpec,
    KenwoodMeterSpec, KenwoodModeCommand, KenwoodModeSpec, KenwoodRitXitLayout,
    KenwoodSplitCommand,
};
use crate::hal::Mode;
use crate::hal_types::{
    ControlId, MemoryChannel, MeterId, RepeaterSettings, RepeaterShift, ToneMode, ToneSettings,
};
use crate::models::{find_model, RadioModelProfile};

const MODES: &[KenwoodModeSpec] = &[
    KenwoodModeSpec {
        code: '1',
        mode: Mode::Lsb,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '2',
        mode: Mode::Usb,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '2',
        mode: Mode::Data,
        preferred: false,
    },
    KenwoodModeSpec {
        code: '3',
        mode: Mode::Cw,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '4',
        mode: Mode::Fm,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '5',
        mode: Mode::Am,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '6',
        mode: Mode::Rtty,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '7',
        mode: Mode::CwReverse,
        preferred: true,
    },
    KenwoodModeSpec {
        code: '9',
        mode: Mode::RttyReverse,
        preferred: true,
    },
];
const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];
const FREQUENCY_RANGES: &[(u64, u64)] = &[(30_000, 60_000_000)];

pub(crate) const CONTROLS: &[KenwoodControlSpec] = &[
    KenwoodControlSpec {
        id: ControlId::AfGain,
        command: "AG",
        response_len: 4,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::RfGain,
        command: "RG",
        response_len: 3,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::Squelch,
        command: "SQ",
        response_len: 4,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::Preamp,
        command: "PA",
        response_len: 2,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::NoiseBlanker,
        command: "NB",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::NoiseReduction,
        command: "NR",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::Notch,
        command: "NT",
        response_len: 2,
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::Filter,
        command: "FL",
        response_len: 1,
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::Rit,
        command: "RT",
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::Xit,
        command: "XT",
        response_len: 1,
        max_value: Some(1),
    },
];
pub(crate) const METERS: &[KenwoodMeterSpec] = &[
    KenwoodMeterSpec {
        id: MeterId::Alc,
        command: "RM",
        selector: '3',
        maximum: 30,
    },
    KenwoodMeterSpec {
        id: MeterId::Compression,
        command: "RM",
        selector: '2',
        maximum: 30,
    },
];

pub(crate) const MEMORY: KenwoodMemorySpec = KenwoodMemorySpec {
    channel_max: 119,
    select_vfo: 2,
    select_command: "MC",
    read_command: "MR",
    write_command: "MW",
    read_parameters: |channel| format!("0{channel:03}"),
    decode: decode_memory,
    encode: encode_memory,
};

pub(crate) fn decode_memory(payload: &str, profile: &KenwoodCatProfile) -> Result<MemoryChannel> {
    anyhow::ensure!(payload.len() >= 39, "TS-590SG MR response is too short");
    let split = payload.as_bytes()[0] == b'1';
    let channel = parse_memory_field::<u16>(&payload[1..4], "memory channel")?;
    let frequency_hz = parse_memory_field::<u64>(&payload[4..15], "memory frequency")?;
    let mode = profile.decode_mode(memory_char(&payload[15..16], "memory mode")?)?;
    let tone_mode = match &payload[17..18] {
        "0" => ToneMode::Off,
        "1" => ToneMode::Encode,
        "2" | "3" => ToneMode::EncodeDecode,
        value => bail!("invalid TS-590SG memory tone mode: {value}"),
    };
    let tone_index = parse_memory_field::<u8>(&payload[20..22], "memory CTCSS index")?;
    let name = payload[39..].trim().to_owned();
    Ok(MemoryChannel {
        channel,
        name: (!name.is_empty()).then_some(name),
        frequency_hz,
        transmit_frequency_hz: split.then_some(frequency_hz),
        mode,
        repeater: RepeaterSettings {
            shift: RepeaterShift::Simplex,
            offset_hz: None,
            tone: ToneSettings {
                mode: tone_mode,
                index: tone_index,
                frequency_tenths_hz: None,
                dtcs_code: None,
                dtcs_reverse: None,
            },
        },
    })
}

fn encode_memory(channel: MemoryChannel, profile: &KenwoodCatProfile) -> Result<String> {
    let mode = profile.encode_mode(channel.mode)?;
    anyhow::ensure!(
        channel.repeater.tone.index <= 41,
        "Kenwood tone index must be 0..=41"
    );
    let split = channel.transmit_frequency_hz.is_some();
    let frequency = channel
        .transmit_frequency_hz
        .unwrap_or(channel.frequency_hz);
    anyhow::ensure!(
        frequency <= 99_999_999_999,
        "Kenwood memory frequency does not fit the documented 11-digit field"
    );
    let tone = match channel.repeater.tone.mode {
        ToneMode::Off => '0',
        ToneMode::Encode => '1',
        ToneMode::EncodeDecode => '2',
        ToneMode::Dtcs => bail!("DTCS is not supported by the TS-590SG memory format"),
    };
    let name = channel.name.unwrap_or_default();
    anyhow::ensure!(
        name.is_ascii() && name.len() <= 8,
        "TS-590SG memory name must be ASCII and at most 8 characters"
    );
    Ok(format!(
        "{}{channel_number:03}{frequency:011}{mode}0{tone}00{tone_index:02}00000000000000000{name}",
        if split { '1' } else { '0' },
        channel_number = channel.channel,
        tone_index = channel.repeater.tone.index,
    ))
}

pub const CAT_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: crate::models::KenwoodCatModel::Ts590Sg,
    id_code: "023",
    frequency_ranges: FREQUENCY_RANGES,
    baud_rates: BAUD_RATES,
    modes: MODES,
    mode_command: KenwoodModeCommand::Md {
        supports_data_flag: true,
    },
    split_command: KenwoodSplitCommand::ReceiverTransmitterVfo,
    supports_vfo: true,
    supports_split: true,
    supports_if_status: true,
    power_range_watts: Some((5, 100)),
    meter_max: 30,
    swr_meter_max: 30,
    swr_rm_selector: '1',
    controls: CONTROLS,
    preamp_values: &[0, 1],
    filter_minimum: 0,
    extra_meters: METERS,
    supports_signal_meter: true,
    supports_power_meter: true,
    supports_swr_meter: true,
    rit_xit_layout: KenwoodRitXitLayout::IfStatus,
    memory: Some(MEMORY),
    ai_on_value: "2",
    sm_payload_len: 5,
    sm_value_start: 1,
    swr_meter_selection: None,
    extra_meter_selection: None,
    repeater: Some(super::profile::STANDARD_REPEATER),
};

pub use CAT_PROFILE as TS590SG_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-590SG").expect("built-in TS-590SG profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ts590sg_catalog_profile() {
        assert_eq!(profile().model, "TS-590SG");
        assert!(!CONTROLS.is_empty());
        assert!(!METERS.is_empty());
    }
}
