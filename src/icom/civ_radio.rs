use crate::hal::{Radio, RadioCapabilities, RadioStatus};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serialport::{SerialPort, SerialPortType};
use std::{
    collections::VecDeque,
    io::ErrorKind,
    io::Read,
    io::Write,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    time::Instant,
};

use super::profile::profile_for_model;
use super::profile::ControlEncoding;
pub use crate::hal_types::{BaseMode, Mode, OperatingMode};
use crate::hal_types::{ControlId, ControlValue, MeterId, TunerStatus};

const CI_V_FRAME_START: u8 = 0xFE;
const CI_V_FRAME_END: u8 = 0xFD;

#[derive(Debug, Default)]
struct ScopeSweepAssembler {
    collecting: bool,
    next_division: usize,
    bins: Vec<u8>,
    dropped_sweeps: u64,
}

#[derive(Debug, Default)]
struct ScopeStreamReader {
    pending: Vec<u8>,
    assembler: ScopeSweepAssembler,
    completed: VecDeque<Vec<u8>>,
    division_frames: u64,
    completed_sweeps: u64,
}

impl ScopeStreamReader {
    fn ingest_bytes(
        &mut self,
        bytes: &[u8],
        radio_address: u8,
        controller_address: u8,
        geometry: Option<crate::models::IcomScopeGeometry>,
    ) {
        self.pending.extend_from_slice(bytes);
        for frame in drain_ci_v_frames(&mut self.pending) {
            if !is_radio_to_controller_frame(&frame, radio_address, controller_address)
                || !is_spectrum_data_frame(&frame)
            {
                continue;
            }
            self.division_frames = self.division_frames.wrapping_add(1);
            if let Some(sweep) = self.assembler.push(&frame, geometry) {
                self.completed_sweeps = self.completed_sweeps.wrapping_add(1);
                self.completed.push_back(sweep);
            }
        }
    }

    fn push_bytes(
        &mut self,
        bytes: &[u8],
        radio_address: u8,
        controller_address: u8,
        geometry: Option<crate::models::IcomScopeGeometry>,
    ) -> Vec<Vec<u8>> {
        self.ingest_bytes(bytes, radio_address, controller_address, geometry);
        self.completed.drain(..).collect()
    }
}

impl ScopeSweepAssembler {
    fn push(
        &mut self,
        frame: &[u8],
        geometry: Option<crate::models::IcomScopeGeometry>,
    ) -> Option<Vec<u8>> {
        let (division, maximum, bins) = parse_scope_waveform_segment(frame, geometry)?;
        let Some(geometry) = geometry else {
            self.reset();
            return None;
        };
        if maximum != geometry.divisions || division == 0 || division > geometry.divisions {
            self.reset();
            return None;
        }

        if division == 1 {
            if self.collecting {
                self.dropped_sweeps = self.dropped_sweeps.wrapping_add(1);
            }
            self.collecting = true;
            self.next_division = 2;
            self.bins.clear();
            return None;
        }

        if !self.collecting || division != self.next_division {
            if self.collecting {
                self.dropped_sweeps = self.dropped_sweeps.wrapping_add(1);
            }
            self.reset();
            return None;
        }

        let expected_bins = if division == geometry.divisions {
            geometry.last_chunk_bins
        } else {
            geometry.full_chunk_bins
        };
        if bins.len() != expected_bins || bins.iter().any(|value| *value > geometry.bin_max) {
            self.reset();
            return None;
        }

        self.bins.extend_from_slice(&bins);
        self.next_division += 1;
        if division == geometry.divisions {
            self.collecting = false;
            self.next_division = 0;
            if self.bins.len() == geometry.bins {
                return Some(std::mem::take(&mut self.bins));
            }
            self.bins.clear();
        }
        None
    }

