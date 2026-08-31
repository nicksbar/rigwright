//! Shared semicolon-framed Elecraft transceiver driver.

use std::{
    io::ErrorKind,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::{
    hal::{Mode, Radio, RadioCapabilities},
    hal_types::{ControlId, ControlValue, MeterId},
    transport::{RadioTransport, SerialPortTransport},
};

use super::profile::{profile_for_model, ElecraftModel, ElecraftProfile};

const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_200);
const MAX_FRAME_LEN: usize = 512;

#[derive(Default)]
struct TransportState {
    port: Option<Box<dyn RadioTransport>>,
    external: Option<Box<dyn RadioTransport>>,
    pending: Vec<u8>,
}

pub struct ElecraftRadio {
    model: Option<ElecraftModel>,
    port: String,
    baud_rate: u32,
    transport: Arc<Mutex<TransportState>>,
}

impl Clone for ElecraftRadio {
    fn clone(&self) -> Self {
        Self {
            model: self.model,
            port: self.port.clone(),
            baud_rate: self.baud_rate,
            transport: Arc::clone(&self.transport),
        }
    }
}

impl std::fmt::Debug for ElecraftRadio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElecraftRadio")
            .field("model", &self.model)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .finish_non_exhaustive()
    }
}

impl ElecraftRadio {
    pub fn new_generic(port: impl Into<String>, baud_rate: u32) -> Self {
        Self::new_internal(None, port, baud_rate)
    }

    pub fn new_for_model(
        model: ElecraftModel,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Result<Self> {
        profile_for_model(model).validate_baud(baud_rate)?;
        Ok(Self::new_internal(Some(model), port, baud_rate))
    }

    pub fn with_external_transport<T>(
        model: Option<ElecraftModel>,
        baud_rate: u32,
        transport: T,
    ) -> Result<Self>
    where
        T: RadioTransport + 'static,
    {
        if let Some(model) = model {
            profile_for_model(model).validate_baud(baud_rate)?;
        }
        Ok(Self {
            model,
            port: String::new(),
            baud_rate,
            transport: Arc::new(Mutex::new(TransportState {
                port: None,
                external: Some(Box::new(transport)),
                pending: Vec::new(),
            })),
        })
    }

    pub fn model(&self) -> Option<ElecraftModel> {
        self.model
    }

    fn new_internal(model: Option<ElecraftModel>, port: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            model,
            port: port.into(),
            baud_rate,
            transport: Arc::new(Mutex::new(TransportState::default())),
        }
    }

    fn profile(&self) -> Option<ElecraftProfile> {
        self.model.map(profile_for_model)
    }

