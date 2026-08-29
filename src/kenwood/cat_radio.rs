//! Model-neutral transport for Kenwood semicolon-terminated PC control.

use std::{
    io::ErrorKind,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::{
    events::{RadioEvent, RadioEventRouter},
    hal::{Mode, Radio, RadioCapabilities},
    hal_types::{
        normalize_meter_level, ControlId, ControlValue, MemoryChannel, MeterId, RepeaterSettings,
        RepeaterShift, ToneMode, ToneSettings,
    },
    models::KenwoodCatModel,
    protocol::ascii_cat,
    transport::{RadioTransport, SerialPortTransport},
};

use super::profile::{
    profile_for_model, KenwoodCatProfile, KenwoodMemorySurface, KenwoodModeCommand,
    KenwoodRitXitLayout, KenwoodSplitCommand,
};

const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_200);
const MAX_FRAME_LEN: usize = 512;

#[derive(Default)]
struct TransportState {
    port: Option<Box<dyn RadioTransport>>,
    external: Option<Box<dyn RadioTransport>>,
    pending: Vec<u8>,
}

fn active_transport(state: &mut TransportState) -> Result<&mut dyn RadioTransport> {
    if let Some(port) = state.port.as_mut() {
        Ok(&mut **port)
    } else if let Some(port) = state.external.as_mut() {
        Ok(&mut **port)
    } else {
        bail!("Kenwood CAT port unavailable")
    }
}

/// Profile-driven Kenwood PC-control driver.
///
/// The serial connection is reused across operations. The transport tolerates
/// interleaved Auto Information frames and matches the requested response by
/// command prefix. Model profiles own command-family differences such as
/// `MD` versus `OM`, `FR`/`FT` versus `TB` split, and `IF` status support.
#[derive(Clone)]
pub struct KenwoodCatRadio {
    model: Option<KenwoodCatModel>,
    port: String,
    baud_rate: u32,
    transport: Arc<Mutex<TransportState>>,
    event_router: RadioEventRouter,
}

impl std::fmt::Debug for KenwoodCatRadio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KenwoodCatRadio")
            .field("model", &self.model)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .finish_non_exhaustive()
    }
}

impl KenwoodCatRadio {
    /// Construct a generic Kenwood driver. Model validation, range checks, and
    /// model-dependent controls require [`Self::new_for_model`].
    pub fn new_generic(port: impl Into<String>, baud_rate: u32) -> Self {
        Self::new_internal(None, port, baud_rate)
    }