    fn reset(&mut self) {
        self.collecting = false;
        self.next_division = 0;
        self.bins.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcomVfo {
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcomReceiver {
    Main,
    Sub,
}

pub fn enumerate_serial_ports() -> Result<Vec<String>> {
    let mut ports = enumerate_serial_port_descriptors()?
        .into_iter()
        .map(|port| port.port_name)
        .collect::<Vec<_>>();
    ports.sort();
    ports.dedup();
    Ok(ports)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialPortDescriptor {
    pub port_name: String,
    pub display_name: String,
    pub likely_radio: Option<String>,
}

pub fn enumerate_serial_port_descriptors() -> Result<Vec<SerialPortDescriptor>> {
    let mut ports = serialport::available_ports().context("failed to enumerate serial ports")?;
    ports.sort_by(|left, right| left.port_name.cmp(&right.port_name));

    let descriptors = ports
        .into_iter()
        .map(|port| {
            let port_name = port.port_name;
            let (extra_label, likely_radio) = describe_port_type(&port.port_type);
            let display_name = if extra_label.is_empty() {
                port_name.clone()
            } else {
                format!("{} — {}", port_name, extra_label)
            };

            SerialPortDescriptor {
                port_name,
                display_name,
                likely_radio,
            }
        })
        .collect::<Vec<_>>();

    Ok(descriptors)
}

fn describe_port_type(port_type: &SerialPortType) -> (String, Option<String>) {
    match port_type {
        SerialPortType::UsbPort(info) => {
            let manufacturer = info.manufacturer.as_deref().unwrap_or("").trim();
            let product = info.product.as_deref().unwrap_or("").trim();

            let likely_radio = detect_likely_radio_model(info.vid, info.pid, manufacturer, product);

            let usb_label = if !manufacturer.is_empty() && !product.is_empty() {
                format!(
                    "{} {} (VID:{:04X} PID:{:04X})",
                    manufacturer, product, info.vid, info.pid
                )
            } else if !product.is_empty() {
                format!("{} (VID:{:04X} PID:{:04X})", product, info.vid, info.pid)
            } else if !manufacturer.is_empty() {
                format!(
                    "{} (VID:{:04X} PID:{:04X})",
                    manufacturer, info.vid, info.pid
                )
            } else {
                format!("USB serial (VID:{:04X} PID:{:04X})", info.vid, info.pid)
            };

            if let Some(model) = &likely_radio {
                (format!("{} [{}]", usb_label, model), likely_radio)
            } else {
                (usb_label, None)
            }
        }
        _ => ("serial device".to_string(), None),
    }
}

fn detect_likely_radio_model(
    vid: u16,
    _pid: u16,
    manufacturer: &str,
    product: &str,
) -> Option<String> {
    let manufacturer_lc = manufacturer.to_ascii_lowercase();
    let product_lc = product.to_ascii_lowercase();

    if product_lc.contains("ic-7300") || (vid == 0x0C26 && product_lc.contains("7300")) {
        return Some("Icom IC-7300 (CI-V)".to_string());
    }

    if manufacturer_lc.contains("icom") || product_lc.contains("icom") {
        return Some("Icom CI-V radio".to_string());
    }

    None
}

/// Model-neutral Icom CI-V radio driver.
///
/// This type owns CI-V framing, serial I/O, response matching, and generic
/// operations such as frequency, mode, PTT, raw protocol access, and profile
/// control execution. Construct it with [`Self::new_generic`] when no model
/// is known. In that mode, basic operations do not assume a particular Icom
/// radio; model-dependent controls and scope decoding require
/// [`Self::new_for_model`].
#[derive(Clone)]
pub struct IcomCiVRadio {
    model: Option<crate::models::IcomCivModel>,
    port: String,
    baud_rate: u32,
    controller_address: u8,
    radio_address: u8,
    serial_port: Arc<Mutex<Option<Box<dyn SerialPort>>>>,
    scope_stream_reader: Arc<Mutex<ScopeStreamReader>>,
}

#[derive(Debug, Clone, Copy)]
enum ControlOp {
    Set,
    Get,
}

impl IcomCiVRadio {
    fn set_operating_mode_blocking(
        &self,
        base_mode: BaseMode,
        data_mode: bool,
        filter: u8,
    ) -> Result<()> {
        if let Some(model) = self.model {
            anyhow::ensure!(
                super::modes::supports_mode(model, base_mode),
                "mode {base_mode:?} is not documented for {}",
                model.model_name()
            );
        }
        let mode_byte = base_mode_to_civ_mode(base_mode)
            .with_context(|| format!("unsupported base mode for CI-V set: {base_mode:?}"))?;
        anyhow::ensure!((1..=3).contains(&filter), "CI-V filter must be in 1..=3");
        let data_byte = if data_mode { 0x01 } else { 0x00 };
        let _ = self.transact(&[0x26, 0x00, mode_byte, data_byte, filter], false)?;
        Ok(())
    }

    pub async fn set_operating_mode_details(
        &self,
        base_mode: BaseMode,
        data_mode: bool,
        filter: u8,
    ) -> Result<()> {
        self.set_operating_mode_blocking(base_mode, data_mode, filter)
    }

    /// Create a model-neutral CI-V radio.
    ///
    /// Basic CI-V operations are available without identifying a specific
    /// radio. Model-dependent controls and scope operations require
    /// [`Self::new_for_model`].
    pub fn new_generic(
        port: impl Into<String>,
        baud_rate: u32,
        controller_address: u8,
        radio_address: u8,
    ) -> Self {
        Self::new_internal(None, port, baud_rate, controller_address, radio_address)
    }

    /// Create a CI-V driver with a selected model profile.
    ///
    /// The profile supplies model-specific validation and command behavior;
    /// the caller still supplies both CI-V addresses so non-default setups
    /// remain possible.
    pub fn new_for_model(
        model: crate::models::IcomCivModel,
        port: impl Into<String>,
        baud_rate: u32,
        controller_address: u8,
        radio_address: u8,
    ) -> Self {
        Self::new_internal(
            Some(model),
            port,
            baud_rate,
            controller_address,
            radio_address,
        )
    }

    /// Create a model-backed CI-V driver using the profile's factory address.
    /// Use [`Self::new_for_model`] when the radio's CI-V address was changed.
    pub fn new_for_model_default_address(
        model: crate::models::IcomCivModel,
        port: impl Into<String>,
        baud_rate: u32,
        controller_address: u8,
    ) -> Self {
        let radio_address = profile_for_model(model).default_address;
        Self::new_for_model(model, port, baud_rate, controller_address, radio_address)
    }

    fn new_internal(
        model: Option<crate::models::IcomCivModel>,
        port: impl Into<String>,
        baud_rate: u32,
        controller_address: u8,
        radio_address: u8,
    ) -> Self {
        Self {
            model,
            port: port.into(),
            baud_rate,
            controller_address,
            radio_address,
            serial_port: Arc::new(Mutex::new(None)),
            scope_stream_reader: Arc::new(Mutex::new(ScopeStreamReader::default())),
        }
    }

    pub fn with_radio_address(mut self, radio_address: u8) -> Self {
        self.radio_address = radio_address;
        self
    }

    /// Return the selected model, or `None` for a model-neutral driver.
    pub fn model(&self) -> Option<crate::models::IcomCivModel> {
        self.model
    }

    /// Controller address used in outgoing CI-V frames (normally `0xE0`).
    pub fn controller_address(&self) -> u8 {
        self.controller_address
    }

    /// Radio address used in outgoing CI-V frames.
    pub fn radio_address(&self) -> u8 {
        self.radio_address
    }

    fn selected_model(&self) -> Result<crate::models::IcomCivModel> {
        self.model
            .context("this CI-V operation requires a selected Icom model profile")
    }

    pub(crate) fn require_model(&self, expected: crate::models::IcomCivModel) -> Result<()> {
        anyhow::ensure!(
            self.selected_model()? == expected,
            "this operation requires the {} Icom profile",
            expected.model_name()
        );
        Ok(())
    }

    /// Select VFO A or B. This uses the global CI-V `0x07` VFO command and
    /// does not require a model profile.
    pub async fn select_vfo(&self, vfo: IcomVfo) -> Result<()> {
        let value = match vfo {
            IcomVfo::A => 0x00,
            IcomVfo::B => 0x01,
        };
        self.transact(&[0x07, value], false).map(|_| ())
    }

    /// Select the main or sub receiver. This is only available for profiles
    /// that declare main/sub behavior.
    pub async fn select_receiver(&self, receiver: IcomReceiver) -> Result<()> {
        let profile = profile_for_model(self.selected_model()?);
        if profile.main_sub.is_none() {
            anyhow::bail!(
                "main/sub receiver selection is not supported for {}",
                self.selected_model()?.model_name()
            );
        }
        let value = profile.main_sub.expect("checked above").set_subcommand_base
            + match receiver {
                IcomReceiver::Main => 0,
                IcomReceiver::Sub => 1,
            };
        self.transact(&[0x07, value], false).map(|_| ())
    }

    pub fn probe(&self) -> Result<RadioStatus> {
        self.probe_direct_serial()
    }

    pub async fn probe_stream_status(&self) -> Result<RadioStatus> {
        self.probe_stream_status_blocking()
    }

    fn probe_direct_serial(&self) -> Result<RadioStatus> {
        self.with_serial_port(Duration::from_millis(700), |port| {
            let freq_cmd = self.build_frame(0x03);
            self.write_frame(port, &freq_cmd)?;
            let freq = self.read_response_matching(
                port,
                Duration::from_millis(1200),
                Some(&freq_cmd),
                |frame| frame_matches_request(frame, &[0x03]),
            )?;
            let freq_value = parse_frequency(&freq);

            let mode_cmd = self.build_frame(0x04);
            self.write_frame(port, &mode_cmd)?;
            let mode = self.read_response_matching(
                port,
                Duration::from_millis(1200),
                Some(&mode_cmd),
                |frame| frame_matches_request(frame, &[0x04]),
            )?;
            let mode_details = parse_mode_details(&mode);
            let mode_value = mode_details.map(|m| m.label());

            Ok(RadioStatus {
                frequency_hz: freq_value,
                mode: mode_value,
                mode_details,
            })
        })
    }

    fn probe_stream_status_blocking(&self) -> Result<RadioStatus> {
        self.with_serial_port(Duration::from_millis(300), |port| {
            let freq_cmd = self.build_frame(0x03);
            self.write_frame(port, &freq_cmd)?;
            let freq = self.read_response_matching(
                port,
                Duration::from_millis(320),
                Some(&freq_cmd),
                |frame| frame_matches_request(frame, &[0x03]),
            )?;
            let freq_value = parse_frequency(&freq);

            let mode_cmd = self.build_frame(0x04);
            self.write_frame(port, &mode_cmd)?;
            let mode = self.read_response_matching(
                port,
                Duration::from_millis(320),
                Some(&mode_cmd),
                |frame| frame_matches_request(frame, &[0x04]),
            )?;
            let mode_details = parse_mode_details(&mode);
            let mode_value = mode_details.map(|m| m.label());

            Ok(RadioStatus {
                frequency_hz: freq_value,
                mode: mode_value,
                mode_details,
            })
        })
    }

    fn build_frame(&self, cmd: u8) -> Vec<u8> {
        self.build_frame_payload(&[cmd])
    }

    fn build_frame_payload(&self, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + payload.len() + 1);
        out.push(CI_V_FRAME_START);
        out.push(CI_V_FRAME_START);
        out.push(self.radio_address);
        out.push(self.controller_address);
        out.extend_from_slice(payload);
        out.push(CI_V_FRAME_END);
        out
    }

    fn with_serial_port<T, F>(&self, timeout: Duration, mut operation: F) -> Result<T>
    where
        F: FnMut(&mut Box<dyn SerialPort>) -> Result<T>,
    {
        let mut guard = self
            .serial_port
            .lock()
            .map_err(|_| anyhow::anyhow!("radio serial port lock poisoned"))?;

        if guard.is_none() {
            *guard = Some(self.open_port_with_timeout(timeout)?);
        }

        let port = guard
            .as_mut()
            .context("radio serial port slot unavailable")?;
        port.set_timeout(timeout)
            .context("failed to update radio serial timeout")?;
        operation(port)
    }

    fn open_port_with_timeout(&self, timeout: Duration) -> Result<Box<dyn SerialPort>> {
        let configured_port = self.port.trim();
        let candidates = if configured_port.is_empty() {
            enumerate_serial_ports().unwrap_or_default()
        } else {
            vec![configured_port.to_string()]
        };

        let mut failures = Vec::new();
        for candidate in &candidates {
            let open_result = serialport::new(candidate, self.baud_rate)
                .timeout(timeout)
                .open();
            match open_result {
                Ok(port) => {
                    eprintln!("[rigwright] opened serial port: {candidate}");
                    return Ok(port);
                }
                Err(err) => {
                    eprintln!("[rigwright] failed to open serial port: {candidate}: {err}");
                    failures.push(format!("{candidate}: {err}"));
                }
            }
        }

        Err(anyhow!(
            "failed to open serial port{} (tried: {})",
            if configured_port.is_empty() {
                String::new()
            } else {
                format!(" {}", configured_port)
            },
            failures.join("; ")
        ))
    }

    fn close_serial_port(&self) {
        if let Ok(mut guard) = self.serial_port.lock() {
            *guard = None;
        }
        if let Ok(mut reader) = self.scope_stream_reader.lock() {
            *reader = ScopeStreamReader::default();
        }
    }

    fn write_frame(&self, port: &mut Box<dyn SerialPort>, frame: &[u8]) -> Result<()> {
        port.write_all(frame)
            .context("failed to write CI-V frame")?;
        Ok(())
    }

    fn read_response_matching<F>(
        &self,
        port: &mut Box<dyn SerialPort>,
        timeout: Duration,
        echo_frame: Option<&[u8]>,
        mut matcher: F,
    ) -> Result<Vec<u8>>
    where
        F: FnMut(&[u8]) -> bool,
    {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 1024];
        let mut pending = Vec::new();

        while Instant::now() < deadline {
            match port.read(&mut buf) {
                Ok(bytes) if bytes > 0 => {
                    // CI-V scope frames are unsolicited and can be interleaved
                    // with command replies. Feed every byte through the
                    // persistent scope parser before matching the requested
                    // response, otherwise normal CAT traffic punches holes in
                    // the waterfall and leaves the next sweep half-framed.
                    self.scope_stream_reader
                        .lock()
                        .map_err(|_| anyhow!("CI-V scope stream state lock poisoned"))?
                        .ingest_bytes(
                            &buf[..bytes],
                            self.radio_address,
                            self.controller_address,
                            self.model
                                .and_then(|model| profile_for_model(model).scope_geometry),
                        );
                    pending.extend_from_slice(&buf[..bytes]);
                    for frame in drain_ci_v_frames(&mut pending) {
                        // With CI-V USB Echo Back enabled, the radio/USB
                        // interface returns the exact outbound frame before
                        // the real ACK or data response. It is transport
                        // noise, not a response to match.
                        if echo_frame.is_some_and(|echo| frame == echo) {
                            continue;
                        }
                        if !is_radio_to_controller_frame(
                            &frame,
                            self.radio_address,
                            self.controller_address,
                        ) {
                            continue;
                        }

                        if matcher(&frame) {
                            return Ok(frame);
                        }
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == ErrorKind::TimedOut => {
                    // keep waiting until timeout
                }
                Err(err) => return Err(err).context("failed to read matched CI-V response"),
            }

            thread::sleep(Duration::from_millis(10));
        }

        Ok(Vec::new())
    }

    fn transact(&self, payload: &[u8], expect_data_frame: bool) -> Result<Vec<u8>> {
        let frame = self.build_frame_payload(payload);
        let response = self.with_serial_port(Duration::from_millis(700), |port| {
            self.write_frame(port, &frame)?;

            if expect_data_frame {
                self.read_response_matching(
                    port,
                    Duration::from_millis(1500),
                    Some(&frame),
                    |response| frame_matches_request(response, payload),
                )
            } else {
                self.read_response_matching(
                    port,
                    Duration::from_millis(1_200),
                    Some(&frame),
                    |response| is_ack_frame(response) || is_nak_frame(response),
                )
            }
        })?;
        if expect_data_frame || is_ack_frame(&response) {
            Ok(response)
        } else if is_nak_frame(&response) {
            anyhow::bail!("radio rejected CI-V command: {}", format_hex_bytes(payload))
        } else {
            anyhow::bail!(
                "radio did not acknowledge CI-V command: {}",
                format_hex_bytes(payload)
            )
        }
    }

    pub(crate) fn transact_ack(&self, payload: &[u8]) -> Result<()> {
        self.transact(payload, false).map(|_| ())
    }

    fn set_frequency_blocking(&self, hz: u64) -> Result<()> {
        anyhow::ensure!(
            hz <= 9_999_999_999,
            "frequency {hz} Hz does not fit the five-byte CI-V BCD format"
        );
        if let Some(model) = self.model {
            if !profile_for_model(model).supports_frequency(hz) {
                anyhow::bail!(
                    "frequency {hz} Hz is outside the documented CAT range for {}",
                    model.model_name()
                );
            }
        }
        let mut payload = Vec::with_capacity(1 + 5);
        payload.push(0x05);
        payload.extend_from_slice(&encode_civ_frequency_bcd(hz));
        let _ = self.transact(&payload, false)?;
        Ok(())
    }

    pub fn supports_scope(&self) -> bool {
        self.model
            .map(|model| profile_for_model(model).scope_geometry.is_some())
            .unwrap_or(false)
    }

    fn set_mode_blocking(&self, mode: Mode) -> Result<()> {
        if self.model.is_none() {
            anyhow::ensure!(
                mode != Mode::Data,
                "setting CI-V data mode requires a selected Icom model profile"
            );
            return self
                .transact(&[0x06, mode_to_civ_mode(mode)?], false)
                .map(|_| ());
        }
        let (base_mode, data_mode) = hal_mode_to_icom_operating_mode(mode);
        let current_filter = if self.model.is_some() {
            self.transact(&[0x26, 0x00], true)
                .ok()
                .and_then(|response| parse_mode_details(&response))
                .and_then(|details| details.filter)
                .unwrap_or(1)
        } else {
            1
        };
        self.set_operating_mode_blocking(base_mode, data_mode, current_filter)
    }

    fn set_ptt_blocking(&self, enabled: bool) -> Result<()> {
        let payload = [0x1C, 0x00, if enabled { 0x01 } else { 0x00 }];
        let _ = self.transact(&payload, false)?;
        Ok(())
    }

    fn set_power_blocking(&self, enabled: bool) -> Result<()> {
        if !enabled {
            let _ = self.transact(&[0x18, 0x00], false)?;
            return Ok(());
        }

        // Icom requires a stream of FE preamble bytes before the power-on
        // command when the radio is already off. The documented counts are
        // time-based examples: 15 at 4800, 30 at 9600, and 60 at 19200.
        // Preserve that preamble duration at faster configured baud rates.
        let preamble_count = (self.baud_rate / 320).max(15) as usize;
        let frame = self.build_frame_payload(&[0x18, 0x01]);
        self.with_serial_port(Duration::from_millis(700), |port| {
            port.write_all(&vec![0xFE; preamble_count])
                .context("failed to write CI-V power-on preamble")?;
            self.write_frame(port, &frame)?;
            port.flush()
                .context("failed to flush CI-V power-on command")
        })
    }

    fn get_frequency_blocking(&self) -> Result<u64> {
        let response = self.transact(&[0x03], true)?;
        parse_frequency(&response).context("frequency not present in CI-V response")
    }

    fn get_mode_blocking(&self) -> Result<Mode> {
        let request: &[u8] = if self.model.is_some() {
            &[0x26, 0x00]
        } else {
            &[0x04]
        };
        let response = self.transact(request, true)?;
        parse_mode(&response).context("mode not present or unsupported in CI-V response")
    }

    fn get_ptt_blocking(&self) -> Result<bool> {
        let response = self.transact(&[0x1C, 0x00], true)?;
        let data = response_data_after_prefix(&response, &[0x1C, 0x00])?;
        match data.first().copied() {
            Some(0x00) => Ok(false),
            Some(0x01) => Ok(true),
            Some(value) => anyhow::bail!("invalid CI-V PTT state: {value:#04x}"),
            None => anyhow::bail!("missing CI-V PTT state"),
        }
    }

    fn get_meter_blocking(&self, id: MeterId) -> Result<u8> {
        let prefix: &[u8] = match id {
            // IC-7300 manual, CI-V command table: 15 12, SWR meter.
            MeterId::Swr => &[0x15, 0x12],
            _ => anyhow::bail!("CI-V meter {id:?} is not implemented for this profile"),
        };
        let response = self.transact(prefix, true)?;
        let data = response_data_after_prefix(&response, prefix)?;
        decode_level_255_bcd(data).context("invalid CI-V meter payload")
    }

    fn set_tuner_enabled_blocking(&self, enabled: bool) -> Result<()> {
        self.transact_ack(&[0x1C, 0x01, if enabled { 0x01 } else { 0x00 }])
    }

    fn start_tuner_blocking(&self) -> Result<()> {
        self.transact_ack(&[0x1C, 0x01, 0x02])
    }

    fn get_tuner_status_blocking(&self) -> Result<TunerStatus> {
        let response = self.transact(&[0x1C, 0x01], true)?;
        let data = response_data_after_prefix(&response, &[0x1C, 0x01])?;
        match *data.first().context("missing CI-V tuner status")? {
            0x00 => Ok(TunerStatus {
                enabled: false,
                tuning: false,
            }),
            0x01 => Ok(TunerStatus {
                enabled: true,
                tuning: false,
            }),
            0x02 => Ok(TunerStatus {
                enabled: true,
                tuning: true,
            }),
            other => anyhow::bail!("invalid CI-V tuner status: {other:#04x}"),
        }
    }

    fn run_control_op(
        &self,
        id: ControlId,
        op: ControlOp,
        value: Option<ControlValue>,
    ) -> Result<Option<ControlValue>> {
        if matches!(
            id,
            ControlId::Vfo | ControlId::MainSub | ControlId::ExternalPreamp
        ) || (id == ControlId::Preamp
            && profile_for_model(self.selected_model()?)
                .external_preamp
                .is_some())
        {
            return self.run_model_specific_control(id, op, value);
        }
        if id == ControlId::DataMode {
            self.selected_model()?;
            return match op {
                ControlOp::Set => {
                    let enabled = match value.context("missing control value for set operation")? {
                        ControlValue::Bool(v) => v,
                        _ => anyhow::bail!("DataMode control expects bool value"),
                    };
                    if enabled {
                        let _ = self.transact(&[0x1A, 0x06, 0x01, 0x01], false)?;
                    } else {
                        let _ = self.transact(&[0x1A, 0x06, 0x00, 0x00], false)?;
                    }
                    Ok(None)
                }
                ControlOp::Get => {
                    let response = self.transact(&[0x26, 0x00], true)?;
                    let details =
                        parse_mode_details(&response).context("unable to decode mode details")?;
                    Ok(Some(ControlValue::Bool(details.data_mode)))
                }
            };
        }

        if id == ControlId::Tuner {
            return match op {
                ControlOp::Set => {
                    let enabled = match value.context("missing tuner value")? {
                        ControlValue::Bool(v) => v,
                        _ => anyhow::bail!("Tuner control expects bool value"),
                    };
                    self.set_tuner_enabled_blocking(enabled)?;
                    Ok(None)
                }
                ControlOp::Get => Ok(Some(ControlValue::Bool(
                    self.get_tuner_status_blocking()?.enabled,
                ))),
            };
        }

        if id == ControlId::Filter {
            self.selected_model()?;
            return match op {
                ControlOp::Set => {
                    let filter = match value.context("missing control value for set operation")? {
                        ControlValue::U8(v) if (1..=3).contains(&v) => v,
                        _ => anyhow::bail!("Filter control expects U8 value in 1..=3"),
                    };
                    let response = self.transact(&[0x26, 0x00], true)?;
                    let details = parse_mode_details(&response)
                        .context("unable to decode current mode for filter set")?;
                    let mode_byte = base_mode_to_civ_mode(details.base)
                        .context("unsupported base mode for filter set")?;
                    let data_byte = if details.data_mode { 0x01 } else { 0x00 };
                    let _ = self.transact(&[0x26, 0x00, mode_byte, data_byte, filter], false)?;
                    Ok(None)
                }
                ControlOp::Get => {
                    let response = self.transact(&[0x26, 0x00], true)?;
                    let details =
                        parse_mode_details(&response).context("unable to decode mode details")?;
                    Ok(details.filter.map(ControlValue::U8))
                }
            };
        }

        if id == ControlId::Attenuator {
            let profile = profile_for_model(self.selected_model()?);
            if let Some(ControlValue::U8(db)) = value.as_ref() {
                if !profile.attenuator_values.contains(db) {
                    anyhow::bail!(
                        "attenuator setting {db} dB is not documented for {}",
                        self.selected_model()?.model_name()
                    );
                }
            }
        }
        if id == ControlId::Preamp {
            let profile = profile_for_model(self.selected_model()?);
            if let Some(ControlValue::U8(level)) = value.as_ref() {
                if *level > profile.preamp_max_level {
                    anyhow::bail!(
                        "preamp level {level} exceeds {} levels for {}",
                        profile.preamp_max_level,
                        self.selected_model()?.model_name()
                    );
                }
            }
        }

        let spec = profile_for_model(self.selected_model()?)
            .control(id)
            .with_context(|| format!("unsupported CI-V control: {id:?}"))?;

        match op {
            ControlOp::Set => {
                let v = value.context("missing control value for set operation")?;
                let encoded = encode_control_value(
                    match spec.encoding {
                        super::profile::ControlEncoding::Bool => ControlEncoding::Bool,
                        super::profile::ControlEncoding::U8 => ControlEncoding::U8,
                        super::profile::ControlEncoding::Level255Bcd => {
                            ControlEncoding::Level255Bcd
                        }
                    },
                    v,
                )?;
                let mut payload = Vec::with_capacity(spec.command_prefix.len() + encoded.len());
                payload.extend_from_slice(spec.command_prefix);
                payload.extend_from_slice(&encoded);
                let _ = self.transact(&payload, false)?;
                Ok(None)
            }
            ControlOp::Get => {
                let response = self.transact(spec.command_prefix, true)?;
                let decoded = decode_profile_control_response(spec, &response)?;
                Ok(Some(decoded))
            }
        }
    }

    fn run_model_specific_control(
        &self,
        id: ControlId,
        op: ControlOp,
        value: Option<ControlValue>,
    ) -> Result<Option<ControlValue>> {
        let profile = profile_for_model(self.selected_model()?);
        match (id, op) {
            (ControlId::MainSub, ControlOp::Set) if profile.main_sub.is_some() => {
                let value = match value.context("missing MainSub value")? {
                    ControlValue::Mode(_) => anyhow::bail!("MainSub expects IcomReceiver"),
                    ControlValue::U8(value) if value <= 1 => value,
                    _ => anyhow::bail!("MainSub expects U8 0=main or 1=sub"),
                };
                let spec = profile.main_sub.expect("profile checked");
                self.transact(&[spec.set_command, spec.set_subcommand_base + value], false)?;
                Ok(None)
            }
            (ControlId::MainSub, _) if profile.main_sub.is_none() => {
                anyhow::bail!(
                    "main/sub selection is not supported for {}",
                    self.selected_model()?.model_name()
                )
            }
            (ControlId::MainSub, ControlOp::Get) => {
                let spec = profile.main_sub.expect("profile checked");
                let response = self.transact(&[spec.get_command, spec.get_subcommand], true)?;
                let data = response_data_after_prefix(
                    &response,
                    &[spec.get_command, spec.get_subcommand],
                )?;
                let selected = *data.first().context("missing main/sub selection data")?;
                anyhow::ensure!(
                    selected <= 1,
                    "invalid main/sub selection value: {selected}"
                );
                Ok(Some(ControlValue::Receiver(selected)))
            }
            (ControlId::ExternalPreamp, ControlOp::Set) if profile.external_preamp.is_some() => {
                let enabled = match value.context("missing ExternalPreamp value")? {
                    ControlValue::Bool(value) => value,
                    _ => anyhow::bail!("ExternalPreamp expects bool"),
                };
                let spec = profile.external_preamp.expect("profile checked");
                let current = self.read_u8_control(spec.command_prefix)?;
                let combined = if enabled {
                    current | spec.enabled_mask
                } else {
                    current & !spec.enabled_mask
                };
                let mut payload = spec.command_prefix.to_vec();
                payload.push(combined);
                self.transact(&payload, false)?;
                Ok(None)
            }
            (ControlId::ExternalPreamp, ControlOp::Get) if profile.external_preamp.is_some() => {
                let spec = profile.external_preamp.expect("profile checked");
                let combined = self.read_u8_control(spec.command_prefix)?;
                Ok(Some(ControlValue::Bool(combined & spec.enabled_mask != 0)))
            }
            (ControlId::ExternalPreamp, _) if profile.external_preamp.is_none() => {
                anyhow::bail!(
                    "external preamp is not supported for {}",
                    self.selected_model()?.model_name()
                )
            }
            (ControlId::Vfo, ControlOp::Set) => {
                let value = match value.context("missing VFO value")? {
                    ControlValue::U8(value) if value <= 1 => value,
                    _ => anyhow::bail!("VFO expects U8 0=A or 1=B"),
                };
                self.transact(&[0x07, value], false)?;
                Ok(None)
            }
            (ControlId::Preamp, op) if profile.external_preamp.is_some() => {
                let spec = profile.external_preamp.expect("profile checked");
                let combined = self.read_u8_control(spec.command_prefix)?;
                match op {
                    ControlOp::Get => Ok(Some(ControlValue::U8(combined & !spec.enabled_mask))),
                    ControlOp::Set => {
                        let level = match value.context("missing Preamp value")? {
                            ControlValue::U8(level) if level <= profile.preamp_max_level => level,
                            _ => anyhow::bail!("Preamp expects a documented U8 level"),
                        };
                        let mut payload = spec.command_prefix.to_vec();
                        payload.push((combined & spec.enabled_mask) | level);
                        self.transact(&payload, false)?;
                        Ok(None)
                    }
                }
            }
            _ => anyhow::bail!(
                "control {id:?} is not supported for {}",
                self.selected_model()?.model_name()
            ),
        }
    }

    fn read_u8_control(&self, command_prefix: &[u8]) -> Result<u8> {
        let response = self.transact(command_prefix, true)?;
        let data = response_data_after_prefix(&response, command_prefix)?;
        data.first().copied().context("missing U8 control data")
    }

    fn enable_spectrum_stream_blocking(&self, timeout: Duration) -> Result<Vec<u8>> {
        let model = self.selected_model()?;
        let scope = profile_for_model(model)
            .scope
            .context("native CI-V scope streaming is not implemented for this model")?;
        if profile_for_model(model).scope_geometry.is_none() {
            anyhow::bail!(
                "native CI-V scope streaming is not implemented for {}",
                model.model_name()
            );
        }
        self.transact_ack(scope.enable_command)?;
        self.transact_ack(scope.stream_command)?;
        self.try_scope_waveform_bins_stream_blocking(timeout)?
            .context("scope output enabled but no complete scope sweep arrived")
    }

    fn disable_spectrum_stream_blocking(&self) -> Result<()> {
        let scope = profile_for_model(self.selected_model()?)
            .scope
            .context("native CI-V scope streaming is not implemented for this model")?;
        self.transact_ack(scope.disable_stream_command)?;
        self.close_serial_port();
        Ok(())
    }

    pub async fn enable_spectrum_stream(&self, timeout: Duration) -> Result<Vec<u8>> {
        self.enable_spectrum_stream_blocking(timeout)
    }

    pub async fn disable_spectrum_stream(&self) -> Result<()> {
        self.disable_spectrum_stream_blocking()
    }

    pub async fn request_scope_waveform_bins(&self) -> Result<Vec<u8>> {
        self.request_scope_waveform_bins_blocking()
    }

    pub async fn try_scope_waveform_bins_stream(
        &self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        self.try_scope_waveform_bins_stream_blocking(timeout)
    }

    pub async fn drain_scope_waveform_sweeps(&self, timeout: Duration) -> Result<Vec<Vec<u8>>> {
        self.drain_scope_waveform_sweeps_blocking(timeout)
    }

    /// Lifetime counters for diagnosing CI-V scope cadence.
    pub fn scope_stream_counters(&self) -> (u64, u64, u64) {
        self.scope_stream_reader
            .lock()
            .map(|reader| {
                (
                    reader.division_frames,
                    reader.completed_sweeps,
                    reader.assembler.dropped_sweeps,
                )
            })
            .unwrap_or_default()
    }

    fn request_scope_waveform_bins_blocking(&self) -> Result<Vec<u8>> {
        let request = self.build_frame_payload(&[0x27, 0x00]);
        let result = self.with_serial_port(Duration::from_millis(700), |port| {
            self.write_frame(port, &request)?;
            self.read_stream_scope_bins(port, Duration::from_millis(1_500))
        });
        match result {
            Ok(Some(bins)) => Ok(bins),
            Ok(None) => {
                let bins = profile_for_model(self.selected_model()?)
                    .scope_geometry
                    .map(|geometry| geometry.bins)
                    .unwrap_or_default();
                anyhow::bail!("no complete {bins}-bin scope sweep arrived")
            }
            Err(err) => {
                self.close_serial_port();
                Err(err)
            }
        }
    }

    fn try_scope_waveform_bins_stream_blocking(
        &self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        let result = self
            .drain_scope_waveform_sweeps_blocking(timeout)
            .map(|sweeps| sweeps.into_iter().next());
        if result.is_err() {
            self.close_serial_port();
        }
        result
    }

    fn drain_scope_waveform_sweeps_blocking(&self, timeout: Duration) -> Result<Vec<Vec<u8>>> {
        let result = self.with_serial_port(Duration::from_millis(25), |port| {
            self.read_stream_scope_sweeps(port, timeout)
        });
        if result.is_err() {
            self.close_serial_port();
        }
        result
    }

    fn read_stream_scope_bins(
        &self,
        port: &mut Box<dyn SerialPort>,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self
            .read_stream_scope_sweeps(port, timeout)?
            .into_iter()
            .next())
    }

    fn read_stream_scope_sweeps(
        &self,
        port: &mut Box<dyn SerialPort>,
        timeout: Duration,
    ) -> Result<Vec<Vec<u8>>> {
        let deadline = Instant::now() + timeout;
        // Keep reads small enough that completed sweeps reach the UI at their
        // native cadence instead of arriving as several-frame visual bursts.
        let mut read_buffer = [0_u8; 512];
        let mut sweeps = Vec::new();
        let mut reader = self
            .scope_stream_reader
            .lock()
            .map_err(|_| anyhow!("CI-V scope stream state lock poisoned"))?;

        while Instant::now() < deadline {
            match port.read(&mut read_buffer) {
                Ok(bytes) if bytes > 0 => {
                    sweeps.extend(reader.push_bytes(
                        &read_buffer[..bytes],
                        self.radio_address,
                        self.controller_address,
                        profile_for_model(self.selected_model()?).scope_geometry,
                    ));
                    if !sweeps.is_empty() {
                        return Ok(sweeps);
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == ErrorKind::TimedOut => {}
                Err(err) => return Err(err).context("failed to read CI-V scope stream"),
            }
        }

        Ok(sweeps)
    }
}

fn parse_scope_waveform_segment(
    frame: &[u8],
    geometry: Option<crate::models::IcomScopeGeometry>,
) -> Option<(usize, usize, Vec<u8>)> {
    if frame.len() < 10 {
        return None;
    }
    if frame.first().copied() != Some(CI_V_FRAME_START)
        || frame.get(1).copied() != Some(CI_V_FRAME_START)
        || frame.last().copied() != Some(CI_V_FRAME_END)
    {
        return None;
    }
    if frame[4] != 0x27 || frame[5] != 0x00 {
        return None;
    }

    let payload = &frame[6..frame.len() - 1];
    if payload.len() < 3 {
        return None;
    }
    let geometry = geometry?;
    // All currently profiled manuals put a model-specific scope selector
    // (fixed 00 on single-scope rigs, MAIN/SUB on dual-scope rigs) first,
    // followed by current and maximum division numbers.
    let current = decode_scope_division_number(*payload.get(1)?, geometry.divisions);
    let total = decode_scope_division_number(*payload.get(2)?, geometry.divisions);
    let (current, total) = (current?, total?);
    let bins = if current == 1 {
        Vec::new()
    } else {
        payload[3..].to_vec()
    };
    Some((current as usize, total as usize, bins))
}

fn decode_scope_division_number(v: u8, maximum: usize) -> Option<u8> {
    if (1..=maximum.min(99)).contains(&(v as usize)) {
        return Some(v);
    }

    let low = v & 0x0F;
    let high = (v >> 4) & 0x0F;
    if low > 9 || high > 9 {
        return None;
    }
    let n = high * 10 + low;
    if (1..=maximum.min(99) as u8).contains(&n) {
        Some(n)
    } else {
        None
    }
}

#[async_trait]
impl Radio for IcomCiVRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        self.get_frequency_blocking()
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        self.set_frequency_blocking(hz)
    }

    async fn get_mode(&self) -> Result<Mode> {
        self.get_mode_blocking()
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        self.set_mode_blocking(mode)
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.set_ptt_blocking(enabled)
    }

    async fn get_ptt(&self) -> Result<bool> {
        self.get_ptt_blocking()
    }

    async fn get_power(&self) -> Result<bool> {
        anyhow::bail!("Icom CI-V power state is write-only")
    }

    async fn set_power(&self, enabled: bool) -> Result<()> {
        self.set_power_blocking(enabled)
    }

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        if request.first().copied() != Some(CI_V_FRAME_START)
            || request.get(1).copied() != Some(CI_V_FRAME_START)
            || request.last().copied() != Some(CI_V_FRAME_END)
        {
            anyhow::bail!("raw CI-V request must be a full frame: FE FE ... FD");
        }

        self.with_serial_port(Duration::from_millis(700), |port| {
            self.write_frame(port, request)?;
            self.read_response_matching(
                port,
                Duration::from_millis(1_200),
                Some(request),
                |frame| !is_spectrum_data_frame(frame),
            )
        })
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        if id == ControlId::RawCiV {
            return Ok(None);
        }
        self.run_control_op(id, ControlOp::Get, None)
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        Ok(Some(self.get_meter_blocking(id)?))
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        self.model().is_some() && matches!(id, MeterId::Swr)
    }

    fn supports_control(&self, id: ControlId) -> bool {
        let Some(model) = self.model() else {
            return false;
        };
        let profile = profile_for_model(model);
        profile.control(id).is_some()
            || matches!(
                id,
                ControlId::DataMode | ControlId::Filter | ControlId::RawCiV | ControlId::Vfo
            )
            || (id == ControlId::MainSub && profile.main_sub.is_some())
            || (id == ControlId::ExternalPreamp && profile.external_preamp.is_some())
    }

    async fn start_tuner(&self) -> Result<()> {
        self.start_tuner_blocking()
    }

    async fn get_tuner_status(&self) -> Result<Option<TunerStatus>> {
        Ok(Some(self.get_tuner_status_blocking()?))
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        if id == ControlId::RawCiV {
            anyhow::bail!("RawCiV is write/read through protocol_write_read, not set_control");
        }
        self.run_control_op(id, ControlOp::Set, Some(value))?;
        Ok(())
    }

    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: true,
            can_set_mode: true,
            can_get_ptt: true,
            can_set_ptt: true,
            can_get_power: false,
            can_set_power: true,
            can_raw_protocol: true,
        }
    }
}

