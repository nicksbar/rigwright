//! Model-neutral transport for Kenwood semicolon-terminated PC control.

use std::{
    io::{ErrorKind, Read, Write},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};

use crate::{
    hal::{Mode, Radio, RadioCapabilities},
    hal_types::{ControlId, ControlValue},
    models::KenwoodCatModel,
    protocol::ascii_cat,
};

use super::profile::{
    profile_for_model, KenwoodCatProfile, KenwoodModeCommand, KenwoodSplitCommand,
};

const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_200);
const MAX_FRAME_LEN: usize = 512;

#[derive(Default)]
struct TransportState {
    port: Option<Box<dyn SerialPort>>,
    pending: Vec<u8>,
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
        }
    }

    pub fn model(&self) -> Option<KenwoodCatModel> {
        self.model
    }

    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    pub fn profile(&self) -> Option<&'static KenwoodCatProfile> {
        self.model.map(profile_for_model)
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.transport.lock() {
            state.port = None;
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

    pub fn get_meter(&self) -> Result<u16> {
        let profile = self.selected_profile()?;
        let (parameters, payload_len) = match profile.model {
            KenwoodCatModel::Ts890S => (None, 4),
            _ => (Some("0"), 5),
        };
        let response = self.query("SM", parameters, Some(payload_len))?;
        let payload = parse_payload(&response, "SM")?;
        let value = if profile.model == KenwoodCatModel::Ts890S {
            payload
        } else {
            payload
                .strip_prefix('0')
                .context("unexpected Kenwood SM receiver selector")?
        };
        let value: u16 = value.parse().context("invalid Kenwood SM response")?;
        if value > profile.meter_max {
            bail!("Kenwood SM response exceeds the profiled meter range: {value}");
        }
        Ok(value)
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
            let port = state
                .port
                .as_mut()
                .context("Kenwood CAT port unavailable")?;
            port.write_all(request)
                .context("failed to write Kenwood CAT command")?;
            port.flush().context("failed to flush Kenwood CAT command")
        })
    }

    fn transact<F>(&self, request: &[u8], mut matcher: F) -> Result<Vec<u8>>
    where
        F: FnMut(&[u8]) -> bool,
    {
        validate_complete_command(request)?;
        self.with_transport(Duration::from_millis(150), |state| {
            let port = state
                .port
                .as_mut()
                .context("Kenwood CAT port unavailable")?;
            port.write_all(request)
                .context("failed to write Kenwood CAT command")?;
            port.flush()
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
                        _ => {} // Ignore interleaved Auto Information frames.
                    }
                }

                match port.read(&mut buffer) {
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
        if self.port.trim().is_empty() {
            bail!("a serial port is required for Kenwood PC control");
        }
        let mut state = self
            .transport
            .lock()
            .map_err(|_| anyhow!("Kenwood CAT transport lock poisoned"))?;
        if state.port.is_none() {
            let stop_bits = if self.baud_rate == 4_800 {
                StopBits::Two
            } else {
                StopBits::One
            };
            state.port = Some(
                serialport::new(&self.port, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(stop_bits)
                    .flow_control(FlowControl::None)
                    .timeout(timeout)
                    .open()
                    .with_context(|| format!("failed to open Kenwood CAT port {}", self.port))?,
            );
        }
        state
            .port
            .as_mut()
            .context("Kenwood CAT port unavailable")?
            .set_timeout(timeout)
            .context("failed to update Kenwood CAT timeout")?;
        let result = operation(&mut state);
        if result.is_err() {
            state.port = None;
            state.pending.clear();
        }
        result
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

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        validate_complete_command(request)?;
        let command = std::str::from_utf8(&request[..2]).context("CAT command is not ASCII")?;
        self.transact(request, |frame| frame.starts_with(command.as_bytes()))
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        match id {
            ControlId::RfPower => Ok(Some(ControlValue::U8(
                self.watts_to_normalized(self.get_power_watts()?)?,
            ))),
            ControlId::Split => Ok(Some(ControlValue::Bool(self.get_split()?))),
            _ => Ok(None),
        }
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        match (id, value) {
            (ControlId::RfPower, ControlValue::U8(level)) => {
                self.set_power_watts(self.normalized_to_watts(level)?)
            }
            (ControlId::Split, ControlValue::Bool(enabled)) => self.set_split(enabled),
            (_, value) => bail!("unsupported Kenwood CAT control/value: {id:?} = {value:?}"),
        }
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
