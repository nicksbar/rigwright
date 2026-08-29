//! Model-neutral transport for modern Yaesu ASCII CAT radios.

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
    hal::{Mode, Radio, RadioCapabilities},
    hal_types::{
        ControlId, ControlValue, MemoryChannel, MeterId, RepeaterSettings, RepeaterShift, ToneMode,
        ToneSettings,
    },
    models::YaesuCatModel,
    protocol::ascii_cat,
    transport::{RadioTransport, SerialPortTransport},
};

use super::profile::{profile_for_model, YaesuCatProfile};

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
        bail!("Yaesu CAT port unavailable")
    }
}

/// Modern semicolon-terminated Yaesu CAT driver.
///
/// This type owns serial transport, framing, response matching, and the common
/// `FA`, `MD`, `TX`, `PC`, and `ST` operations. Model-specific command groups
/// remain in their model modules. The connection is reused between operations
/// so rapid polling does not repeatedly reopen the USB serial device.
#[derive(Clone)]
pub struct YaesuCatRadio {
    model: Option<YaesuCatModel>,
    port: String,
    baud_rate: u32,
    hardware_flow_control: bool,
    cat_rts_detected: Arc<Mutex<bool>>,
    transport: Arc<Mutex<TransportState>>,
}

impl std::fmt::Debug for YaesuCatRadio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("YaesuCatRadio")
            .field("model", &self.model)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .field("hardware_flow_control", &self.hardware_flow_control)
            .finish_non_exhaustive()
    }
}

impl YaesuCatRadio {
    /// Construct a model-neutral modern Yaesu CAT driver.
    ///
    /// This is useful for probing. Model-specific range checks, ID checks, and
    /// controls require [`Self::new_for_model`].
    pub fn new_generic(port: impl Into<String>, baud_rate: u32) -> Self {
        Self::new_internal(None, port, baud_rate)
    }

    /// Construct a modern Yaesu driver using a declarative model profile.
    pub fn new_for_model(
        model: YaesuCatModel,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Result<Self> {
        let profile = profile_for_model(model);
        if !profile.baud_rates.contains(&baud_rate) {
            bail!(
                "unsupported CAT baud rate {baud_rate} for {}; documented rates: {:?}",
                model.model_name(),
                profile.baud_rates
            );
        }
        Ok(Self::new_internal(Some(model), port, baud_rate))
    }

    /// Construct a model-backed radio for interfaces where the radio's CAT
    /// RTS setting is enabled and the USB/serial bridge requires RTS/CTS
    /// hardware flow control. The normal constructor remains no-flow-control
    /// because that is the FTDX10 factory/default configuration.
    pub fn new_for_model_with_hardware_flow_control(
        model: YaesuCatModel,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Result<Self> {
        let mut radio = Self::new_for_model(model, port, baud_rate)?;
        radio.hardware_flow_control = true;
        Ok(radio)
    }

    /// Construct a modern Yaesu CAT radio over an externally configured byte
    /// transport, such as Android USB Host or Bluetooth.
    pub fn with_external_transport<T>(
        model: Option<YaesuCatModel>,
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
            hardware_flow_control: false,
            cat_rts_detected: Arc::new(Mutex::new(false)),
        }
    }

    fn new_internal(model: Option<YaesuCatModel>, port: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            model,
            port: port.into(),
            baud_rate,
            hardware_flow_control: false,
            cat_rts_detected: Arc::new(Mutex::new(false)),
            transport: Arc::new(Mutex::new(TransportState::default())),
        }
    }