    pub fn new_for_model(
        model: KenwoodCatModel,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Result<Self> {
        let profile = profile_for_model(model);
        if !profile.baud_rates.contains(&baud_rate) {
            bail!(
                "unsupported PC-control baud rate {baud_rate} for {}; documented rates: {:?}",
                model.model_name(),
                profile.baud_rates
            );
        }
        Ok(Self::new_internal(Some(model), port, baud_rate))
    }

    /// Construct a Kenwood CAT radio over an externally configured byte
    /// transport, such as Android USB Host or Bluetooth.
    pub fn with_external_transport<T>(
        model: Option<KenwoodCatModel>,
        baud_rate: u32,
        transport: T,
    ) -> Self
    where
        T: RadioTransport + 'static,
    {
        Self {
            model,
            port: String::new(),
            baud_rate,
            transport: Arc::new(Mutex::new(TransportState {
                port: None,
                external: Some(Box::new(transport)),
                pending: Vec::new(),
            })),
            event_router: RadioEventRouter::default(),
        }
    }

    fn new_internal(
        model: Option<KenwoodCatModel>,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Self {
        Self {
            model,
            port: port.into(),
            baud_rate,
            transport: Arc::new(Mutex::new(TransportState::default())),
            event_router: RadioEventRouter::default(),
        }
    }

    pub fn model(&self) -> Option<KenwoodCatModel> {
        self.model
    }

    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    /// Enable or disable Kenwood Auto Information output on this connection.
    /// TS-890S uses value 2 so the setting is not persisted across power cycles;
    /// the older command families use the documented 0/1 form.
    pub fn set_auto_information(&self, enabled: bool) -> Result<()> {
        let value = if enabled {
            self.selected_profile()?.ai_on_value
        } else {
            "0"
        };
        self.send_set("AI", value)
    }

    pub fn profile(&self) -> Option<&'static KenwoodCatProfile> {
        self.model.map(profile_for_model)
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.transport.lock() {
            state.port = None;
            state.external = None;
            state.pending.clear();
        }
    }

    /// Query `ID;` and reject a radio that does not match the selected model.
    pub fn verify_model(&self) -> Result<()> {
        let profile = self.selected_profile()?;
        let response = self.query("ID", None, Some(3))?;
        let id = parse_payload(&response, "ID")?;
        if id != profile.id_code {
            bail!(
                "Kenwood radio identified as {id}, expected {} ({})",
                profile.id_code,
                profile.model.model_name()
            );
        }
        Ok(())
    }

    pub fn send_set(&self, command: &str, parameters: &str) -> Result<()> {
        self.write_only(&ascii_cat::encode(command, Some(parameters))?)
    }

    pub fn send_raw(&self, request: &[u8]) -> Result<()> {
        self.write_only(request)
    }

    pub fn query_raw(&self, command: &str, parameters: Option<&str>) -> Result<Vec<u8>> {
        self.query(command, parameters, None)
    }

    pub fn get_power_watts(&self) -> Result<u16> {
        self.selected_profile()?
            .power_range_watts
            .context("RF power control is not profiled for this Kenwood model")?;
        parse_payload(&self.query("PC", None, Some(3))?, "PC")?
            .parse()
            .context("invalid Kenwood PC response")
    }

    /// Set RF power in watts. The profiled range is the broad radio range;
    /// modes such as AM and some TS-2000 bands impose lower radio-side maxima.
    pub fn set_power_watts(&self, watts: u16) -> Result<()> {
        self.selected_profile()?.validate_power(watts)?;
        self.send_set("PC", &format!("{watts:03}"))
    }

    pub fn get_power_state(&self) -> Result<bool> {
        match parse_payload(&self.query("PS", None, Some(1))?, "PS")? {
            "0" => Ok(false),
            "1" => Ok(true),
            value => bail!("invalid Kenwood PS response: {value}"),
        }
    }

    pub fn set_power_state(&self, enabled: bool) -> Result<()> {
        self.send_set("PS", if enabled { "1" } else { "0" })
    }

    pub fn get_meter(&self) -> Result<u16> {
        let profile = self.selected_profile()?;
        let parameters = (profile.sm_value_start != 0).then_some("0");
        let response = self.query("SM", parameters, Some(profile.sm_payload_len))?;
        let payload = parse_payload(&response, "SM")?;
        let value = &payload[profile.sm_value_start..];
        let value: u16 = value.parse().context("invalid Kenwood SM response")?;
        if value > profile.meter_max {
            bail!("Kenwood SM response exceeds the profiled meter range: {value}");
        }
        Ok(value)
    }

    pub(crate) fn get_swr_meter(&self) -> Result<u8> {
        let profile = self.selected_profile()?;
        if profile.swr_meter_requires_selection {
            self.send_set("RM", "21")?;
        }
        self.get_rm_meter(profile.swr_rm_selector, profile.swr_meter_max)
    }

    fn get_rm_meter(&self, selector: char, maximum: u16) -> Result<u8> {
        let response = self.query("RM", None, Some(5))?;
        let payload = parse_payload(&response, "RM")?;
        let meter_type = payload
            .chars()
            .next()
            .context("missing Kenwood RM meter selector")?;
        if meter_type != selector {
            bail!(
                "Kenwood RM response meter {meter_type} is not the requested meter {selector}: {payload}"
            );
        }
        let value: u16 = payload[1..]
            .parse()
            .context("invalid Kenwood RM response")?;
        normalize_meter_level(value, maximum)
            .context("Kenwood RM response exceeds the profiled range")
    }

    pub(crate) fn get_signal_meter(&self) -> Result<u8> {
        let profile = self.selected_profile()?;
        let value = self.get_meter()?;
        normalize_meter_level(value, profile.meter_max)
            .context("Kenwood SM response exceeds the profiled meter range")
    }

    fn get_noise_blanker(&self) -> Result<bool> {
        let spec = self
            .selected_profile()?
            .control(ControlId::NoiseBlanker)
            .context("Kenwood noise blanker is not profiled")?;
        Ok(self.get_flag_or_level_with_len(spec.command, spec.response_len)? != 0)
    }

    fn get_flag_or_level_with_len(&self, command: &str, payload_len: usize) -> Result<u8> {
        let response = self.query(command, None, Some(payload_len))?;
        let payload = parse_payload(&response, command)?;
        payload
            .chars()
            .next()
            .and_then(|value| value.to_digit(10))
            .map(|value| value as u8)
            .context("invalid Kenwood control response")
    }

    fn set_noise_blanker(&self, enabled: bool) -> Result<()> {
        let command = self
            .selected_profile()?
            .control(ControlId::NoiseBlanker)
            .context("Kenwood noise blanker is not profiled")?
            .command;
        self.send_set(command, if enabled { "1" } else { "0" })
    }

    fn set_flag_or_level(&self, command: &str, value: u8) -> Result<()> {
        self.send_set(command, &value.to_string())
    }

    fn get_ts890_meter(&self, selector: char) -> Result<u8> {
        // TS-890S meters are initially configured as "do not read" after
        // power-on. Select this meter for reading before requesting its value.
        self.send_set("RM", &format!("{selector}1"))?;
        let response = self.query("RM", None, Some(5))?;
        let payload = parse_payload(&response, "RM")?;
        anyhow::ensure!(
            payload.starts_with(selector),
            "unexpected TS-890S RM meter: {payload}"
        );
        let value: u16 = payload[1..]
            .parse()
            .context("invalid TS-890S meter response")?;
        normalize_meter_level(value, 70).context("TS-890S meter exceeds 0..70")
    }

    fn get_level_control(&self, id: ControlId) -> Result<u8> {
        let spec = self
            .selected_profile()?
            .control(id)
            .filter(|_| {
                matches!(
                    id,
                    ControlId::AfGain | ControlId::RfGain | ControlId::Squelch
                )
            })
            .context("Kenwood control is not a level control")?;
        let response = self.query(spec.command, None, Some(spec.response_len))?;
        let payload = parse_payload(&response, spec.command)?;
        let value = if spec.response_len > 3 {
            &payload[1..]
        } else {
            payload
        };
        value.parse().context("invalid Kenwood normalized level")
    }

    fn get_agc(&self) -> Result<u8> {
        let spec = self
            .selected_profile()?
            .control(ControlId::Agc)
            .context("AGC is not profiled for this Kenwood model")?;
        self.get_flag_or_level_with_len(spec.command, spec.response_len)
    }

    fn get_filter(&self) -> Result<u8> {
        let spec = self
            .selected_profile()?
            .control(ControlId::Filter)
            .context("Kenwood filter selection is not profiled")?;
        self.get_flag_or_level_with_len(spec.command, spec.response_len)
    }

    fn set_filter(&self, value: u8) -> Result<()> {
        let spec = self
            .selected_profile()?
            .control(ControlId::Filter)
            .context("Kenwood filter selection is not profiled")?;
        if spec.max_value.is_some_and(|maximum| value <= maximum)
            && !(spec.max_value == Some(2) && spec.command == "FL" && value == 0)
        {
            self.send_set(spec.command, &value.to_string())
        } else {
            bail!("invalid or unprofiled Kenwood IF filter selection: {value}")
        }
    }

    fn set_level_control(&self, id: ControlId, value: u8) -> Result<()> {
        let parameter = format!("{value:03}");
        let spec = self
            .selected_profile()?
            .control(id)
            .filter(|_| {
                matches!(
                    id,
                    ControlId::AfGain | ControlId::RfGain | ControlId::Squelch
                )
            })
            .context("Kenwood control is not a level control")?;
        let parameters = if spec.response_len > 3 {
            format!("0{parameter}")
        } else {
            parameter
        };
        self.send_set(spec.command, &parameters)
    }

    pub fn get_split(&self) -> Result<bool> {
        match self.selected_profile()?.split_command {
            KenwoodSplitCommand::Tb => parse_bool_payload(&self.query("TB", None, Some(1))?, "TB"),
            KenwoodSplitCommand::ReceiverTransmitterVfo => {
                Ok(self.read_vfo("FR")? != self.read_vfo("FT")?)
            }
        }
    }

    pub fn set_split(&self, enabled: bool) -> Result<()> {
        match self.selected_profile()?.split_command {
            KenwoodSplitCommand::Tb => self.send_set("TB", if enabled { "1" } else { "0" }),
            KenwoodSplitCommand::ReceiverTransmitterVfo => {
                let receive = self.read_vfo("FR")?;
                let transmit = if enabled { 1 - receive } else { receive };
                self.send_set("FT", &transmit.to_string())
            }
        }
    }

    fn read_if_rit_xit(&self) -> Result<(i32, bool, bool)> {
        if self.selected_profile()?.rit_xit_layout == KenwoodRitXitLayout::RfAndFunctionState {
            let response = self.query("RF", None, Some(5))?;
            let rf = parse_payload(&response, "RF")?;
            anyhow::ensure!(rf.len() == 5, "invalid TS-890S RF response");
            let magnitude: i32 = rf[1..].parse().context("invalid TS-890S RIT/XIT offset")?;
            let offset = match &rf[0..1] {
                "0" => magnitude,
                "1" => -magnitude,
                value => bail!("invalid TS-890S RIT/XIT direction: {value}"),
            };
            let rit = parse_bool_payload(&self.query("RT", None, Some(1))?, "RT")?;
            let xit = parse_bool_payload(&self.query("XT", None, Some(1))?, "XT")?;
            return Ok((offset, rit, xit));
        }
        let response = self.query("IF", None, None)?;
        let payload = parse_payload(&response, "IF")?;
        anyhow::ensure!(payload.len() >= 23, "Kenwood IF response is too short");
        let offset: i32 = payload[16..21]
            .parse()
            .context("invalid Kenwood RIT/XIT offset")?;
        Ok((
            offset,
            payload.as_bytes()[21] == b'1',
            payload.as_bytes()[22] == b'1',
        ))
    }

    fn set_rit_xit_offset(&self, offset_hz: i32) -> Result<()> {
        anyhow::ensure!(
            (-9_999..=9_999).contains(&offset_hz),
            "Kenwood RIT/XIT offset must fit ±9999 Hz"
        );
        self.send_set("RC", "")?;
        if offset_hz >= 0 {
            self.send_set("RU", &format!("{offset_hz:05}"))
        } else {
            self.send_set("RD", &format!("{:05}", offset_hz.unsigned_abs()))
        }
    }

    pub fn get_rit_offset_hz(&self) -> Result<i32> {
        Ok(self.read_if_rit_xit()?.0)
    }

    pub fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.send_set("RT", "1")?;
        self.set_rit_xit_offset(offset_hz)
    }

