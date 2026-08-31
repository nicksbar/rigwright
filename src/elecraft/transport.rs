//! Framing and byte-stream ownership for Elecraft ASCII protocols.

use std::{
    io::ErrorKind,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::transport::{RadioTransport, SerialPortTransport};

pub(crate) const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_200);
const MAX_FRAME_LEN: usize = 512;

#[derive(Default)]
struct State {
    port: Option<Box<dyn RadioTransport>>,
    pending: Vec<u8>,
}

/// A serialized Elecraft byte stream. The parser deliberately ignores
/// unrelated complete frames while waiting for the requested response, which
/// is required when Auto-Info is enabled.
#[derive(Clone)]
pub(crate) struct ElecraftTransport {
    port_name: String,
    baud_rate: u32,
    state: Arc<Mutex<State>>,
}

impl ElecraftTransport {
    pub(crate) fn serial(port_name: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port_name: port_name.into(),
            baud_rate,
            state: Arc::new(Mutex::new(State::default())),
        }
    }

    pub(crate) fn external(transport: impl RadioTransport + 'static) -> Self {
        Self {
            port_name: String::new(),
            baud_rate: 0,
            state: Arc::new(Mutex::new(State {
                port: Some(Box::new(transport)),
                pending: Vec::new(),
            })),
        }
    }

    pub(crate) fn transact(
        &self,
        command: &[u8],
        response_prefix: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        self.transact_with_handler(command, response_prefix, |_| {})
    }

    pub(crate) fn transact_with_handler<F>(
        &self,
        command: &[u8],
        response_prefix: Option<&[u8]>,
        mut on_unmatched: F,
    ) -> Result<Vec<u8>>
    where
        F: FnMut(&[u8]),
    {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Elecraft transport lock poisoned"))?;
        if state.port.is_none() {
            if self.port_name.trim().is_empty() {
                bail!("a serial port is required for Elecraft control");
            }
            state.port = Some(Box::new(SerialPortTransport(
                serialport::new(&self.port_name, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(FlowControl::None)
                    .timeout(RESPONSE_TIMEOUT)
                    .open()
                    .with_context(|| {
                        format!("failed to open Elecraft serial port {}", self.port_name)
                    })?,
            )));
        }
        let result = Self::transact_locked(&mut state, command, response_prefix, &mut on_unmatched);
        if result.is_err() {
            state.port = None;
            state.pending.clear();
        }
        result
    }

    fn transact_locked(
        state: &mut State,
        command: &[u8],
        response_prefix: Option<&[u8]>,
        on_unmatched: &mut impl FnMut(&[u8]),
    ) -> Result<Vec<u8>> {
        let transport = state
            .port
            .as_mut()
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
            if let Some(end) = state.pending.iter().position(|byte| *byte == b';') {
                let frame: Vec<u8> = state.pending.drain(..=end).collect();
                if frame.starts_with(prefix) {
                    return Ok(frame);
                }
                on_unmatched(&frame);
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
    }

    pub(crate) fn query_with_handler<F>(&self, command: &str, on_unmatched: F) -> Result<Vec<u8>>
    where
        F: FnMut(&[u8]),
    {
        let mut frame = command.as_bytes().to_vec();
        frame.push(b';');
        self.transact_with_handler(&frame, Some(command.as_bytes()), on_unmatched)
    }

    pub(crate) fn set(&self, command: &str, parameter: &str) -> Result<()> {
        let mut frame = command.as_bytes().to_vec();
        frame.extend_from_slice(parameter.as_bytes());
        frame.push(b';');
        self.transact(&frame, None).map(|_| ())
    }
}
