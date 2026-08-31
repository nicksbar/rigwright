//! Framing and byte-stream ownership for Elecraft ASCII protocols.

use std::{
    collections::VecDeque,
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
const MAX_RETAINED_FRAMES: usize = 128;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ElecraftTransportMetrics {
    pub commands_started: u64,
    pub responses_matched: u64,
    pub response_timeouts: u64,
    pub bytes_read: u64,
    pub frames_received: u64,
    pub frames_retained: u64,
    pub frames_dropped: u64,
    pub total_response_time: Duration,
    pub consecutive_timeouts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElecraftSerialPolicy {
    pub hardware_flow_control: bool,
    pub dtr: Option<bool>,
    pub rts: Option<bool>,
    pub startup_settle: Duration,
}

impl Default for ElecraftSerialPolicy {
    fn default() -> Self {
        Self {
            hardware_flow_control: false,
            dtr: None,
            rts: None,
            startup_settle: Duration::from_millis(50),
        }
    }
}

#[derive(Default)]
struct State {
    port: Option<Box<dyn RadioTransport>>,
    pending: Vec<u8>,
    retained: VecDeque<Vec<u8>>,
    metrics: ElecraftTransportMetrics,
}

/// A serialized Elecraft byte stream. The parser deliberately ignores
/// unrelated complete frames while waiting for the requested response, which
/// is required when Auto-Info is enabled.
#[derive(Clone)]
pub(crate) struct ElecraftTransport {
    port_name: String,
    baud_rate: u32,
    state: Arc<Mutex<State>>,
    serial_policy: ElecraftSerialPolicy,
}

impl ElecraftTransport {
    pub(crate) fn serial(port_name: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port_name: port_name.into(),
            baud_rate,
            state: Arc::new(Mutex::new(State::default())),
            serial_policy: ElecraftSerialPolicy::default(),
        }
    }

    pub(crate) fn external(transport: impl RadioTransport + 'static) -> Self {
        Self {
            port_name: String::new(),
            baud_rate: 0,
            state: Arc::new(Mutex::new(State {
                port: Some(Box::new(transport)),
                pending: Vec::new(),
                retained: VecDeque::new(),
                metrics: ElecraftTransportMetrics::default(),
            })),
            serial_policy: ElecraftSerialPolicy::default(),
        }
    }

    pub(crate) fn with_serial_policy(mut self, policy: ElecraftSerialPolicy) -> Self {
        self.serial_policy = policy;
        self
    }

    pub(crate) fn transact(
        &self,
        command: &[u8],
        response_prefix: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        self.transact_with_handler(command, response_prefix, |_| {})
    }

    pub(crate) fn metrics(&self) -> ElecraftTransportMetrics {
        self.state
            .lock()
            .map(|state| state.metrics)
            .unwrap_or_default()
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
        let started = Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("Elecraft transport lock poisoned"))?;
        state.metrics.commands_started = state.metrics.commands_started.saturating_add(1);
        if state.port.is_none() {
            if self.port_name.trim().is_empty() {
                bail!("a serial port is required for Elecraft control");
            }
            let mut port = serialport::new(&self.port_name, self.baud_rate)
                .data_bits(DataBits::Eight)
                .parity(Parity::None)
                .stop_bits(StopBits::One)
                .flow_control(if self.serial_policy.hardware_flow_control {
                    FlowControl::Hardware
                } else {
                    FlowControl::None
                })
                .timeout(RESPONSE_TIMEOUT)
                .open()
                .with_context(|| {
                    format!("failed to open Elecraft serial port {}", self.port_name)
                })?;
            if let Some(enabled) = self.serial_policy.dtr {
                port.write_data_terminal_ready(enabled)
                    .map_err(std::io::Error::other)?;
            }
            if let Some(enabled) = self.serial_policy.rts {
                port.write_request_to_send(enabled)
                    .map_err(std::io::Error::other)?;
            }
            port.clear(serialport::ClearBuffer::Input)
                .map_err(std::io::Error::other)?;
            thread::sleep(self.serial_policy.startup_settle);
            state.port = Some(Box::new(SerialPortTransport(port)));
        }
        let timeout =
            RESPONSE_TIMEOUT.mul_f32(1.0_f32 + state.metrics.consecutive_timeouts.min(2) as f32);
        let result = Self::transact_locked(
            &mut state,
            command,
            response_prefix,
            timeout,
            &mut on_unmatched,
        );
        if result.is_ok() {
            state.metrics.responses_matched = state.metrics.responses_matched.saturating_add(1);
            state.metrics.total_response_time += started.elapsed();
            state.metrics.consecutive_timeouts = 0;
        } else {
            state.metrics.response_timeouts = state.metrics.response_timeouts.saturating_add(1);
            state.metrics.consecutive_timeouts =
                state.metrics.consecutive_timeouts.saturating_add(1);
        }
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
        timeout: Duration,
        on_unmatched: &mut impl FnMut(&[u8]),
    ) -> Result<Vec<u8>> {
        let transport = state
            .port
            .as_mut()
            .context("Elecraft transport unavailable")?;
        transport
            .set_timeout(timeout)
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
        if let Some(index) = state
            .retained
            .iter()
            .position(|frame| frame.starts_with(prefix))
        {
            return Ok(state.retained.remove(index).expect("retained frame index"));
        }
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(end) = state.pending.iter().position(|byte| *byte == b';') {
                let frame: Vec<u8> = state.pending.drain(..=end).collect();
                state.metrics.frames_received = state.metrics.frames_received.saturating_add(1);
                if frame.starts_with(prefix) {
                    return Ok(frame);
                }
                on_unmatched(&frame);
                if state.retained.len() >= MAX_RETAINED_FRAMES {
                    state.retained.pop_front();
                    state.metrics.frames_dropped = state.metrics.frames_dropped.saturating_add(1);
                }
                state.retained.push_back(frame);
                state.metrics.frames_retained = state.metrics.frames_retained.saturating_add(1);
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
                Ok(count) if count > 0 => {
                    state.metrics.bytes_read =
                        state.metrics.bytes_read.saturating_add(count as u64);
                    state.pending.extend_from_slice(&buffer[..count])
                }
                Ok(_) => thread::sleep(Duration::from_millis(1)),
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

    pub(crate) fn query_with_response_prefix(
        &self,
        command: &str,
        response_prefix: &str,
    ) -> Result<Vec<u8>> {
        let mut frame = command.as_bytes().to_vec();
        frame.push(b';');
        self.transact(&frame, Some(response_prefix.as_bytes()))
    }

    pub(crate) fn set(&self, command: &str, parameter: &str) -> Result<()> {
        let mut frame = command.as_bytes().to_vec();
        frame.extend_from_slice(parameter.as_bytes());
        frame.push(b';');
        self.transact(&frame, None).map(|_| ())
    }
}