#[cfg(test)]
fn extract_ci_v_frames(response: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    let mut idx = 0usize;

    while idx + 1 < response.len() {
        if response[idx] == CI_V_FRAME_START && response[idx + 1] == CI_V_FRAME_START {
            if let Some(rel_end) = response[idx..].iter().position(|&b| b == CI_V_FRAME_END) {
                let end = idx + rel_end;
                if end >= idx + 5 {
                    frames.push(response[idx..=end].to_vec());
                }
                idx = end + 1;
                continue;
            }
            break;
        }
        idx += 1;
    }

    frames
}

fn drain_ci_v_frames(pending: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    loop {
        let Some(start) = pending
            .windows(2)
            .position(|window| window == [CI_V_FRAME_START, CI_V_FRAME_START])
        else {
            if pending.last().copied() == Some(CI_V_FRAME_START) {
                pending.drain(..pending.len() - 1);
            } else {
                pending.clear();
            }
            break;
        };
        if start > 0 {
            pending.drain(..start);
        }
        let Some(end) = pending.iter().position(|byte| *byte == CI_V_FRAME_END) else {
            break;
        };
        let frame: Vec<u8> = pending.drain(..=end).collect();
        if frame.len() >= 6 {
            frames.push(frame);
        }
    }
    frames
}

