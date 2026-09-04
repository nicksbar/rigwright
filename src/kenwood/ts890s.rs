//! Kenwood TS-890S model profile (framework only; validation pending).

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
    KenwoodModeSpec {
        code: 'A',
        mode: Mode::Data,
        preferred: false,
    },
    KenwoodModeSpec {
        code: 'B',
        mode: Mode::Data,
        preferred: false,
    },
    KenwoodModeSpec {
        code: 'C',
        mode: Mode::Data,
        preferred: false,
    },
    KenwoodModeSpec {
        code: 'D',
        mode: Mode::Data,
        preferred: true,
    },
    KenwoodModeSpec {
        code: 'E',
        mode: Mode::Data,
        preferred: false,
    },
    KenwoodModeSpec {
        code: 'F',
        mode: Mode::Data,
        preferred: false,
    },
];
const BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];
const FREQUENCY_RANGES: &[(u64, u64)] = &[(30_000, 60_000_000)];

pub(crate) const CONTROLS: &[KenwoodControlSpec] = &[
    KenwoodControlSpec {
        id: ControlId::AfGain,
        command: "AG",
        response_len: 3,
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
        response_len: 3,
        max_value: Some(255),
    },
    KenwoodControlSpec {
        id: ControlId::Preamp,
        command: "PA",
        response_len: 1,
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::NoiseBlanker,
        command: "NB1",
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
        response_len: 1,
        max_value: Some(1),
    },
    KenwoodControlSpec {
        id: ControlId::Filter,
        command: "FL0",
        response_len: 2,
        max_value: Some(2),
    },
    KenwoodControlSpec {
        id: ControlId::Agc,
        command: "GC",
        response_len: 1,
        max_value: Some(3),
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
        selector: '1',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Compression,
        command: "RM",
        selector: '3',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Current,
        command: "RM",
        selector: '4',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Voltage,
        command: "RM",
        selector: '5',
        maximum: 70,
    },
    KenwoodMeterSpec {
        id: MeterId::Temperature,
        command: "RM",
        selector: '6',
        maximum: 70,
    },
];

pub(crate) const MEMORY: KenwoodMemorySpec = KenwoodMemorySpec {
    channel_max: 119,
    select_vfo: 3,
    select_command: "MN",
    read_command: "MA0",
    write_command: "MA0",
    read_parameters: |channel| format!("{channel:03}"),
    decode: decode_memory,
    encode: encode_memory,
};

pub(crate) fn decode_memory(payload: &str, profile: &KenwoodCatProfile) -> Result<MemoryChannel> {
    anyhow::ensure!(payload.len() >= 36, "TS-890S MA0 response is too short");
    let channel = parse_memory_field::<u16>(&payload[0..3], "memory channel")?;
    let frequency_hz = parse_memory_field::<u64>(&payload[3..14], "memory frequency")?;
    let mode = profile.decode_mode(memory_char(&payload[14..15], "memory mode")?)?;
    let tone_mode = match &payload[16..17] {
        "0" => ToneMode::Off,
        "1" => ToneMode::Encode,
        "2" | "3" => ToneMode::EncodeDecode,
        value => bail!("invalid TS-890S memory tone mode: {value}"),
    };
    let tone_index = parse_memory_field::<u8>(&payload[19..21], "memory CTCSS index")?;
    let tx_frequency = parse_memory_field::<u64>(&payload[21..32], "memory transmit frequency")?;
    let name = payload[36..].trim().to_owned();
    Ok(MemoryChannel {
        channel,
        name: (!name.is_empty()).then_some(name),
        frequency_hz,
        transmit_frequency_hz: (tx_frequency != 0).then_some(tx_frequency),
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
    anyhow::ensure!(
        channel.frequency_hz <= 99_999_999_999,
        "Kenwood memory frequency does not fit the documented 11-digit field"
    );
    anyhow::ensure!(
        matches!(channel.repeater.shift, RepeaterShift::Simplex),
        "Kenwood MA0 memory writes require simplex; split memory needs a separate TX frequency"
    );
    let mode = profile.encode_mode(channel.mode)?;
    let tone_type = match channel.repeater.tone.mode {
        ToneMode::Off => '0',
        ToneMode::Encode => '1',
        ToneMode::EncodeDecode => '2',
        ToneMode::Dtcs => bail!("DTCS is not supported by this Kenwood profile"),
    };
    anyhow::ensure!(
        channel.repeater.tone.index <= 41,
        "Kenwood CTCSS index must be 0..=41"
    );
    let tx_frequency = channel
        .transmit_frequency_hz
        .unwrap_or(channel.frequency_hz);
    anyhow::ensure!(
        tx_frequency <= 99_999_999_999,
        "Kenwood transmit memory frequency does not fit the documented 11-digit field"
    );
    let name = channel.name.unwrap_or_default();
    anyhow::ensure!(
        name.is_ascii() && name.len() <= 10,
        "Kenwood channel name must be ASCII and at most 10 characters"
    );
    Ok(format!(
        "{:03}{:011}{mode}0{tone_type}00{:02}{:011}{mode}000{name}",
        channel.channel, channel.frequency_hz, channel.repeater.tone.index, tx_frequency,
    ))
}

pub const CAT_PROFILE: KenwoodCatProfile = KenwoodCatProfile {
    model: crate::models::KenwoodCatModel::Ts890S,
    id_code: "024",
    frequency_ranges: FREQUENCY_RANGES,
    baud_rates: BAUD_RATES,
    modes: MODES,
    mode_command: KenwoodModeCommand::Om,
    split_command: KenwoodSplitCommand::Tb,
    supports_vfo: true,
    supports_split: true,
    supports_if_status: false,
    power_range_watts: Some((5, 100)),
    meter_max: 70,
    swr_meter_max: 70,
    swr_rm_selector: '2',
    controls: CONTROLS,
    extra_meters: METERS,
    rit_xit_layout: KenwoodRitXitLayout::RfAndFunctionState,
    memory: Some(MEMORY),
    ai_on_value: "2",
    sm_payload_len: 4,
    sm_value_start: 0,
    swr_meter_requires_selection: true,
};

pub use CAT_PROFILE as TS890S_PROFILE;

pub fn profile() -> &'static RadioModelProfile {
    find_model("TS-890S").expect("built-in TS-890S profile")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_the_ts890s_catalog_profile() {
        assert_eq!(profile().model, "TS-890S");
        assert!(!CONTROLS.is_empty());
        assert!(!METERS.is_empty());
    }
}