    pub fn get_xit_offset_hz(&self) -> Result<i32> {
        Ok(self.read_if_rit_xit()?.0)
    }

    pub fn set_xit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.send_set("XT", "1")?;
        self.set_rit_xit_offset(offset_hz)
    }

    pub fn start_tuner(&self) -> Result<()> {
        self.send_set("AC", "111")
    }

    pub fn get_tuner_status(&self) -> Result<crate::hal_types::TunerStatus> {
        let response = self.query("AC", None, Some(3))?;
        let payload = parse_payload(&response, "AC")?;
        anyhow::ensure!(payload.len() == 3, "invalid Kenwood tuner response");
        Ok(crate::hal_types::TunerStatus {
            enabled: payload.as_bytes()[0] == b'1',
            tuning: payload.as_bytes()[2] == b'1',
        })
    }

    /// Kenwood PC command references document CN (CTCSS frequency index) and
    /// CT (tone function) across the supported CAT families. Shift and offset
    /// are intentionally not synthesized here because their command forms
    /// differ by model.
    pub fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        let tone_index = parse_payload(&self.query("CN", None, Some(2))?, "CN")?
            .parse::<u8>()
            .context("invalid Kenwood CN tone index")?;
        let mode = match parse_payload(&self.query("CT", None, Some(1))?, "CT")? {
            "0" => ToneMode::Off,
            "1" => ToneMode::Encode,
            "2" => ToneMode::EncodeDecode,
            value => bail!("invalid Kenwood CT tone mode: {value}"),
        };
        Ok(RepeaterSettings {
            shift: RepeaterShift::Simplex,
            offset_hz: None,
            tone: ToneSettings {
                mode,
                index: tone_index,
                frequency_tenths_hz: None,
                dtcs_code: None,
                dtcs_reverse: None,
            },
        })
    }

    pub fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        if !matches!(settings.shift, RepeaterShift::Simplex) || settings.offset_hz.is_some() {
            bail!("Kenwood repeater shift/offset is not yet model-profiled");
        }
        if settings.tone.index > 41 {
            bail!("Kenwood CTCSS tone index must be 0..=41");
        }
        let mode = match settings.tone.mode {
            ToneMode::Off => "0",
            ToneMode::Encode => "1",
            ToneMode::Dtcs => anyhow::bail!("DTCS is not supported by this Kenwood profile"),
            ToneMode::EncodeDecode => "2",
        };
        self.send_set("CN", &format!("{:02}", settings.tone.index))?;
        self.send_set("CT", mode)
    }

    pub fn select_memory_channel(&self, channel: u16) -> Result<()> {
        anyhow::ensure!(
            self.selected_profile()?.memory_surface != KenwoodMemorySurface::None && channel <= 119,
            "this Kenwood profile does not expose channels 0..119"
        );
        match self.selected_profile()?.memory_surface {
            KenwoodMemorySurface::Ts890 => {
                self.send_set("FR", "3")?;
                self.send_set("MN", &format!("{channel:03}"))
            }
            KenwoodMemorySurface::Ts590 => {
                self.send_set("FR", "2")?;
                self.send_set("MC", &format!("{channel:03}"))
            }
            KenwoodMemorySurface::None => unreachable!(),
        }
    }

    pub fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        anyhow::ensure!(
            self.selected_profile()?.memory_surface != KenwoodMemorySurface::None && channel <= 119,
            "this Kenwood profile does not expose memory records"
        );
        match self.selected_profile()?.memory_surface {
            KenwoodMemorySurface::Ts890 => {
                let response = self.query("MA0", Some(&format!("{channel:03}")), None)?;
                decode_ts890_memory(parse_payload(&response, "MA0")?, self.selected_profile()?)
            }
            KenwoodMemorySurface::Ts590 => {
                let response = self.query("MR", Some(&format!("0{channel:03}")), None)?;
                decode_ts590_memory(parse_payload(&response, "MR")?, self.selected_profile()?)
            }
            KenwoodMemorySurface::None => unreachable!(),
        }
    }

    pub fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        anyhow::ensure!(
            self.selected_profile()?.memory_surface != KenwoodMemorySurface::None
                && channel.channel <= 119,
            "this Kenwood profile does not expose memory writes"
        );
        anyhow::ensure!(
            channel.frequency_hz <= 99_999_999_999,
            "Kenwood memory frequency does not fit the documented 11-digit field"
        );
        if self.selected_profile()?.memory_surface == KenwoodMemorySurface::Ts590 {
            return self.write_ts590_memory_channel(channel);
        }
        anyhow::ensure!(
            matches!(channel.repeater.shift, RepeaterShift::Simplex),
            "Kenwood MA0 memory writes require simplex; split memory needs a separate TX frequency"
        );
        let mode = self.selected_profile()?.encode_mode(channel.mode)?;
        let tone_type = match channel.repeater.tone.mode {
            ToneMode::Off => '0',
            ToneMode::Encode => '1',
            ToneMode::EncodeDecode => '2',
            ToneMode::Dtcs => anyhow::bail!("DTCS is not supported by this Kenwood profile"),
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
        let params = format!(
            "{:03}{:011}{mode}0{tone_type}00{:02}{:011}{mode}000{name}",
            channel.channel, channel.frequency_hz, channel.repeater.tone.index, tx_frequency,
        );
        self.send_set("MA0", &params)
    }

    fn write_ts590_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        let mode = self.selected_profile()?.encode_mode(channel.mode)?;
        anyhow::ensure!(
            channel.repeater.tone.index <= 42,
            "Kenwood tone index must be 0..=42"
        );
        let split = channel.transmit_frequency_hz.is_some();
        let frequency = channel
            .transmit_frequency_hz
            .unwrap_or(channel.frequency_hz);
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
        let params = format!(
            "{}{channel_number:03}{frequency:011}{mode}0{tone}00{tone_index:02}00000000000000000{name}",
            if split { '1' } else { '0' },
            channel_number = channel.channel,
            tone_index = channel.repeater.tone.index,
        );
        self.send_set("MW", &params)?;
        Ok(())
    }

    fn selected_profile(&self) -> Result<&'static KenwoodCatProfile> {
        self.profile()
            .context("this operation requires a selected Kenwood model profile")
    }

    fn read_vfo(&self, command: &str) -> Result<u8> {
        match parse_payload(&self.query(command, None, Some(1))?, command)? {
            "0" => Ok(0),
            "1" => Ok(1),
            value => bail!("{command} reports non-VFO selection {value}; select VFO A or B first"),
        }
    }

    fn frequency_command(&self) -> Result<&'static str> {
        Ok(if self.read_vfo("FR")? == 0 {
            "FA"
        } else {
            "FB"
        })
    }

    fn get_if_status(&self) -> Result<KenwoodIfStatus> {
        if !self.selected_profile()?.supports_if_status {
            bail!("IF status is not documented for this Kenwood model");
        }
        decode_if_status(&self.query("IF", None, None)?)
    }

    fn query(
        &self,
        command: &str,
        parameters: Option<&str>,
        payload_len: Option<usize>,
    ) -> Result<Vec<u8>> {
        let request = ascii_cat::encode(command, parameters)?;
        let command = command.to_ascii_uppercase();
        self.transact(&request, |frame| {
            if !frame.starts_with(command.as_bytes()) || frame.last() != Some(&b';') {
                return false;
            }
            payload_len.is_none_or(|len| frame.len() == command.len() + len + 1)
        })
    }

    fn write_only(&self, request: &[u8]) -> Result<()> {
        validate_complete_command(request)?;
        self.with_transport(Duration::from_millis(750), |state| {
            active_transport(state)?
                .write_all(request)
                .context("failed to write Kenwood CAT command")?;
            active_transport(state)?
                .flush()
                .context("failed to flush Kenwood CAT command")
        })
    }

    fn transact<F>(&self, request: &[u8], mut matcher: F) -> Result<Vec<u8>>
    where
        F: FnMut(&[u8]) -> bool,
    {
        validate_complete_command(request)?;
        self.with_transport(Duration::from_millis(150), |state| {
            active_transport(state)?
                .write_all(request)
                .context("failed to write Kenwood CAT command")?;
            active_transport(state)?
                .flush()
                .context("failed to flush Kenwood CAT command")?;

            let deadline = Instant::now() + RESPONSE_TIMEOUT;
            let mut buffer = [0_u8; 256];
            while Instant::now() < deadline {
                for frame in take_complete_frames(&mut state.pending) {
                    match frame.as_slice() {
                        b"?;" => bail!("Kenwood CAT rejected {}", display_command(request)),
                        b"E;" => bail!(
                            "Kenwood CAT communication error after {}",
                            display_command(request)
                        ),
                        b"O;" => bail!(
                            "Kenwood CAT receive-buffer overrun after {}",
                            display_command(request)
                        ),
                        _ if matcher(&frame) => return Ok(frame),
                        _ => self.publish_unsolicited(&frame),
                    }
                }

                match active_transport(state)?.read(&mut buffer) {
                    Ok(count) if count > 0 => state.pending.extend_from_slice(&buffer[..count]),
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::TimedOut => {}
                    Err(error) => return Err(error).context("failed to read Kenwood CAT response"),
                }
                if state.pending.len() > MAX_FRAME_LEN * 4 {
                    bail!("Kenwood CAT receive buffer exceeded safety limit");
                }
                thread::sleep(Duration::from_millis(5));
            }
            bail!(
                "timed out waiting for Kenwood CAT response to {}",
                display_command(request)
            )
        })
    }

    fn with_transport<T, F>(&self, timeout: Duration, operation: F) -> Result<T>
    where
        F: FnOnce(&mut TransportState) -> Result<T>,
    {
        if self.port.trim().is_empty()
            && self
                .transport
                .lock()
                .map_err(|_| anyhow!("Kenwood CAT transport lock poisoned"))?
                .external
                .is_none()
        {
            bail!("a serial port is required for Kenwood PC control");
        }
        let mut state = self
            .transport
            .lock()
            .map_err(|_| anyhow!("Kenwood CAT transport lock poisoned"))?;
        if state.port.is_none() && state.external.is_none() {
            let stop_bits = if self.baud_rate == 4_800 {
                StopBits::Two
            } else {
                StopBits::One
            };
            state.port = Some(Box::new(SerialPortTransport(
                serialport::new(&self.port, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(stop_bits)
                    .flow_control(FlowControl::None)
                    .timeout(timeout)
                    .open()
                    .with_context(|| format!("failed to open Kenwood CAT port {}", self.port))?,
            )));
        }
        active_transport(&mut state)?
            .set_timeout(timeout)
            .context("failed to update Kenwood CAT timeout")?;
        let result = operation(&mut state);
        if result.is_err() {
            state.port = None;
            state.pending.clear();
        }
        result
    }

    fn publish_unsolicited(&self, frame: &[u8]) {
        // Keep the raw frame available even when a model-specific command is
        // not yet mapped to a typed HAL event. This is important with AI on:
        // silently discarding state changes makes polling clients stale.
        let Some(payload) = frame.strip_suffix(b";") else {
            return;
        };
        if payload.starts_with(b"FA") || payload.starts_with(b"FB") {
            if payload.len() == 13 {
                if let Ok(text) = std::str::from_utf8(&payload[2..]) {
                    if let Ok(frequency_hz) = text.parse() {
                        self.event_router
                            .publish(RadioEvent::FrequencyChanged { frequency_hz });
                        return;
                    }
                }
            }
        } else if payload == b"TX0" || payload == b"TX1" {
            self.event_router.publish(RadioEvent::PttChanged {
                enabled: payload[2] == b'1',
            });
            return;
        } else if payload == b"RX" {
            self.event_router
                .publish(RadioEvent::PttChanged { enabled: false });
            return;
        }
        self.event_router.publish(RadioEvent::Raw {
            payload: frame.to_vec(),
        });
    }

    fn watts_to_normalized(&self, watts: u16) -> Result<u8> {
        let (minimum, maximum) = self
            .selected_profile()?
            .power_range_watts
            .context("RF power control is not profiled for this Kenwood model")?;
        if !(minimum..=maximum).contains(&watts) {
            bail!("CAT power response is outside the profiled range: {watts} W");
        }
        let span = u32::from(maximum - minimum);
        Ok((((u32::from(watts - minimum) * 255) + span / 2) / span) as u8)
    }

    fn normalized_to_watts(&self, level: u8) -> Result<u16> {
        let (minimum, maximum) = self
            .selected_profile()?
            .power_range_watts
            .context("RF power control is not profiled for this Kenwood model")?;
        let span = u32::from(maximum - minimum);
        Ok(minimum + (((u32::from(level) * span) + 127) / 255) as u16)
    }
}