fn is_spectrum_data_frame(frame: &[u8]) -> bool {
    if frame.len() < 8 {
        return false;
    }

    let is_legacy = frame[4] == 0x27 && frame[5] == 0x12 && frame[6] == 0x01;
    let is_scope_wave = frame[4] == 0x27 && frame[5] == 0x00;

    frame.first().copied() == Some(CI_V_FRAME_START)
        && frame.get(1).copied() == Some(CI_V_FRAME_START)
        && frame.last().copied() == Some(CI_V_FRAME_END)
        && (is_legacy || is_scope_wave)
}

fn is_ack_frame(frame: &[u8]) -> bool {
    frame.len() >= 6
        && frame.first().copied() == Some(CI_V_FRAME_START)
        && frame.get(1).copied() == Some(CI_V_FRAME_START)
        && frame.last().copied() == Some(CI_V_FRAME_END)
        && frame[4] == 0xFB
}

fn is_nak_frame(frame: &[u8]) -> bool {
    frame.len() >= 6
        && frame.first().copied() == Some(CI_V_FRAME_START)
        && frame.get(1).copied() == Some(CI_V_FRAME_START)
        && frame.last().copied() == Some(CI_V_FRAME_END)
        && frame[4] == 0xFA
}

fn format_hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_frequency(response: &[u8]) -> Option<u64> {
    let payload = response
        .windows(2)
        .position(|w| w[0] == CI_V_FRAME_START && w[1] == CI_V_FRAME_START)
        .map(|idx| &response[idx..])?;

    let end = payload.iter().position(|&b| b == CI_V_FRAME_END)?;
    let frame = &payload[..=end];
    if frame.len() <= 6 {
        return None;
    }
    if frame[4] != 0x03 {
        return None;
    }

    let data = &frame[5..frame.len() - 1];
    if data.is_empty() {
        return None;
    }

    // CI-V frequency payload is little-endian BCD bytes (command 0x03/0x00 style).
    // IC-7300 returns 5 bytes for frequency. If there is a selector byte present,
    // the payload still starts at the data area and we decode the first 5 bytes.
    let freq_bytes = if data.len() >= 5 { &data[..5] } else { data };
    decode_civ_frequency_bcd(freq_bytes)
}