    fn with_transport<T>(
        &self,
        operation: impl FnOnce(&mut TransportState) -> Result<T>,
    ) -> Result<T> {
        let mut state = self
            .transport
            .lock()
            .map_err(|_| anyhow::anyhow!("Elecraft transport lock poisoned"))?;
        if state.port.is_none() && state.external.is_none() {
            if self.port.trim().is_empty() {
                bail!("a serial port is required for Elecraft control");
            }
            state.port = Some(Box::new(SerialPortTransport(
                serialport::new(&self.port, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(FlowControl::None)
                    .timeout(RESPONSE_TIMEOUT)
                    .open()
                    .with_context(|| {
                        format!("failed to open Elecraft serial port {}", self.port)
                    })?,
            )));
        }
        let result = operation(&mut state);
        if result.is_err() {
            state.port = None;
            state.pending.clear();
        }
        result
    }

    fn transact(&self, command: &[u8], response_prefix: Option<&[u8]>) -> Result<Vec<u8>> {
        self.with_transport(|state| {
            let transport = state
                .external
                .as_mut()
                .or(state.port.as_mut())
                .map(|p| &mut **p)
                .context("Elecraft transport unavailable")?;
            transport
                .set_timeout(RESPONSE_TIMEOUT)
                .context("failed to set Elecraft timeout")?;
            transport
                .write_all(command)
                .context("failed to write Elecraft command")?;
            transport
                .flush()
                .context("failed to flush Elecraft command")?;
            let Some(prefix) = response_prefix else {
                return Ok(Vec::new());
            };
            let deadline = Instant::now() + RESPONSE_TIMEOUT;
            loop {
                if let Some(end) = state.pending.iter().position(|b| *b == b';') {
                    let frame: Vec<u8> = state.pending.drain(..=end).collect();
                    if frame.starts_with(prefix) {
                        return Ok(frame);
                    }
                    continue;
                }
                if state.pending.len() > MAX_FRAME_LEN {
                    bail!("Elecraft receive frame exceeded safety limit");
                }
                if Instant::now() >= deadline {
                    bail!(
                        "timed out waiting for Elecraft response to {}",
                        String::from_utf8_lossy(command)
                    );
                }
                let mut buffer = [0_u8; 128];
                match transport.read(&mut buffer) {
                    Ok(count) if count > 0 => state.pending.extend_from_slice(&buffer[..count]),
                    Ok(_) => thread::sleep(Duration::from_millis(5)),
                    Err(error) if error.kind() == ErrorKind::TimedOut => {}
                    Err(error) => return Err(error).context("failed to read Elecraft response"),
                }
            }
        })
    }

    fn query(&self, command: &str) -> Result<Vec<u8>> {
        let mut frame = command.as_bytes().to_vec();
        frame.push(b';');
        self.transact(&frame, Some(command.as_bytes()))
    }

    fn set(&self, command: &str, parameter: &str) -> Result<()> {
        let mut frame = command.as_bytes().to_vec();
        frame.extend_from_slice(parameter.as_bytes());
        frame.push(b';');
        self.transact(&frame, None).map(|_| ())
    }

    fn selected_frequency(&self) -> &'static str {
        "FA"
    }

    fn parse_frequency(response: &[u8]) -> Result<u64> {
        let text =
            std::str::from_utf8(response).context("Elecraft frequency response is not ASCII")?;
        text.strip_prefix("FA")
            .and_then(|v| v.strip_suffix(';'))
            .context("unexpected Elecraft frequency response")?
            .parse()
            .context("invalid Elecraft frequency")
    }

    fn parse_mode(profile: ElecraftProfile, response: &[u8]) -> Result<Mode> {
        let text = std::str::from_utf8(response).context("Elecraft mode response is not ASCII")?;
        let code = text
            .strip_prefix("MD")
            .and_then(|v| v.strip_suffix(';'))
            .and_then(|v| v.chars().next())
            .context("unexpected Elecraft mode response")?;
        profile.decode_mode(code)
    }
}