    pub fn model(&self) -> Option<YaesuCatModel> {
        self.model
    }

    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    pub fn profile(&self) -> Option<&'static YaesuCatProfile> {
        self.model.map(profile_for_model)
    }

    /// Query `ID;` and reject a radio that does not match the selected profile.
    pub fn verify_model(&self) -> Result<()> {
        let profile = self.selected_profile()?;
        let response = self.query("ID", None, 4)?;
        let id = parse_payload(&response, "ID")?;
        if id != profile.id_code {
            bail!(
                "CAT radio identified as {id}, expected {} ({})",
                profile.id_code,
                profile.model.model_name()
            );
        }
        Ok(())
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.transport.lock() {
            state.port = None;
            state.external = None;
            state.pending.clear();
        }
    }

    /// Send a documented set command. Parameters are validated by the shared
    /// CAT encoder, but their model-specific meaning remains the caller's
    /// responsibility.
    pub fn send_set(&self, command: &str, parameters: &str) -> Result<()> {
        let request = ascii_cat::encode(command, Some(parameters))?;
        self.write_only(&request)
    }

    /// Send one prebuilt set command, such as a typed helper returned by a
    /// model module. This does not wait for a reply because Yaesu set commands
    /// normally do not return one.
    pub fn send_raw(&self, request: &[u8]) -> Result<()> {
        self.write_only(request)
    }

    /// Send a documented read command and return its complete CAT frame.
    pub fn query_raw(&self, command: &str, parameters: Option<&str>) -> Result<Vec<u8>> {
        self.query(command, parameters, 0)
    }

    pub fn get_power_watts(&self) -> Result<u16> {
        let profile = self.selected_profile()?;
        profile
            .power_range_watts
            .context("RF power control is not profiled for this Yaesu model")?;
        parse_payload(&self.query("PC", None, 3)?, "PC")?
            .parse()
            .context("invalid Yaesu PC response")
    }

    pub fn set_power_watts(&self, watts: u16) -> Result<()> {
        self.selected_profile()?.validate_power(watts)?;
        self.send_set("PC", &format!("{watts:03}"))
    }

    pub fn get_power_state(&self) -> Result<bool> {
        match parse_payload(&self.query("PS", None, 1)?, "PS")? {
            "0" => Ok(false),
            "1" => Ok(true),
            value => bail!("invalid Yaesu PS response: {value}"),
        }
    }

    pub fn set_power_state(&self, enabled: bool) -> Result<()> {
        self.send_set("PS", if enabled { "1" } else { "0" })
    }

    pub fn get_split(&self) -> Result<bool> {
        if !self.selected_profile()?.supports_split {
            bail!("split control is not profiled for this Yaesu model");
        }
        match parse_payload(&self.query("ST", None, 1)?, "ST")? {
            "0" => Ok(false),
            "1" | "2" => Ok(true),
            value => bail!("invalid Yaesu ST response: {value}"),
        }
    }

    pub fn set_split(&self, enabled: bool) -> Result<()> {
        if !self.selected_profile()?.supports_split {
            bail!("split control is not profiled for this Yaesu model");
        }
        self.send_set("ST", if enabled { "1" } else { "0" })
    }

    /// Read the FTDX10-family documented repeater controls.  The CAT manual
    /// exposes tone as an index and offset as a direction; those native forms
    /// are preserved in the protocol-neutral value instead of inventing Hz.
    pub fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        if !self.selected_profile()?.supports_repeater_settings {
            bail!("repeater settings are not documented for this Yaesu model");
        }
        let tone_index = parse_payload(&self.query("CN", None, 3)?, "CN")?
            .parse::<u8>()
            .context("invalid Yaesu CN tone index")?;
        let tone_mode = match parse_payload(&self.query("CT", None, 1)?, "CT")? {
            "0" => ToneMode::Off,
            "1" => ToneMode::EncodeDecode,
            "2" => ToneMode::Encode,
            value => bail!("invalid Yaesu CT tone mode: {value}"),
        };
        let shift = match parse_payload(&self.query("OS", None, 1)?, "OS")? {
            "0" => RepeaterShift::Simplex,
            "1" => RepeaterShift::Plus,
            "2" => RepeaterShift::Minus,
            value => bail!("invalid Yaesu OS repeater shift: {value}"),
        };
        Ok(RepeaterSettings {
            shift,
            offset_hz: None,
            tone: ToneSettings {
                mode: tone_mode,
                index: tone_index,
                frequency_tenths_hz: None,
                dtcs_code: None,
                dtcs_reverse: None,
            },
        })
    }

    pub fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        if !self.selected_profile()?.supports_repeater_settings {
            bail!("repeater settings are not documented for this Yaesu model");
        }
        if settings.tone.index > 49 {
            bail!("Yaesu FTDX10 tone index must be 0..=49");
        }
        let tone_mode = match settings.tone.mode {
            ToneMode::Off => "0",
            ToneMode::EncodeDecode => "1",
            ToneMode::Encode => "2",
            ToneMode::Dtcs => anyhow::bail!("DTCS is not supported by this Yaesu profile"),
        };
        let shift = match settings.shift {
            RepeaterShift::Simplex => "0",
            RepeaterShift::Plus => "1",
            RepeaterShift::Minus => "2",
        };
        self.send_set("CN", &format!("{:03}", settings.tone.index))?;
        self.send_set("CT", tone_mode)?;
        self.send_set("OS", shift)
    }

    /// Select a documented FTDX10 memory channel. Read/write of the complete
    /// `MR`/`MT` records remains a separate model-specific operation.
    pub fn select_memory_channel(&self, channel: u16) -> Result<()> {
        if !self.selected_profile()?.supports_repeater_settings || channel > 99 {
            bail!("memory channel selection is not documented for this Yaesu model");
        }
        self.send_set("MC", &format!("{channel:03}"))
    }

    pub fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        anyhow::ensure!(
            self.model() == Some(YaesuCatModel::Ftdx10) && (1..=99).contains(&channel),
            "FTDX10 memory records are the only Yaesu memory surface currently profiled"
        );
        let response = self.query("MR", Some(&format!("{channel:03}")), 0)?;
        decode_ftdx10_memory(parse_payload(&response, "MR")?, self.selected_profile()?)
    }

    pub fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        anyhow::ensure!(
            self.model() == Some(YaesuCatModel::Ftdx10) && (1..=99).contains(&channel.channel),
            "FTDX10 memory records are the only Yaesu memory surface currently profiled"
        );
        anyhow::ensure!(
            channel.frequency_hz <= 999_999_999,
            "FTDX10 memory frequency exceeds CAT width"
        );
        let mode = self.selected_profile()?.encode_mode(channel.mode)?;
        let tone = match channel.repeater.tone.mode {
            ToneMode::Off => '0',
            ToneMode::EncodeDecode => '1',
            ToneMode::Encode => '2',
            ToneMode::Dtcs => anyhow::bail!("DTCS is not supported by this Yaesu profile"),
        };
        let shift = match channel.repeater.shift {
            RepeaterShift::Simplex => '0',
            RepeaterShift::Plus => '1',
            RepeaterShift::Minus => '2',
        };
        let offset = channel.repeater.offset_hz.unwrap_or(0).min(9990);
        let name = channel.name.unwrap_or_default();
        anyhow::ensure!(
            name.is_ascii() && name.len() <= 12,
            "FTDX10 memory tag must be ASCII and at most 12 characters"
        );
        let params = format!(
            "0{:03}{:09}+{:04}00{}0{}00{}0{}",
            channel.channel, channel.frequency_hz, offset, mode, tone, shift, name
        );
        self.send_set("MT", &params)
    }

    fn selected_profile(&self) -> Result<&'static YaesuCatProfile> {
        self.profile()
            .context("this operation requires a selected Yaesu model profile")
    }

    fn query(
        &self,
        command: &str,
        parameters: Option<&str>,
        payload_len: usize,
    ) -> Result<Vec<u8>> {
        self.ensure_cat_rts_detected(command)?;
        let request = ascii_cat::encode(command, parameters)?;
        let command = command.to_ascii_uppercase();
        self.transact(&request, |frame| {
            if !frame.starts_with(command.as_bytes()) || frame.last() != Some(&b';') {
                return false;
            }
            payload_len == 0 || frame.len() == command.len() + payload_len + 1
        })
    }

    /// FTDX10 exposes its CAT RTS setting as EX 03-03-10. Read it once over
    /// CAT and adapt the serial adapter before issuing ordinary queries.
    /// Other Yaesu profiles do not share this menu address and retain their
    /// configured/default transport behavior.
    fn ensure_cat_rts_detected(&self, command: &str) -> Result<()> {
        if command.eq_ignore_ascii_case("EX") || self.model != Some(YaesuCatModel::Ftdx10) {
            return Ok(());
        }
        if self
            .cat_rts_detected
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT RTS state lock poisoned"))?
            .to_owned()
        {
            return Ok(());
        }

        let response = self.query("EX", Some("030310"), 7);
        let enabled = response
            .ok()
            .and_then(|frame| parse_payload(&frame, "EX").ok().map(str::to_owned))
            .and_then(|payload| payload.chars().last())
            .is_some_and(|value| value == '1');
        if enabled && !self.hardware_flow_control {
            self.with_transport(Duration::from_millis(150), |state| {
                active_transport(state)?
                    .set_hardware_flow_control(true)
                    .context("failed to enable Yaesu CAT RTS/CTS flow control")
            })?;
        }
        *self
            .cat_rts_detected
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT RTS state lock poisoned"))? = true;
        Ok(())
    }

    fn write_only(&self, request: &[u8]) -> Result<()> {
        validate_complete_command(request)?;
        self.with_transport(Duration::from_millis(750), |state| {
            active_transport(state)?
                .write_all(request)
                .context("failed to write Yaesu CAT command")?;
            active_transport(state)?
                .flush()
                .context("failed to flush Yaesu CAT command")
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
                .context("failed to write Yaesu CAT command")?;
            active_transport(state)?
                .flush()
                .context("failed to flush Yaesu CAT command")?;

            let deadline = Instant::now() + RESPONSE_TIMEOUT;
            let mut buffer = [0_u8; 256];
            while Instant::now() < deadline {
                for frame in take_complete_frames(&mut state.pending) {
                    if frame == b"?;" {
                        bail!("Yaesu CAT rejected command {}", display_command(request));
                    }
                    if matcher(&frame) {
                        return Ok(frame);
                    }
                    // Auto-information frames and replies for other commands
                    // may be interleaved. They are intentionally ignored here.
                }

                match active_transport(state)?.read(&mut buffer) {
                    Ok(count) if count > 0 => {
                        state.pending.extend_from_slice(&buffer[..count]);
                        if state.pending.len() > MAX_FRAME_LEN * 4 {
                            bail!("Yaesu CAT receive buffer exceeded safety limit");
                        }
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::TimedOut => {}
                    Err(error) => return Err(error).context("failed to read Yaesu CAT response"),
                }
                thread::sleep(Duration::from_millis(5));
            }
            bail!(
                "timed out waiting for Yaesu CAT response to {}",
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
                .map_err(|_| anyhow!("Yaesu CAT transport lock poisoned"))?
                .external
                .is_none()
        {
            bail!("a serial port is required for modern Yaesu CAT");
        }
        let mut state = self
            .transport
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT transport lock poisoned"))?;
        if state.port.is_none() && state.external.is_none() {
            state.port = Some(Box::new(SerialPortTransport(
                serialport::new(&self.port, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(if self.hardware_flow_control {
                        FlowControl::Hardware
                    } else {
                        FlowControl::None
                    })
                    .timeout(timeout)
                    .open()
                    .with_context(|| format!("failed to open Yaesu CAT port {}", self.port))?,
            )));
        }
        active_transport(&mut state)?
            .set_timeout(timeout)
            .context("failed to update Yaesu CAT timeout")?;
        let result = operation(&mut state);
        if result.is_err() {
            // A disconnected USB bridge can leave an open-looking handle that
            // fails forever. Drop it after any transport failure so the next
            // operation gets one clean reopen attempt.
            state.port = None;
            state.pending.clear();
        }
        result
    }
}

#[async_trait]
impl Radio for YaesuCatRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        parse_payload(&self.query("FA", None, 9)?, "FA")?
            .parse()
            .context("invalid Yaesu FA response")
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        if hz > 999_999_999 {
            bail!("frequency {hz} Hz does not fit Yaesu's nine-digit FA field");
        }
        if let Some(profile) = self.profile() {
            if !profile.supports_frequency(hz) {
                bail!(
                    "frequency {hz} Hz is outside the documented CAT range for {}",
                    profile.model.model_name()
                );
            }
        }
        self.send_set("FA", &format!("{hz:09}"))
    }

    async fn get_mode(&self) -> Result<Mode> {
        // Yaesu's MD read command has no selector parameter.  The response
        // includes the VFO selector and mode, for example MD02;.
        let response = self.query("MD", None, 2)?;
        let payload = parse_payload(&response, "MD")?;
        let mut chars = payload.chars();
        if chars.next() != Some('0') {
            bail!("unexpected Yaesu MD receiver selector: {payload}");
        }
        let code = chars.next().context("missing Yaesu MD mode code")?;
        match self.profile() {
            Some(profile) => profile.decode_mode(code),
            None => decode_common_mode(code),
        }
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let code = match self.profile() {
            Some(profile) => profile.encode_mode(mode)?,
            None => encode_common_mode(mode)?,
        };
        self.send_set("MD", &format!("0{code}"))
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.send_set("TX", if enabled { "1" } else { "0" })
    }

    async fn get_ptt(&self) -> Result<bool> {
        match parse_payload(&self.query("TX", None, 1)?, "TX")? {
            "0" => Ok(false),
            "1" | "2" => Ok(true),
            value => bail!("invalid Yaesu TX response: {value}"),
        }
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
            ControlId::RfPower => {
                let watts = self.get_power_watts()?;
                Ok(Some(ControlValue::U8(self.watts_to_normalized(watts)?)))
            }
            ControlId::Split => Ok(Some(ControlValue::Bool(self.get_split()?))),
            ControlId::Agc => {
                let response = self.query("GT", None, 2)?;
                let payload = parse_payload(&response, "GT")?;
                let value = payload
                    .strip_prefix('0')
                    .context("invalid Yaesu GT response")?
                    .parse::<u8>()?;
                if value > 4 {
                    bail!("invalid Yaesu AGC response: {value}");
                }
                Ok(Some(ControlValue::U8(value)))
            }
            ControlId::NoiseReduction => {
                let response = self.query("NR", None, 2)?;
                let payload = parse_payload(&response, "NR")?;
                match payload {
                    "00" => Ok(Some(ControlValue::Bool(false))),
                    "01" => Ok(Some(ControlValue::Bool(true))),
                    value => bail!("invalid Yaesu NR response: {value}"),
                }
            }
            ControlId::NoiseReductionLevel => {
                let response = self.query("RL", None, 3)?;
                let payload = parse_payload(&response, "RL")?;
                let value = payload
                    .strip_prefix('0')
                    .context("invalid Yaesu RL response")?
                    .parse::<u8>()?;
                if !(1..=15).contains(&value) {
                    bail!("invalid Yaesu NR level response: {value}");
                }
                Ok(Some(ControlValue::U8(value)))
            }
            _ => Ok(None),
        }
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        match id {
            MeterId::Signal => Ok(Some(self.get_yaesu_meter(1)?)),
            MeterId::Compression => Ok(Some(self.get_yaesu_meter(3)?)),
            MeterId::Alc => Ok(Some(self.get_yaesu_meter(4)?)),
            MeterId::Power => Ok(Some(self.get_yaesu_meter(5)?)),
            MeterId::Swr => Ok(Some(self.get_yaesu_meter(6)?)),
            MeterId::Current => Ok(Some(self.get_yaesu_meter(7)?)),
            MeterId::Voltage => Ok(Some(self.get_yaesu_meter(8)?)),
            MeterId::Temperature => bail!("Yaesu CAT temperature meter is not profiled"),
        }
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        match (id, value) {
            (ControlId::RfPower, ControlValue::U8(level)) => {
                self.set_power_watts(self.normalized_to_watts(level)?)
            }
            (ControlId::Split, ControlValue::Bool(enabled)) => self.set_split(enabled),
            (ControlId::Agc, ControlValue::U8(value)) if value <= 4 => {
                self.send_set("GT", &format!("0{value}"))
            }
            (ControlId::NoiseReduction, ControlValue::Bool(enabled)) => {
                self.send_set("NR", if enabled { "01" } else { "00" })
            }
            (ControlId::NoiseReductionLevel, ControlValue::U8(value))
                if (1..=15).contains(&value) =>
            {
                self.send_set("RL", &format!("0{value:02}"))
            }
            (_, value) => bail!("unsupported Yaesu CAT control/value: {id:?} = {value:?}"),
        }
    }

    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        YaesuCatRadio::get_repeater_settings(self)
    }

    async fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        YaesuCatRadio::set_repeater_settings(self, settings)
    }

    async fn select_memory_channel(&self, channel: u16) -> Result<()> {
        YaesuCatRadio::select_memory_channel(self, channel)
    }

    async fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        YaesuCatRadio::read_memory_channel(self, channel)
    }

    async fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        YaesuCatRadio::write_memory_channel(self, channel)
    }

    fn supports_repeater_settings(&self) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_repeater_settings)
    }

    fn supports_memory_channels(&self) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_repeater_settings)
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        self.model().is_some() && !matches!(id, MeterId::Temperature)
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.profile().is_some_and(|profile| {
            (id == ControlId::RfPower && profile.power_range_watts.is_some())
                || (id == ControlId::Split && profile.supports_split)
                || matches!(
                    id,
                    ControlId::Agc | ControlId::NoiseReduction | ControlId::NoiseReductionLevel
                )
        })
    }

    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: true,
            can_set_mode: true,
            can_get_ptt: true,
            can_set_ptt: true,
            can_get_power: true,
            can_set_power: true,
            can_raw_protocol: true,
        }
    }
}