fn decode_civ_frequency_bcd(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return None;
    }

    let mut value = 0u64;
    let mut place = 1u64;

    for &b in bytes {
        let low = b & 0x0F;
        let high = (b >> 4) & 0x0F;
        if low > 9 || high > 9 {
            return None;
        }

        value += (low as u64) * place;
        place *= 10;
        value += (high as u64) * place;
        place *= 10;
    }

    Some(value)
}

fn hal_mode_to_icom_operating_mode(mode: Mode) -> (BaseMode, bool) {
    match mode {
        Mode::Data => (BaseMode::Usb, true),
        Mode::Lsb => (BaseMode::Lsb, false),
        Mode::Usb => (BaseMode::Usb, false),
        Mode::Cw => (BaseMode::Cw, false),
        Mode::Am => (BaseMode::Am, false),
        Mode::Fm => (BaseMode::Fm, false),
        Mode::Wfm => (BaseMode::Wfm, false),
        Mode::Rtty => (BaseMode::Rtty, false),
        Mode::CwReverse => (BaseMode::CwR, false),
        Mode::RttyReverse => (BaseMode::RttyR, false),
    }
}

pub(crate) fn encode_civ_frequency_bcd(hz: u64) -> [u8; 5] {
    let mut remaining = hz;
    let mut out = [0u8; 5];
    for b in &mut out {
        let low = (remaining % 10) as u8;
        remaining /= 10;
        let high = (remaining % 10) as u8;
        remaining /= 10;
        *b = (high << 4) | low;
    }
    out
}