#[async_trait]
impl Radio for KenwoodCatRadio {
    fn event_router(&self) -> Option<RadioEventRouter> {
        Some(self.event_router.clone())
    }

    async fn get_frequency_hz(&self) -> Result<u64> {
        let command = self.frequency_command()?;
        parse_payload(&self.query(command, None, Some(11))?, command)?
            .parse()
            .context("invalid Kenwood frequency response")
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        if hz > 99_999_999_999 {
            bail!("frequency {hz} Hz does not fit Kenwood's eleven-digit field");
        }
        if let Some(profile) = self.profile() {
            if !profile.supports_frequency(hz) {
                bail!(
                    "frequency {hz} Hz is outside the documented PC-control range for {}",
                    profile.model.model_name()
                );
            }
        }
        let command = self.frequency_command()?;
        self.send_set(command, &format!("{hz:011}"))
    }

    async fn get_mode(&self) -> Result<Mode> {
        let profile = self.selected_profile()?;
        match profile.mode_command {
            KenwoodModeCommand::Md { supports_data_flag } => {
                let response = self.query("MD", None, Some(1))?;
                let payload = parse_payload(&response, "MD")?;
                let code = one_char(payload, "Kenwood MD response")?;
                let base = profile.decode_mode(code)?;
                if supports_data_flag
                    && matches!(base, Mode::Lsb | Mode::Usb | Mode::Fm | Mode::Am)
                    && parse_bool_payload(&self.query("DA", None, Some(1))?, "DA")?
                {
                    Ok(Mode::Data)
                } else {
                    Ok(base)
                }
            }
            KenwoodModeCommand::Om => {
                let selector = self.read_vfo("FR")?;
                let response = self.query("OM", Some(&selector.to_string()), Some(2))?;
                let payload = parse_payload(&response, "OM")?;
                if payload.as_bytes().first() != Some(&(b'0' + selector)) {
                    bail!("unexpected Kenwood OM VFO selector: {payload}");
                }
                profile.decode_mode(one_char(&payload[1..], "Kenwood OM response")?)
            }
        }
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let profile = self.selected_profile()?;
        match profile.mode_command {
            KenwoodModeCommand::Md { supports_data_flag } => {
                if mode == Mode::Data && supports_data_flag {
                    self.send_set("MD", "2")?;
                    self.send_set("DA", "1")
                } else {
                    self.send_set("MD", &profile.encode_mode(mode)?.to_string())?;
                    if supports_data_flag
                        && matches!(mode, Mode::Lsb | Mode::Usb | Mode::Fm | Mode::Am)
                    {
                        self.send_set("DA", "0")?;
                    }
                    Ok(())
                }
            }
            KenwoodModeCommand::Om => {
                self.send_set("OM", &format!("0{}", profile.encode_mode(mode)?))
            }
        }
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        if enabled {
            self.send_set("TX", "0")?;
        } else {
            self.send_set("RX", "")?;
        }
        if self.selected_profile()?.supports_if_status {
            let actual = self.get_if_status()?.transmitting;
            if actual != enabled {
                bail!(
                    "Kenwood radio did not enter the requested {} state",
                    if enabled { "transmit" } else { "receive" }
                );
            }
        }
        Ok(())
    }