impl YaesuCatRadio {
    fn get_yaesu_meter(&self, selector: u8) -> Result<u8> {
        let response = self.query("RM", Some(&selector.to_string()), 10)?;
        let payload = parse_payload(&response, "RM")?;
        let response_selector = payload
            .chars()
            .next()
            .context("missing Yaesu RM meter selector")?
            .to_digit(10)
            .context("invalid Yaesu RM meter selector")? as u8;
        if response_selector != selector {
            bail!(
                "Yaesu RM response selector {response_selector} did not match requested {selector}"
            );
        }
        let value = payload
            .get(1..4)
            .context("invalid Yaesu RM meter response")?
            .parse::<u16>()
            .context("invalid Yaesu RM meter value")?;
        if value > 255 {
            bail!("Yaesu RM meter value exceeds 255: {value}");
        }
        Ok(value as u8)
    }

    fn watts_to_normalized(&self, watts: u16) -> Result<u8> {
        let (minimum, maximum) = self
            .selected_profile()?
            .power_range_watts
            .context("RF power control is not profiled for this Yaesu model")?;
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
            .context("RF power control is not profiled for this Yaesu model")?;
        let span = u32::from(maximum - minimum);
        Ok(minimum + (((u32::from(level) * span) + 127) / 255) as u16)
    }
}