fn mode_to_civ_mode(mode: Mode) -> Result<u8> {
    let v = match mode {
        Mode::Lsb => 0x00,
        Mode::Usb => 0x01,
        Mode::Cw => 0x03,
        Mode::Data => 0x01,
        Mode::Am => 0x02,
        Mode::Fm => 0x05,
        Mode::Wfm => 0x06,
        Mode::Rtty => 0x04,
        Mode::CwReverse => 0x07,
        Mode::RttyReverse => 0x08,
    };
    Ok(v)
}

#[cfg(test)]
fn civ_mode_to_mode(mode: u8) -> Option<Mode> {
    match mode {
        0x00 => Some(Mode::Lsb),
        0x01 => Some(Mode::Usb),
        0x02 => Some(Mode::Am),
        0x03 => Some(Mode::Cw),
        0x04 => Some(Mode::Rtty),
        0x05 => Some(Mode::Fm),
        0x07 => Some(Mode::CwReverse),
        0x08 => Some(Mode::RttyReverse),
        _ => None,
    }
}

fn civ_mode_to_base_mode(mode: u8) -> BaseMode {
    match mode {
        0x00 => BaseMode::Lsb,
        0x01 => BaseMode::Usb,
        0x02 => BaseMode::Am,
        0x03 => BaseMode::Cw,
        0x04 => BaseMode::Rtty,
        0x05 => BaseMode::Fm,
        0x06 => BaseMode::Wfm,
        0x07 => BaseMode::CwR,
        0x08 => BaseMode::RttyR,
        other => BaseMode::Unknown(other),
    }
}

fn base_mode_to_civ_mode(mode: BaseMode) -> Option<u8> {
    match mode {
        BaseMode::Lsb => Some(0x00),
        BaseMode::Usb => Some(0x01),
        BaseMode::Am => Some(0x02),
        BaseMode::Cw => Some(0x03),
        BaseMode::Rtty => Some(0x04),
        BaseMode::Fm => Some(0x05),
        BaseMode::Wfm => Some(0x06),
        BaseMode::CwR => Some(0x07),
        BaseMode::RttyR => Some(0x08),
        BaseMode::Unknown(_) => None,
    }
}

fn encode_control_value(encoding: ControlEncoding, value: ControlValue) -> Result<Vec<u8>> {
    match (encoding, value) {
        (ControlEncoding::Bool, ControlValue::Bool(v)) => Ok(vec![if v { 1 } else { 0 }]),
        (ControlEncoding::U8, ControlValue::U8(v)) => Ok(vec![v]),
        (ControlEncoding::Level255Bcd, ControlValue::U8(v)) => Ok(encode_level_255_bcd(v)),
        _ => anyhow::bail!("control value type does not match control encoding"),
    }
}

fn decode_profile_control_response(
    spec: &super::profile::ControlSpec,
    response: &[u8],
) -> Result<ControlValue> {
    let data = response_data_after_prefix(response, spec.command_prefix)?;
    Ok(match spec.encoding {
        super::profile::ControlEncoding::Bool => {
            ControlValue::Bool(*data.first().context("missing boolean control data")? != 0)
        }
        super::profile::ControlEncoding::U8 => {
            ControlValue::U8(*data.first().context("missing U8 control data")?)
        }
        super::profile::ControlEncoding::Level255Bcd => {
            ControlValue::U8(decode_level_255_bcd(data).context("invalid BCD level payload")?)
        }
    })
}

fn response_data_after_prefix<'a>(response: &'a [u8], prefix: &[u8]) -> Result<&'a [u8]> {
    anyhow::ensure!(!prefix.is_empty(), "CI-V command prefix cannot be empty");
    let start = response
        .windows(2)
        .position(|w| w == [CI_V_FRAME_START, CI_V_FRAME_START])
        .context("no CI-V frame in response")?;
    let end = response[start..]
        .iter()
        .position(|&b| b == CI_V_FRAME_END)
        .map(|i| i + start)
        .context("no CI-V frame terminator")?;
    let frame = &response[start..=end];
    anyhow::ensure!(frame.len() >= 6, "CI-V control response is too short");
    let payload = &frame[4..frame.len() - 1];
    if !payload.starts_with(prefix) {
        anyhow::bail!("CI-V control response command mismatch");
    }
    Ok(&payload[prefix.len()..])
}

fn encode_level_255_bcd(v: u8) -> Vec<u8> {
    let hundreds = v / 100;
    let tens = (v / 10) % 10;
    let ones = v % 10;
    vec![hundreds, (tens << 4) | ones]
}