#[async_trait]
impl Radio for ElecraftRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        Self::parse_frequency(&self.query(self.selected_frequency())?)
    }

    async fn set_frequency_hz(&self, frequency_hz: u64) -> Result<()> {
        if let Some(profile) = self.profile() {
            if !profile.supports_frequency(frequency_hz) {
                bail!(
                    "{} frequency is outside the profiled range",
                    profile.model.model_name()
                );
            }
        }
        self.set("FA", &format!("{frequency_hz:011}"))
    }

    async fn get_mode(&self) -> Result<Mode> {
        let profile = self
            .profile()
            .context("Elecraft mode decoding requires a selected model")?;
        Self::parse_mode(profile, &self.query("MD")?)
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let profile = self
            .profile()
            .context("Elecraft mode encoding requires a selected model")?;
        self.set("MD", &profile.encode_mode(mode)?.to_string())
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.set(if enabled { "TX" } else { "RX" }, "")
    }

    async fn get_ptt(&self) -> Result<bool> {
        let response = self.query("TQ")?;
        let text = std::str::from_utf8(&response).context("Elecraft TQ response is not ASCII")?;
        match text
            .strip_prefix("TQ")
            .and_then(|v| v.strip_suffix(';').or(Some(v)))
        {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => bail!("unexpected Elecraft TQ response: {text}"),
        }
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        if id == ControlId::AfGain {
            let value = std::str::from_utf8(&self.query("AG")?)
                .context("Elecraft AF gain response is not ASCII")?
                .trim_end_matches(';')
                .strip_prefix("AG")
                .context("unexpected Elecraft AF gain response")?
                .parse::<u16>()
                .context("invalid Elecraft AF gain")?;
            return Ok(Some(ControlValue::U8(
                u8::try_from(value).context("Elecraft AF gain out of range")?,
            )));
        }
        Ok(None)
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        match (id, value) {
            (ControlId::AfGain, ControlValue::U8(value)) => self.set("AG", &format!("{value:03}")),
            _ => bail!("Elecraft control {id:?} is not implemented"),
        }
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        if id != MeterId::Signal {
            return Ok(None);
        }
        let response = self.query("SM")?;
        let value = std::str::from_utf8(&response)
            .context("Elecraft S-meter response is not ASCII")?
            .trim_end_matches(';')
            .strip_prefix("SM")
            .context("unexpected Elecraft S-meter response")?
            .parse::<u16>()
            .context("invalid Elecraft S-meter")?;
        Ok(Some(
            crate::normalize_meter_level(
                value,
                if matches!(self.model, Some(ElecraftModel::K2)) {
                    15
                } else {
                    30
                },
            )
            .context("Elecraft S-meter out of range")?,
        ))
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        id == MeterId::Signal
    }
    fn supports_control(&self, id: ControlId) -> bool {
        id == ControlId::AfGain
    }
    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: self.model.is_some(),
            can_set_mode: self.model.is_some(),
            can_get_ptt: true,
            can_set_ptt: true,
            can_get_power: false,
            can_set_power: false,
            can_raw_protocol: true,
        }
    }

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        let mut command = request.to_vec();
        if !command.ends_with(b";") {
            command.push(b';');
        }
        self.transact(&command, Some(&command[..command.len() - 1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::io::{Read, Write};

    struct MemoryTransport {
        input: Vec<u8>,
        output: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for MemoryTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = buffer.len().min(self.input.len());
            buffer[..count].copy_from_slice(&self.input[..count]);
            self.input.drain(..count);
            Ok(count)
        }
    }
    impl Write for MemoryTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.output.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl RadioTransport for MemoryTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn k3_core_commands_use_documented_frames() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let transport = MemoryTransport {
            input: b"FA00014060000;MD2;TQ0;SM00015;".to_vec(),
            output: Arc::clone(&output),
        };
        let radio =
            ElecraftRadio::with_external_transport(Some(ElecraftModel::K3), 9_600, transport)
                .unwrap();
        assert_eq!(block_on(radio.get_frequency_hz()).unwrap(), 14_060_000);
        assert_eq!(block_on(radio.get_mode()).unwrap(), Mode::Usb);
        assert!(!block_on(radio.get_ptt()).unwrap());
        assert_eq!(
            block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(128)
        );
        block_on(radio.set_frequency_hz(14_074_000)).unwrap();
        block_on(radio.set_mode(Mode::Cw)).unwrap();
        block_on(radio.set_ptt(true)).unwrap();
        assert_eq!(
            &*output.lock().unwrap(),
            b"FA;MD;TQ;SM;FA00014074000;MD3;TX;"
        );
    }

    #[test]
    fn k2_profile_preserves_legacy_mode_and_meter_limits() {
        let transport = MemoryTransport {
            input: b"MD6;SM0015;".to_vec(),
            output: Arc::new(Mutex::new(Vec::new())),
        };
        let radio =
            ElecraftRadio::with_external_transport(Some(ElecraftModel::K2), 4_800, transport)
                .unwrap();
        assert_eq!(block_on(radio.get_mode()).unwrap(), Mode::Rtty);
        assert_eq!(
            block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(255)
        );
    }
}