fn parse_payload<'a>(frame: &'a [u8], command: &str) -> Result<&'a str> {
    let text = std::str::from_utf8(frame).context("Yaesu CAT response is not ASCII")?;
    text.strip_prefix(command)
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected Yaesu CAT response")
}

fn decode_ftdx10_memory(payload: &str, profile: &YaesuCatProfile) -> Result<MemoryChannel> {
    anyhow::ensure!(payload.len() >= 25, "FTDX10 MR response is too short");
    let channel = payload[0..3]
        .parse::<u16>()
        .context("invalid FTDX10 memory channel")?;
    let frequency_hz = payload[3..12]
        .parse::<u64>()
        .context("invalid FTDX10 memory frequency")?;
    let sign = match &payload[12..13] {
        "+" => 1i32,
        "-" => -1i32,
        value => bail!("invalid FTDX10 clarifier direction: {value}"),
    };
    let offset = payload[13..17]
        .parse::<i32>()
        .context("invalid FTDX10 memory offset")?;
    let mode = profile.decode_mode(
        payload[19..20]
            .chars()
            .next()
            .context("missing FTDX10 memory mode")?,
    )?;
    let tone = match &payload[21..22] {
        "0" => ToneMode::Off,
        "1" => ToneMode::EncodeDecode,
        "2" => ToneMode::Encode,
        value => bail!("invalid FTDX10 memory tone mode: {value}"),
    };
    let shift = match &payload[24..25] {
        "0" => RepeaterShift::Simplex,
        "1" => RepeaterShift::Plus,
        "2" => RepeaterShift::Minus,
        value => bail!("invalid FTDX10 memory shift: {value}"),
    };
    Ok(MemoryChannel {
        channel,
        name: None,
        frequency_hz,
        transmit_frequency_hz: None,
        mode,
        repeater: RepeaterSettings {
            shift,
            offset_hz: Some((offset * sign).unsigned_abs()),
            tone: ToneSettings {
                mode: tone,
                index: 0,
                frequency_tenths_hz: None,
                dtcs_code: None,
                dtcs_reverse: None,
            },
        },
    })
}