    async fn get_ptt(&self) -> Result<bool> {
        Ok(self.get_if_status()?.transmitting)
    }

    async fn get_power(&self) -> Result<bool> {
        self.get_power_state()
    }

    async fn set_power(&self, enabled: bool) -> Result<()> {
        self.set_power_state(enabled)
    }

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        validate_complete_command(request)?;
        let command = std::str::from_utf8(&request[..2]).context("CAT command is not ASCII")?;
        self.transact(request, |frame| frame.starts_with(command.as_bytes()))
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        match id {
            ControlId::AfGain | ControlId::RfGain | ControlId::Squelch => {
                Ok(Some(ControlValue::U8(self.get_level_control(id)?)))
            }
            ControlId::NoiseReduction | ControlId::Notch | ControlId::Preamp => {
                let spec = self
                    .selected_profile()?
                    .control(id)
                    .context("Kenwood control is not profiled")?;
                Ok(Some(if id == ControlId::Preamp {
                    ControlValue::U8(
                        self.get_flag_or_level_with_len(spec.command, spec.response_len)?,
                    )
                } else {
                    ControlValue::Bool(
                        self.get_flag_or_level_with_len(spec.command, spec.response_len)? != 0,
                    )
                }))
            }
            ControlId::NoiseBlanker => Ok(Some(ControlValue::Bool(self.get_noise_blanker()?))),
            ControlId::Filter => Ok(Some(ControlValue::U8(self.get_filter()?))),
            ControlId::Agc if self.selected_profile()?.control(ControlId::Agc).is_some() => {
                Ok(Some(ControlValue::U8(self.get_agc()?)))
            }
            ControlId::Rit => Ok(Some(ControlValue::Bool(self.read_if_rit_xit()?.1))),
            ControlId::Xit => Ok(Some(ControlValue::Bool(self.read_if_rit_xit()?.2))),
            ControlId::Vfo => Ok(Some(ControlValue::Vfo(self.read_vfo("FR")?))),
            ControlId::RfPower => Ok(Some(ControlValue::U8(
                self.watts_to_normalized(self.get_power_watts()?)?,
            ))),
            ControlId::Split => Ok(Some(ControlValue::Bool(self.get_split()?))),
            _ => Ok(None),
        }
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        match id {
            MeterId::Signal | MeterId::Power => Ok(Some(self.get_signal_meter()?)),
            MeterId::Swr => Ok(Some(self.get_swr_meter()?)),
            id => {
                let spec = self
                    .selected_profile()?
                    .meter(id)
                    .context("Kenwood meter is not profiled")?;
                Ok(Some(
                    if self.selected_profile()?.swr_meter_requires_selection {
                        self.get_ts890_meter(spec.selector)?
                    } else {
                        self.get_rm_meter(spec.selector, spec.maximum)?
                    },
                ))
            }
        }
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        self.profile().is_some_and(|profile| {
            matches!(id, MeterId::Signal | MeterId::Power | MeterId::Swr)
                || profile.meter(id).is_some()
        })
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        match (id, value) {
            (ControlId::AfGain, ControlValue::U8(value))
            | (ControlId::RfGain, ControlValue::U8(value))
            | (ControlId::Squelch, ControlValue::U8(value)) => self.set_level_control(id, value),
            (ControlId::Preamp, ControlValue::U8(value)) => {
                let spec = self
                    .selected_profile()?
                    .control(id)
                    .context("Kenwood control is not profiled")?;
                anyhow::ensure!(
                    spec.max_value.is_none_or(|maximum| value <= maximum),
                    "Kenwood control value is outside the model profile"
                );
                self.set_flag_or_level(spec.command, value)
            }
            (ControlId::NoiseReduction, ControlValue::Bool(enabled))
            | (ControlId::Notch, ControlValue::Bool(enabled)) => {
                let spec = self
                    .selected_profile()?
                    .control(id)
                    .context("Kenwood control is not profiled")?;
                self.set_flag_or_level(spec.command, u8::from(enabled))
            }
            (ControlId::NoiseBlanker, ControlValue::Bool(enabled)) => {
                self.set_noise_blanker(enabled)
            }
            (ControlId::Filter, ControlValue::U8(value)) => self.set_filter(value),
            (ControlId::Agc, ControlValue::U8(value)) => {
                let spec = self
                    .selected_profile()?
                    .control(ControlId::Agc)
                    .context("AGC is not profiled")?;
                anyhow::ensure!(
                    spec.max_value.is_none_or(|maximum| value <= maximum),
                    "Kenwood AGC value is outside the model profile"
                );
                self.set_flag_or_level(spec.command, value)
            }
            (ControlId::Rit, ControlValue::Bool(enabled)) => {
                let command = self
                    .selected_profile()?
                    .control(ControlId::Rit)
                    .context("RIT is not profiled")?
                    .command;
                self.send_set(command, u8::from(enabled).to_string().as_str())
            }
            (ControlId::Xit, ControlValue::Bool(enabled)) => {
                let command = self
                    .selected_profile()?
                    .control(ControlId::Xit)
                    .context("XIT is not profiled")?
                    .command;
                self.send_set(command, u8::from(enabled).to_string().as_str())
            }
            (ControlId::Vfo, ControlValue::Vfo(vfo)) if vfo <= 1 => {
                self.send_set("FR", &vfo.to_string())
            }
            (ControlId::RfPower, ControlValue::U8(level)) => {
                self.set_power_watts(self.normalized_to_watts(level)?)
            }
            (ControlId::Split, ControlValue::Bool(enabled)) => self.set_split(enabled),
            (_, value) => bail!("unsupported Kenwood CAT control/value: {id:?} = {value:?}"),
        }
    }

    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        KenwoodCatRadio::get_repeater_settings(self)
    }

    async fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        KenwoodCatRadio::set_repeater_settings(self, settings)
    }

    async fn get_rit_offset_hz(&self) -> Result<i32> {
        KenwoodCatRadio::get_rit_offset_hz(self)
    }

    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        KenwoodCatRadio::set_rit_offset_hz(self, offset_hz)
    }

    async fn get_xit_offset_hz(&self) -> Result<i32> {
        KenwoodCatRadio::get_xit_offset_hz(self)
    }

    async fn set_xit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        KenwoodCatRadio::set_xit_offset_hz(self, offset_hz)
    }

    async fn start_tuner(&self) -> Result<()> {
        KenwoodCatRadio::start_tuner(self)
    }

    async fn get_tuner_status(&self) -> Result<Option<crate::hal_types::TunerStatus>> {
        Ok(Some(KenwoodCatRadio::get_tuner_status(self)?))
    }

    async fn select_memory_channel(&self, channel: u16) -> Result<()> {
        KenwoodCatRadio::select_memory_channel(self, channel)
    }

    async fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        KenwoodCatRadio::read_memory_channel(self, channel)
    }

    async fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        KenwoodCatRadio::write_memory_channel(self, channel)
    }

    fn supports_memory_channels(&self) -> bool {
        self.profile()
            .is_some_and(|profile| profile.memory_surface != KenwoodMemorySurface::None)
    }

    fn supports_repeater_settings(&self) -> bool {
        self.model().is_some()
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.profile().is_some_and(|profile| {
            ((matches!(
                id,
                ControlId::AfGain | ControlId::RfGain | ControlId::Squelch
            )) || (id == ControlId::RfPower && profile.power_range_watts.is_some()))
                || profile.control(id).is_some()
                || id == ControlId::Vfo
                || id == ControlId::Split
        })
    }

    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: self.model.is_some(),
            can_set_mode: self.model.is_some(),
            can_get_ptt: self
                .profile()
                .is_some_and(|profile| profile.supports_if_status),
            can_set_ptt: true,
            can_get_power: true,
            can_set_power: true,
            can_raw_protocol: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KenwoodIfStatus {
    transmitting: bool,
}