fn decode_level_255_bcd(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 2 {
        return None;
    }
    let high = bytes[0];
    let tens = (bytes[1] >> 4) & 0x0F;
    let ones = bytes[1] & 0x0F;
    if high > 2 || tens > 9 || ones > 9 {
        return None;
    }
    let value = high * 100 + tens * 10 + ones;
    Some(value)
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::icom::ic7300::decimal_to_bcd;

    #[test]
    fn parses_live_response_frame_from_radio() {
        let frame = [
            0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x80, 0x18, 0x07, 0x00, 0xFD,
        ];
        assert_eq!(parse_frequency(&frame), Some(7_188_000));
    }

    #[test]
    fn maps_mode_to_from_civ() {
        assert_eq!(mode_to_civ_mode(Mode::Lsb).ok(), Some(0x00));
        assert_eq!(mode_to_civ_mode(Mode::Usb).ok(), Some(0x01));
        assert_eq!(mode_to_civ_mode(Mode::Cw).ok(), Some(0x03));
        assert_eq!(civ_mode_to_mode(0x00), Some(Mode::Lsb));
        assert_eq!(civ_mode_to_mode(0x01), Some(Mode::Usb));
        assert_eq!(civ_mode_to_mode(0x03), Some(Mode::Cw));
        assert_eq!(civ_mode_to_mode(0x05), Some(Mode::Fm));
    }

    #[test]
    fn plain_usb_does_not_enable_data_mode() {
        assert_eq!(
            hal_mode_to_icom_operating_mode(Mode::Usb),
            (BaseMode::Usb, false)
        );
        assert_eq!(
            hal_mode_to_icom_operating_mode(Mode::Data),
            (BaseMode::Usb, true)
        );
    }

    #[test]
    fn parses_ptt_and_command_only_control_responses() {
        let ptt = [0xFE, 0xFE, 0xE0, 0x94, 0x1C, 0x00, 0x01, 0xFD];
        assert_eq!(
            response_data_after_prefix(&ptt, &[0x1C, 0x00]).unwrap(),
            &[1]
        );

        let split = [0xFE, 0xFE, 0xE0, 0x94, 0x0F, 0x01, 0xFD];
        assert_eq!(response_data_after_prefix(&split, &[0x0F]).unwrap(), &[1]);
        let attenuator = [0xFE, 0xFE, 0xE0, 0x94, 0x11, 0x20, 0xFD];
        assert_eq!(
            response_data_after_prefix(&attenuator, &[0x11]).unwrap(),
            &[0x20]
        );
    }

    #[test]
    fn parses_mode_details_with_data_and_filter() {
        let frame = [0xFE, 0xFE, 0xE0, 0x94, 0x04, 0x01, 0x01, 0x02, 0xFD];
        let details = parse_mode_details(&frame).expect("mode details expected");
        assert_eq!(details.base, BaseMode::Usb);
        assert!(details.data_mode);
        assert_eq!(details.filter, Some(2));
        assert_eq!(details.label(), "USB-D");
    }

    #[test]
    fn parses_extended_mode_response_after_subcommand() {
        let frame = [0xFE, 0xFE, 0xE0, 0x94, 0x26, 0x00, 0x01, 0x01, 0x03, 0xFD];
        let details = parse_mode_details(&frame).expect("extended mode details expected");
        assert_eq!(details.base, BaseMode::Usb);
        assert!(details.data_mode);
        assert_eq!(details.filter, Some(3));
        assert_eq!(details.label(), "USB-D");
    }

    #[test]
    fn uses_configured_port_when_present() {
        let radio = IcomCiVRadio::new_generic("/dev/ttyUSB0", 115_200, 0xE0, 0x94);
        assert_eq!(radio.port, "/dev/ttyUSB0");
        assert_eq!(radio.model(), None);
        assert_eq!(
            encode_civ_frequency_bcd(14_074_000),
            [0x00, 0x40, 0x07, 0x14, 0x00]
        );
    }

    #[test]
    fn falls_back_to_enumerated_ports_when_config_is_missing() {
        let radio = IcomCiVRadio::new_generic("", 115_200, 0xE0, 0x94);
        assert_eq!(radio.port, "");
    }

    #[test]
    fn generic_driver_rejects_profile_controls() {
        let radio = IcomCiVRadio::new_generic("", 115_200, 0xE0, 0x94);
        let error = futures::executor::block_on(
            radio.set_control(ControlId::RfPower, ControlValue::U8(50)),
        )
        .expect_err("generic CI-V must reject model controls");
        assert!(error.to_string().contains("selected Icom model profile"));
    }

    #[test]
    fn ic7300_scope_controls_reject_other_models_before_transport() {
        let radio = IcomCiVRadio::new_for_model(
            crate::models::IcomCivModel::Ic7610,
            "",
            115_200,
            0xE0,
            0x98,
        );
        let error = futures::executor::block_on(radio.set_scope_hold(true))
            .expect_err("IC-7300 scope controls must reject other profiles");
        assert!(error.to_string().contains("IC-7300"));
    }

    #[test]
    fn bcd_level_roundtrip() {
        let encoded = encode_level_255_bcd(173);
        assert_eq!(encoded, vec![0x01, 0x73]);
        assert_eq!(decode_level_255_bcd(&encoded), Some(173));
        assert_eq!(decode_level_255_bcd(&[0xF1, 0x73]), None);
        assert_eq!(decode_level_255_bcd(&[0x01, 0xFA]), None);
    }

    #[test]
    fn swr_meter_uses_documented_decimal_scaling() {
        assert_eq!(decode_level_255_bcd(&[0x00, 0x00]), Some(0));
        assert_eq!(decode_level_255_bcd(&[0x00, 0x48]), Some(48));
        assert_eq!(decode_level_255_bcd(&[0x01, 0x20]), Some(120));
    }

    #[test]
    fn parses_captured_ft8_frames_fixture() {
        let fixture = include_str!("../../tests/fixtures/ic7300_ft8_status_frames.txt");
        let mut lines = fixture
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'));

        let freq_line = lines.next().expect("freq frame in fixture");
        let mode_line = lines.next().expect("mode frame in fixture");

        let freq =
            parse_frequency(&parse_hex_line(freq_line)).expect("frequency decode from fixture");
        let mode =
            parse_mode_details(&parse_hex_line(mode_line)).expect("mode decode from fixture");

        assert_eq!(freq, 14_074_000);
        assert_eq!(mode.label(), "USB-D");
        assert_eq!(mode.filter, Some(1));
    }

    #[test]
    fn detects_spectrum_data_frame_shape() {
        let legacy = [
            0xFE, 0xFE, 0xE0, 0x94, 0x27, 0x12, 0x01, 0x10, 0x00, 0xAB, 0xFD,
        ];
        let scope_wave = [0xFE, 0xFE, 0xE0, 0x94, 0x27, 0x00, 0x01, 0x11, 0xFD];
        assert!(is_spectrum_data_frame(&legacy));
        assert!(is_spectrum_data_frame(&scope_wave));
    }

    #[test]
    fn extracts_multiple_frames_from_buffer() {
        let ack = [0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD];
        let spec = [
            0xFE, 0xFE, 0xE0, 0x94, 0x27, 0x12, 0x01, 0x10, 0x00, 0xAB, 0xFD,
        ];
        let mut buf = Vec::new();
        buf.extend_from_slice(&ack);
        buf.extend_from_slice(&spec);

        let frames = extract_ci_v_frames(&buf);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], ack);
        assert_eq!(frames[1], spec);
        assert!(is_spectrum_data_frame(&frames[1]));
    }

    #[test]
    fn streaming_frame_drain_preserves_a_split_preamble() {
        let mut pending = vec![0xAA, 0xFE];
        assert!(drain_ci_v_frames(&mut pending).is_empty());
        assert_eq!(pending, vec![0xFE]);
        pending.extend_from_slice(&[0xFE, 0xE0, 0x94, 0xFB, 0xFD]);
        let frames = drain_ci_v_frames(&mut pending);
        assert_eq!(frames, vec![vec![0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]]);
        assert!(pending.is_empty());
    }

    #[test]
    fn detects_nak_frame() {
        let nak = [0xFE, 0xFE, 0xE0, 0x94, 0xFA, 0xFD];
        assert!(is_nak_frame(&nak));
    }

    #[test]
    fn detects_ack_frame() {
        let ack = [0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD];
        assert!(is_ack_frame(&ack));
    }

    #[test]
    fn parses_real_ic7300_scope_chunk_layout() {
        let frame = [
            0xFE, 0xFE, 0xE0, 0x94, 0x27, 0x00, 0x00, 0x07, 0x11, 0x27, 0x13, 0x15, 0x01, 0x00,
            0x22, 0x21, 0x09, 0x08, 0x06, 0x19, 0x0e, 0x20, 0x23, 0x25, 0x2c, 0x2d, 0x17, 0x27,
            0x29, 0x16, 0x14, 0x1b, 0x1b, 0x21, 0x27, 0x1a, 0x18, 0x17, 0x1e, 0x21, 0x1b, 0x24,
            0x21, 0x22, 0x23, 0x13, 0x19, 0x23, 0x2f, 0x2d, 0x25, 0x25, 0x0a, 0x0e, 0x1e, 0x20,
            0x1f, 0x1a, 0x0c, 0xFD,
        ];
        let (division, maximum, bins) = parse_scope_waveform_segment(
            &frame,
            Some(crate::models::IcomScopeGeometry {
                divisions: 11,
                bins: 475,
                full_chunk_bins: 50,
                last_chunk_bins: 25,
                bin_max: 160,
                supports_main_sub_scope: false,
            }),
        )
        .unwrap();
        assert_eq!((division, maximum, bins.len()), (7, 11, 50));
        assert_eq!(&bins[..4], &[0x27, 0x13, 0x15, 0x01]);
    }

    #[test]
    fn parses_ic7610_scope_header_with_main_sub_metadata() {
        let mut frame = vec![0xFE, 0xFE, 0xE0, 0x98, 0x27, 0x00, 0x00, 0x01, 0x0F, 0x01];
        frame.extend(std::iter::repeat_n(42, 50));
        frame.push(0xFD);
        let geometry = crate::models::IcomScopeGeometry {
            divisions: 15,
            bins: 689,
            full_chunk_bins: 50,
            last_chunk_bins: 39,
            bin_max: 200,
            supports_main_sub_scope: true,
        };
        let parsed = parse_scope_waveform_segment(&frame, Some(geometry)).unwrap();
        // The manual specifies that division 1 carries only waveform
        // metadata; waveform samples begin with division 2.
        assert_eq!((parsed.0, parsed.1, parsed.2.len()), (1, 15, 0));
    }

    #[test]
    fn parses_ic9700_scope_header_without_skipping_division_bytes() {
        let mut frame = vec![0xFE, 0xFE, 0xE0, 0xA2, 0x27, 0x00, 0x01, 0x02, 0x11];
        frame.extend(std::iter::repeat_n(42, 50));
        frame.push(0xFD);
        let geometry = profile_for_model(crate::models::IcomCivModel::Ic9700)
            .scope_geometry
            .unwrap();
        let parsed = parse_scope_waveform_segment(&frame, Some(geometry)).unwrap();
        assert_eq!((parsed.0, parsed.1, parsed.2.len()), (2, 11, 50));
    }

    #[test]
    fn assembles_only_one_complete_ordered_ic7300_sweep() {
        fn frame(division: usize) -> Vec<u8> {
            let mut frame = vec![
                0xFE,
                0xFE,
                0xE0,
                0x94,
                0x27,
                0x00,
                0x00,
                decimal_to_bcd(division as u8),
                0x11,
            ];
            if division == 1 {
                frame.extend_from_slice(&[0x00; 12]);
            } else {
                let count = if division == 11 { 25 } else { 50 };
                frame.extend(std::iter::repeat_n(division as u8, count));
            }
            frame.push(0xFD);
            frame
        }

        let mut assembler = ScopeSweepAssembler::default();
        for division in 1..11 {
            assert!(assembler
                .push(
                    &frame(division),
                    Some(crate::models::IcomScopeGeometry {
                        divisions: 11,
                        bins: 475,
                        full_chunk_bins: 50,
                        last_chunk_bins: 25,
                        bin_max: 160,
                        supports_main_sub_scope: false,
                    })
                )
                .is_none());
        }
        let sweep = assembler
            .push(
                &frame(11),
                Some(crate::models::IcomScopeGeometry {
                    divisions: 11,
                    bins: 475,
                    full_chunk_bins: 50,
                    last_chunk_bins: 25,
                    bin_max: 160,
                    supports_main_sub_scope: false,
                }),
            )
            .expect("complete sweep");
        assert_eq!(sweep.len(), 475);
        assert_eq!(&sweep[..50], &[2; 50]);
        assert_eq!(&sweep[450..], &[11; 25]);

        let mut assembler = ScopeSweepAssembler::default();
        for division in [1, 2, 3, 5, 6, 7, 8, 9, 10, 11] {
            assert!(assembler
                .push(
                    &frame(division),
                    Some(crate::models::IcomScopeGeometry {
                        divisions: 11,
                        bins: 475,
                        full_chunk_bins: 50,
                        last_chunk_bins: 25,
                        bin_max: 160,
                        supports_main_sub_scope: false,
                    })
                )
                .is_none());
        }
        for division in 1..11 {
            assert!(assembler
                .push(
                    &frame(division),
                    Some(crate::models::IcomScopeGeometry {
                        divisions: 11,
                        bins: 475,
                        full_chunk_bins: 50,
                        last_chunk_bins: 25,
                        bin_max: 160,
                        supports_main_sub_scope: false,
                    })
                )
                .is_none());
        }
        assert_eq!(
            assembler
                .push(
                    &frame(11),
                    Some(crate::models::IcomScopeGeometry {
                        divisions: 11,
                        bins: 475,
                        full_chunk_bins: 50,
                        last_chunk_bins: 25,
                        bin_max: 160,
                        supports_main_sub_scope: false,
                    })
                )
                .unwrap()
                .len(),
            475
        );
    }

    #[test]
    fn stream_reader_preserves_every_complete_sweep_in_one_serial_read() {
        fn frame(division: usize, level: u8) -> Vec<u8> {
            let mut frame = vec![
                0xFE,
                0xFE,
                0xE0,
                0x94,
                0x27,
                0x00,
                0x00,
                decimal_to_bcd(division as u8),
                0x11,
            ];
            if division == 1 {
                frame.extend_from_slice(&[0x00; 12]);
            } else {
                let count = if division == 11 { 25 } else { 50 };
                frame.extend(std::iter::repeat_n(level, count));
            }
            frame.push(0xFD);
            frame
        }

        let mut bytes = Vec::new();
        for level in [20, 80] {
            for division in 1..=11 {
                bytes.extend(frame(division, level));
            }
        }
        let sweeps = ScopeStreamReader::default().push_bytes(
            &bytes,
            0x94,
            0xE0,
            Some(crate::models::IcomScopeGeometry {
                divisions: 11,
                bins: 475,
                full_chunk_bins: 50,
                last_chunk_bins: 25,
                bin_max: 160,
                supports_main_sub_scope: false,
            }),
        );
        assert_eq!(sweeps.len(), 2);
        assert!(sweeps[0].iter().all(|value| *value == 20));
        assert!(sweeps[1].iter().all(|value| *value == 80));
    }

    #[test]
    fn command_reader_can_queue_a_complete_unsolicited_sweep_for_stream_drain() {
        let mut bytes = Vec::new();
        for division in 1..=11 {
            bytes.extend([
                0xFE,
                0xFE,
                0xE0,
                0x94,
                0x27,
                0x00,
                0x00,
                decimal_to_bcd(division),
                0x11,
            ]);
            if division == 1 {
                bytes.extend([0x00; 12]);
            } else {
                let count = if division == 11 { 25 } else { 50 };
                bytes.extend(std::iter::repeat_n(42, count));
            }
            bytes.push(0xFD);
        }

        let mut reader = ScopeStreamReader::default();
        reader.ingest_bytes(
            &bytes,
            0x94,
            0xE0,
            Some(crate::models::IcomScopeGeometry {
                divisions: 11,
                bins: 475,
                full_chunk_bins: 50,
                last_chunk_bins: 25,
                bin_max: 160,
                supports_main_sub_scope: false,
            }),
        );
        let queued = reader.push_bytes(
            &[],
            0x94,
            0xE0,
            Some(crate::models::IcomScopeGeometry {
                divisions: 11,
                bins: 475,
                full_chunk_bins: 50,
                last_chunk_bins: 25,
                bin_max: 160,
                supports_main_sub_scope: false,
            }),
        );
        assert_eq!(queued.len(), 1);
        assert!(queued[0].iter().all(|value| *value == 42));
    }

    #[test]
    fn detects_ic7300_from_usb_identity() {
        assert_eq!(
            detect_likely_radio_model(0x0C26, 0x0000, "Icom Inc.", "IC-7300"),
            Some("Icom IC-7300 (CI-V)".to_string())
        );
    }

    #[test]
    fn detects_generic_icom_when_model_is_unknown() {
        assert_eq!(
            detect_likely_radio_model(0x0C26, 0x0001, "Icom Inc.", "USB Audio CODEC"),
            Some("Icom CI-V radio".to_string())
        );
    }

    fn parse_hex_line(line: &str) -> Vec<u8> {
        line.split_whitespace()
            .map(|t| u8::from_str_radix(t, 16).expect("valid hex token"))
            .collect()
    }
}