fn validate_complete_command(command: &[u8]) -> Result<()> {
    if command.len() < 3
        || command.last() != Some(&b';')
        || command[..command.len() - 1].contains(&b';')
        || !command.is_ascii()
        || !command[..2].iter().all(u8::is_ascii_alphabetic)
    {
        bail!("expected one complete semicolon-terminated Yaesu CAT command");
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

fn encode_common_mode(mode: Mode) -> Result<char> {
    match mode {
        Mode::Lsb => Ok('1'),
        Mode::Usb => Ok('2'),
        Mode::Cw => Ok('3'),
        Mode::Fm => Ok('4'),
        Mode::Am => Ok('5'),
        Mode::Rtty => Ok('6'),
        Mode::CwReverse => Ok('7'),
        Mode::RttyReverse => Ok('9'),
        Mode::Data => Ok('C'),
        Mode::Wfm => bail!("WFM has no common modern Yaesu CAT mode mapping"),
    }
}

fn decode_common_mode(code: char) -> Result<Mode> {
    match code.to_ascii_uppercase() {
        '1' => Ok(Mode::Lsb),
        '2' => Ok(Mode::Usb),
        '3' => Ok(Mode::Cw),
        '4' | 'B' => Ok(Mode::Fm),
        '5' | 'D' => Ok(Mode::Am),
        '6' => Ok(Mode::Rtty),
        '7' => Ok(Mode::CwReverse),
        '9' => Ok(Mode::RttyReverse),
        '8' | 'A' | 'C' | 'E' | 'F' => Ok(Mode::Data),
        code => bail!("unsupported modern Yaesu CAT mode code: {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaesu::profile::FTDX10_PROFILE;

    #[test]
    fn common_commands_match_official_manual_examples() {
        assert_eq!(
            ascii_cat::encode("FA", Some("014250000")).unwrap(),
            b"FA014250000;"
        );
        assert_eq!(ascii_cat::encode("MD", Some("0C")).unwrap(), b"MD0C;");
        assert_eq!(ascii_cat::encode("MD", Some("0")).unwrap(), b"MD0;");
        assert_eq!(ascii_cat::encode("MD", None).unwrap(), b"MD;");
        assert_eq!(ascii_cat::encode("TX", None).unwrap(), b"TX;");
    }

    #[test]
    fn parser_handles_interleaved_and_partial_frames() {
        let mut pending = b"IF00140250000+0000000001;FA014250000;MD0".to_vec();
        let frames = take_complete_frames(&mut pending);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1], b"FA014250000;");
        assert_eq!(pending, b"MD0");
    }

    #[test]
    fn ptt_response_values_cover_cat_and_front_panel_tx() {
        assert_eq!(parse_payload(b"TX0;", "TX").unwrap(), "0");
        assert_eq!(parse_payload(b"TX1;", "TX").unwrap(), "1");
        assert_eq!(parse_payload(b"TX2;", "TX").unwrap(), "2");
    }

    #[test]
    fn raw_commands_must_be_single_complete_frames() {
        assert!(validate_complete_command(b"ID;").is_ok());
        assert!(validate_complete_command(b"ID").is_err());
        assert!(validate_complete_command(b"ID;FA;").is_err());
    }

    #[test]
    fn decodes_documented_ftdx10_memory_record() {
        let payload = format!(
            "{:03}{:09}+{:04}00{}0{}00{}",
            1, 14_074_000, 0, '2', '2', '1'
        );
        let channel = decode_ftdx10_memory(&payload, &FTDX10_PROFILE).unwrap();
        assert_eq!(channel.channel, 1);
        assert_eq!(channel.frequency_hz, 14_074_000);
        assert_eq!(channel.mode, Mode::Usb);
        assert_eq!(channel.repeater.tone.mode, ToneMode::Encode);
        assert_eq!(channel.repeater.shift, RepeaterShift::Plus);
    }

    #[test]
    fn constructors_enforce_profile_baud_rates() {
        assert!(YaesuCatRadio::new_for_model(YaesuCatModel::Ftdx10, "test", 38_400).is_ok());
        assert!(YaesuCatRadio::new_for_model(YaesuCatModel::Ftdx10, "test", 115_200).is_err());
        assert!(YaesuCatRadio::new_for_model(YaesuCatModel::Ft710, "test", 115_200).is_ok());
    }

    #[test]
    fn protocol_neutral_power_is_normalized_while_exact_api_uses_watts() {
        let ftdx10 = YaesuCatRadio::new_for_model(YaesuCatModel::Ftdx10, "test", 38_400).unwrap();
        assert_eq!(ftdx10.normalized_to_watts(0).unwrap(), 5);
        assert_eq!(ftdx10.normalized_to_watts(255).unwrap(), 100);
        assert_eq!(ftdx10.watts_to_normalized(5).unwrap(), 0);
        assert_eq!(ftdx10.watts_to_normalized(100).unwrap(), 255);

        let mp = YaesuCatRadio::new_for_model(YaesuCatModel::Ftdx101Mp, "test", 38_400).unwrap();
        assert_eq!(mp.normalized_to_watts(255).unwrap(), 200);
    }
}