fn decode_if_status(frame: &[u8]) -> Result<KenwoodIfStatus> {
    let payload = parse_payload(frame, "IF")?;
    if payload.len() < 27 || !payload.is_ascii() {
        bail!("Kenwood IF response is too short");
    }
    let transmitting = match payload.as_bytes()[26] {
        b'0' => false,
        b'1' => true,
        value => bail!("invalid Kenwood IF RX/TX flag: {value:#04x}"),
    };
    Ok(KenwoodIfStatus { transmitting })
}

fn parse_bool_payload(frame: &[u8], command: &str) -> Result<bool> {
    match parse_payload(frame, command)? {
        "0" => Ok(false),
        "1" => Ok(true),
        value => bail!("invalid Kenwood {command} response: {value}"),
    }
}

fn decode_ts890_memory(payload: &str, profile: &KenwoodCatProfile) -> Result<MemoryChannel> {
    anyhow::ensure!(payload.len() >= 36, "TS-890S MA0 response is too short");
    let channel = parse_field::<u16>(&payload[0..3], "memory channel")?;
    let frequency_hz = parse_field::<u64>(&payload[3..14], "memory frequency")?;
    let mode = profile.decode_mode(one_char(&payload[14..15], "memory mode")?)?;
    let tone_mode = match &payload[16..17] {
        "0" => ToneMode::Off,
        "1" => ToneMode::Encode,
        "2" | "3" => ToneMode::EncodeDecode,
        value => bail!("invalid TS-890S memory tone mode: {value}"),
    };
    let tone_index = parse_field::<u8>(&payload[19..21], "memory CTCSS index")?;
    let tx_frequency = parse_field::<u64>(&payload[21..32], "memory transmit frequency")?;
    // MA0 P10 is FM narrow/normal, not repeater-shift direction. Split is
    // represented independently by P11 and the TX frequency in P8.
    let shift = RepeaterShift::Simplex;
    let name = payload[36..].trim().to_owned();
    Ok(MemoryChannel {
        channel,
        name: (!name.is_empty()).then_some(name),
        frequency_hz,
        transmit_frequency_hz: (tx_frequency != 0).then_some(tx_frequency),
        mode,
        repeater: RepeaterSettings {
            shift,
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

fn decode_ts590_memory(payload: &str, profile: &KenwoodCatProfile) -> Result<MemoryChannel> {
    // MR payload: P1, three-digit channel, 11-digit frequency, mode/data,
    // tone fields, filter state, and an optional eight-character name.
    anyhow::ensure!(payload.len() >= 39, "TS-590SG MR response is too short");
    let split = payload.as_bytes()[0] == b'1';
    let channel = parse_field::<u16>(&payload[1..4], "memory channel")?;
    let frequency_hz = parse_field::<u64>(&payload[4..15], "memory frequency")?;
    let mode = profile.decode_mode(one_char(&payload[15..16], "memory mode")?)?;
    let tone_mode = match &payload[17..18] {
        "0" => ToneMode::Off,
        "1" => ToneMode::Encode,
        "2" | "3" => ToneMode::EncodeDecode,
        value => bail!("invalid TS-590SG memory tone mode: {value}"),
    };
    let tone_index = parse_field::<u8>(&payload[20..22], "memory CTCSS index")?;
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

fn parse_field<T>(value: &str, label: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .trim()
        .parse()
        .map_err(|error| anyhow!("invalid {label}: {error}"))
}

fn one_char(payload: &str, context: &str) -> Result<char> {
    let mut chars = payload.chars();
    let value = chars.next().with_context(|| format!("missing {context}"))?;
    if chars.next().is_some() {
        bail!("unexpected {context}: {payload}");
    }
    Ok(value)
}

fn parse_payload<'a>(frame: &'a [u8], command: &str) -> Result<&'a str> {
    let text = std::str::from_utf8(frame).context("Kenwood CAT response is not ASCII")?;
    text.strip_prefix(command)
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected Kenwood CAT response")
}

fn validate_complete_command(command: &[u8]) -> Result<()> {
    if command.len() < 3
        || command.last() != Some(&b';')
        || command[..command.len() - 1].contains(&b';')
        || !command.is_ascii()
        || !command[..2].iter().all(u8::is_ascii_alphabetic)
    {
        bail!("expected one complete semicolon-terminated Kenwood CAT command");
    }
    Ok(())
}

fn take_complete_frames(pending: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let (frames, tail) = ascii_cat::decode_frames(pending);
    *pending = tail;
    frames
        .into_iter()
        .filter(|frame| frame.len() <= MAX_FRAME_LEN)
        .collect()
}

fn display_command(request: &[u8]) -> String {
    String::from_utf8_lossy(request).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kenwood::profile::{TS590SG_PROFILE, TS890S_PROFILE};

    #[test]
    fn constructors_select_exact_profiles_and_validate_baud() {
        let radio =
            KenwoodCatRadio::new_for_model(KenwoodCatModel::Ts890S, "test", 115_200).unwrap();
        assert_eq!(radio.model(), Some(KenwoodCatModel::Ts890S));
        assert_eq!(radio.profile().unwrap().id_code, "024");
        assert!(KenwoodCatRadio::new_for_model(KenwoodCatModel::Ts2000, "test", 115_200).is_err());
    }

    #[test]
    fn commands_match_official_manual_examples() {
        assert_eq!(
            ascii_cat::encode("FA", Some("00007000000")).unwrap(),
            b"FA00007000000;"
        );
        assert_eq!(ascii_cat::encode("MD", Some("2")).unwrap(), b"MD2;");
        assert_eq!(ascii_cat::encode("OM", Some("0D")).unwrap(), b"OM0D;");
        assert_eq!(ascii_cat::encode("TX", Some("0")).unwrap(), b"TX0;");
    }

    #[test]
    fn if_status_uses_the_documented_rx_tx_field() {
        let mut payload = vec![b'0'; 35];
        payload[26] = b'0';
        let mut frame = b"IF".to_vec();
        frame.extend_from_slice(&payload);
        frame.push(b';');
        assert!(!decode_if_status(&frame).unwrap().transmitting);
        frame[28] = b'1';
        assert!(decode_if_status(&frame).unwrap().transmitting);
    }

    #[test]
    fn decodes_documented_ts890_memory_record() {
        let payload = format!(
            "{:03}{:011}{}0{}00{:02}{:011}{}000{}",
            1, 14_500_000, '4', '1', 8, 14_500_000, '4', "LOCAL"
        );
        let channel = decode_ts890_memory(&payload, &TS890S_PROFILE).unwrap();
        assert_eq!(channel.channel, 1);
        assert_eq!(channel.frequency_hz, 14_500_000);
        assert_eq!(channel.mode, Mode::Fm);
        assert_eq!(channel.repeater.tone.mode, ToneMode::Encode);
        assert_eq!(channel.repeater.tone.index, 8);
        assert_eq!(channel.name.as_deref(), Some("LOCAL"));
    }

    #[test]
    fn decodes_documented_ts590_memory_record() {
        let payload = format!(
            "1{:03}{:011}{}0{}00{:02}00000000000000000{}",
            7, 14_074_000, '2', '2', 8, "LOCAL"
        );
        let channel = decode_ts590_memory(&payload, &TS590SG_PROFILE).unwrap();
        assert_eq!(channel.channel, 7);
        assert_eq!(channel.frequency_hz, 14_074_000);
        assert_eq!(channel.transmit_frequency_hz, Some(14_074_000));
        assert_eq!(channel.mode, Mode::Usb);
        assert_eq!(channel.repeater.tone.mode, ToneMode::EncodeDecode);
        assert_eq!(channel.repeater.tone.index, 8);
        assert_eq!(channel.name.as_deref(), Some("LOCAL"));
    }

    #[test]
    fn parser_preserves_partial_auto_information_frames() {
        let mut pending = b"FA00014074000;MD2;IF000".to_vec();
        let frames = take_complete_frames(&mut pending);
        assert_eq!(frames, [b"FA00014074000;".to_vec(), b"MD2;".to_vec()]);
        assert_eq!(pending, b"IF000");
    }

    #[test]
    fn raw_commands_are_single_complete_frames() {
        assert!(validate_complete_command(b"ID;").is_ok());
        assert!(validate_complete_command(b"ID").is_err());
        assert!(validate_complete_command(b"ID;FA;").is_err());
    }
}