fn parse_mode(response: &[u8]) -> Option<Mode> {
    let details = parse_mode_details(response)?;
    let base = match details.base {
        BaseMode::Lsb => Mode::Lsb,
        BaseMode::Usb => Mode::Usb,
        BaseMode::Am => Mode::Am,
        BaseMode::Cw => Mode::Cw,
        BaseMode::CwR => Mode::CwReverse,
        BaseMode::Rtty => Mode::Rtty,
        BaseMode::RttyR => Mode::RttyReverse,
        BaseMode::Fm => Mode::Fm,
        BaseMode::Wfm => Mode::Wfm,
        BaseMode::Unknown(_) => return None,
    };
    if details.data_mode {
        Some(Mode::Data)
    } else {
        Some(base)
    }
}

fn parse_mode_details(response: &[u8]) -> Option<OperatingMode> {
    let payload = response
        .windows(2)
        .position(|w| w[0] == CI_V_FRAME_START && w[1] == CI_V_FRAME_START)
        .map(|idx| &response[idx..])?;

    let end = payload.iter().position(|&b| b == CI_V_FRAME_END)?;
    let frame = &payload[..=end];
    if frame.len() <= 6 {
        return None;
    }
    if frame[4] != 0x04 && frame[4] != 0x26 {
        return None;
    }

    let data = if frame[4] == 0x26 {
        let data = &frame[5..frame.len() - 1];
        if data.first().copied() != Some(0x00) {
            return None;
        }
        &data[1..]
    } else {
        &frame[5..frame.len() - 1]
    };
    let mode = *data.first()?;
    let data_on = data.get(1).copied().unwrap_or(0) != 0;
    let filter = data.get(2).copied().filter(|v| (1..=3).contains(v));
    Some(OperatingMode {
        base: civ_mode_to_base_mode(mode),
        data_mode: data_on,
        filter,
    })
}

fn frame_matches_request(frame: &[u8], payload: &[u8]) -> bool {
    if payload.is_empty() || frame.len() < 6 {
        return false;
    }
    if frame.first().copied() != Some(CI_V_FRAME_START)
        || frame.get(1).copied() != Some(CI_V_FRAME_START)
        || frame.last().copied() != Some(CI_V_FRAME_END)
    {
        return false;
    }

    let expected_cmd = payload[0];
    let response_cmd = frame[4];
    if response_cmd != expected_cmd {
        // Some Icom rigs may answer mode-detail-style queries with 0x04 payloads.
        if expected_cmd == 0x26 && response_cmd == 0x04 {
            return true;
        }
        return false;
    }

    // For command families with subcommand addressing, match subcommand too.
    if payload.len() >= 2 {
        let cmd_has_sub = matches!(
            expected_cmd,
            0x11 | 0x14 | 0x15 | 0x16 | 0x1A | 0x1C | 0x1F | 0x26 | 0x27
        );
        if cmd_has_sub {
            return frame.get(5).copied() == Some(payload[1]);
        }
    }

    true
}

fn is_radio_to_controller_frame(frame: &[u8], radio_address: u8, controller_address: u8) -> bool {
    frame.len() >= 6
        && frame.first().copied() == Some(CI_V_FRAME_START)
        && frame.get(1).copied() == Some(CI_V_FRAME_START)
        && frame.last().copied() == Some(CI_V_FRAME_END)
        && frame.get(2).copied() == Some(controller_address)
        && frame.get(3).copied() == Some(radio_address)
}
