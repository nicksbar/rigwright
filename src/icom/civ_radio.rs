use crate::hal::{Radio, RadioCapabilities, RadioStatus};
use crate::transport::{RadioTransport, SerialPortTransport};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serialport::SerialPortType;
use std::{
    collections::VecDeque,
    io::ErrorKind,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
    time::Instant,
};

use super::profile::ControlEncoding;
use super::profile::{
    meter_command_prefix, model_from_usb_identity, profile_for_model, MemoryLayout, ScopeMenuSpec,
};
use crate::events::{RadioEvent, RadioEventRouter, RadioEventSubscription};
pub use crate::hal_types::{BaseMode, Mode, OperatingMode};
use crate::hal_types::{
    ControlId, ControlValue, MemoryChannel, MeterId, MeterPollSpec, MeterPresentation,
    RepeaterSettings, RepeaterShift, ScopeCenterType, ScopeColor, ScopeConfiguration,
    ScopeMarkerPosition, ScopeMaxHold, ScopeMetadata, ScopeState, ScopeWaveformType, SwrSweepSetup,
    ToneMode, ToneSettings, TunerStatus,
};

const CI_V_FRAME_START: u8 = 0xFE;
const CI_V_FRAME_END: u8 = 0xFD;
const MAX_RETAINED_CI_V_FRAMES: usize = 128;

/// Connection-level counters useful for diagnosing real CI-V links.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IcomTransportMetrics {
    pub commands_started: u64,
    pub responses_matched: u64,
    pub response_timeouts: u64,
    pub bytes_read: u64,
    pub frames_received: u64,
    pub frames_retained: u64,
    pub frames_dropped: u64,
    pub echo_frames_ignored: u64,
    pub unsolicited_events: u64,
    pub total_response_time: Duration,
    pub consecutive_timeouts: u32,
}

/// Host-side serial behavior for a native Icom CI-V connection.
///
/// DTR and RTS are left untouched by default because some older interfaces
/// derive power or PTT behavior from modem-control lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcomSerialPolicy {
    pub hardware_flow_control: bool,
    pub dtr: Option<bool>,
    pub rts: Option<bool>,
    pub startup_settle: Duration,
}

/// Successful result from an explicit CI-V connection probe.
#[derive(Debug, Clone, Default)]
pub struct IcomProbeResult {
    pub baud_rate: u32,
    pub radio_address: u8,
    pub status: RadioStatus,
}

impl Default for IcomSerialPolicy {
    fn default() -> Self {
        Self {
            hardware_flow_control: false,
            dtr: None,
            rts: None,
            startup_settle: Duration::from_millis(50),
        }
    }
}

#[derive(Debug, Default)]
struct IcomTransportState {
    retained_frames: VecDeque<Vec<u8>>,
    metrics: IcomTransportMetrics,
    /// The last time a typed unsolicited CI-V event (frequency, mode, PTT,
    /// meter) arrived. Used to trust the live event stream instead of
    /// polling, and to drive scope/link keep-alive heuristics.
    last_event_at: Option<Instant>,
}

/// Compatibility name for the shared byte transport used by CI-V.
pub use crate::transport::RadioTransport as CiVTransport;

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
    /// When the last complete scope sweep was assembled. Drives the scope
    /// keep-alive health signal so the UI can recover a stalled waterfall.
    last_sweep_at: Option<Instant>,
}

/// A point-in-time snapshot of CI-V scope/waterfall stream health.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeStreamHealth {
    /// Total scope division frames ingested.
    pub division_frames: u64,
    /// Total complete sweeps assembled.
    pub completed_sweeps: u64,
    /// Sweeps dropped because a frame broke framing or geometry.
    pub dropped_sweeps: u64,
    /// How long ago the last complete sweep arrived; `None` if the scope has
    /// never produced a sweep on this connection.
    pub last_sweep_age: Option<Duration>,
}

impl ScopeStreamHealth {
    /// True when the scope has produced sweeps before but none recently,
    /// indicating a stalled waterfall the UI may want to recover.
    pub fn is_stalled(&self, max_age: Duration) -> bool {
        self.completed_sweeps > 0 && self.last_sweep_age.is_some_and(|age| age > max_age)
    }
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
                self.last_sweep_at = Some(Instant::now());
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

    if let Some(model) = model_from_usb_identity(vid, manufacturer, product) {
        return Some(format!("Icom {} (CI-V)", model.model_name()));
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
    serial_port: Arc<Mutex<Option<Box<dyn RadioTransport>>>>,
    external_transport: Arc<Mutex<Option<Box<dyn RadioTransport>>>>,
    scope_stream_reader: Arc<Mutex<ScopeStreamReader>>,
    transport_state: Arc<Mutex<IcomTransportState>>,
    serial_policy: IcomSerialPolicy,
    event_router: RadioEventRouter,
}

#[derive(Debug, Clone, Copy)]
enum ControlOp {
    Set,
    Get,
}

impl IcomCiVRadio {
    /// Subscribe to unsolicited CI-V state changes. Events are routed while
    /// this radio's transport is being read, including reads performed to
    /// satisfy another command, so subscribers do not need a second worker.
    pub fn subscribe_events(&self) -> RadioEventSubscription {
        self.event_router.subscribe()
    }

    /// Apply the common scope controls supported by the selected Icom model.
    /// Payload validation stays here at the protocol boundary; model profiles
    /// still own geometry and whether native scope output is advertised.
    pub async fn set_scope_configuration(&self, config: ScopeConfiguration) -> Result<()> {
        anyhow::ensure!(
            self.supports_scope(),
            "native CI-V scope is unavailable for this model"
        );
        let profile = self.model().map(profile_for_model);
        let menu = profile.and_then(|profile| profile.scope.and_then(|scope| scope.menu));
        let set_menu = |index: u16, value: u8| -> Result<()> {
            let [high, low] = encode_civ_menu_index(index);
            self.transact_ack(&[0x1A, 0x05, high, low, value])
        };
        let require_menu = |menu: Option<ScopeMenuSpec>| -> Result<ScopeMenuSpec> {
            menu.ok_or_else(|| anyhow!("advanced scope controls are unavailable for this model"))
        };
        if let Some(value) = config.center_mode {
            self.transact_ack(&[0x27, 0x14, 0x00, u8::from(value)])?;
        }
        if let Some(span) = config.span_hz {
            anyhow::ensure!(span > 0, "scope span must be positive");
            let mut payload = vec![0x27, 0x15, 0x00];
            payload.extend_from_slice(&encode_civ_frequency_bcd(span));
            self.transact_ack(&payload)?;
        }
        if let Some(edge) = config.fixed_edge_number {
            let metadata = self
                .scope_metadata()
                .ok_or_else(|| anyhow!("scope metadata is unavailable"))?;
            anyhow::ensure!(
                metadata.fixed_edge_numbers.contains(&edge),
                "unsupported scope fixed-edge number {edge}"
            );
            self.transact_ack(&[0x27, 0x16, 0x00, edge])?;
        }
        if let Some(hold) = config.hold {
            self.transact_ack(&[0x27, 0x17, 0x00, u8::from(hold)])?;
        }
        if let Some(level) = config.reference_level_tenths_db {
            anyhow::ensure!(
                (-200..=200).contains(&level) && level % 5 == 0,
                "scope reference level must be -20.0..=20.0 dB in 0.5 dB steps"
            );
            let magnitude = level.unsigned_abs();
            self.transact_ack(&[
                0x27,
                0x19,
                0x00,
                decimal_to_bcd((magnitude / 10) as u8),
                if magnitude % 10 == 5 { 0x50 } else { 0 },
                u8::from(level < 0),
            ])?;
        }
        if let Some(speed) = config.sweep_speed {
            let metadata = self
                .scope_metadata()
                .ok_or_else(|| anyhow!("scope metadata is unavailable"))?;
            anyhow::ensure!(
                metadata.sweep_speed_values.contains(&speed),
                "unsupported scope sweep speed {speed}"
            );
            self.transact_ack(&[0x27, 0x1A, 0x00, speed])?;
        }
        if let Some(wide) = config.vbw_wide {
            self.transact_ack(&[0x27, 0x1D, 0x00, u8::from(wide)])?;
        }
        if let Some((lower, upper)) = config.fixed_edges_hz {
            anyhow::ensure!(lower < upper, "scope lower edge must be below upper edge");
            let edge = config.fixed_edge_number.unwrap_or(1);
            let metadata = self
                .scope_metadata()
                .ok_or_else(|| anyhow!("scope metadata is unavailable"))?;
            anyhow::ensure!(
                metadata.fixed_edge_numbers.contains(&edge),
                "unsupported scope fixed-edge number {edge}"
            );
            let mut payload = vec![0x27, 0x1E, 0x00, edge];
            payload.extend_from_slice(&encode_civ_frequency_bcd(lower));
            payload.extend_from_slice(&encode_civ_frequency_bcd(upper));
            self.transact_ack(&payload)?;
        }
        if let Some(value) = config.tx_display {
            let menu = require_menu(menu)?;
            set_menu(menu.tx_display, u8::from(value))?;
        }
        if let Some(value) = config.max_hold {
            let menu = require_menu(menu)?;
            set_menu(menu.max_hold, scope_max_hold_value(value))?;
        }
        if let Some(value) = config.center_type {
            let menu = require_menu(menu)?;
            set_menu(menu.center_type, scope_center_type_value(value))?;
        }
        if let Some(value) = config.marker_position {
            let menu = require_menu(menu)?;
            set_menu(menu.marker_position, scope_marker_position_value(value))?;
        }
        if let Some(value) = config.averaging {
            let menu = require_menu(menu)?;
            let encoded = match value {
                0 => 0,
                2..=4 => value - 1,
                _ => anyhow::bail!("unsupported scope averaging value {value}"),
            };
            set_menu(menu.averaging, encoded)?;
        }
        if let Some(value) = config.waveform_type {
            let menu = require_menu(menu)?;
            set_menu(menu.waveform_type, scope_waveform_type_value(value))?;
        }
        if let Some(value) = config.waterfall_display {
            let menu = require_menu(menu)?;
            set_menu(menu.waterfall_display, u8::from(value))?;
        }
        if let Some(value) = config.waterfall_size {
            let menu = require_menu(menu)?;
            anyhow::ensure!(value <= 2, "unsupported waterfall size {value}");
            set_menu(menu.waterfall_size, value)?;
        }
        if let Some(value) = config.waterfall_peak_level {
            let menu = require_menu(menu)?;
            let metadata = self
                .scope_metadata()
                .ok_or_else(|| anyhow!("scope metadata is unavailable"))?;
            anyhow::ensure!(
                metadata.waterfall_peak_level_options.contains(&value),
                "unsupported waterfall peak level {value}"
            );
            set_menu(menu.waterfall_peak_level, value.saturating_sub(1))?;
        }
        if let Some(value) = config.marker_auto_hide {
            let menu = require_menu(menu)?;
            set_menu(menu.marker_auto_hide, u8::from(value))?;
        }
        if let Some(color) = config.waveform_color_current {
            let menu = require_menu(menu)?;
            self.set_scope_color(menu.waveform_color_current, color)?;
        }
        if let Some(color) = config.waveform_color_line {
            let menu = require_menu(menu)?;
            self.set_scope_color(menu.waveform_color_line, color)?;
        }
        if let Some(color) = config.waveform_color_max_hold {
            let menu = require_menu(menu)?;
            self.set_scope_color(menu.waveform_color_max_hold, color)?;
        }
        Ok(())
    }

    pub async fn get_scope_state(&self) -> Result<ScopeState> {
        anyhow::ensure!(
            self.supports_scope(),
            "native CI-V scope is unavailable for this model"
        );
        let profile = profile_for_model(self.selected_model()?);
        let menu = profile
            .scope
            .and_then(|scope| scope.menu)
            .context("scope menu is unavailable")?;
        let tx_display = self.read_scope_menu(menu.tx_display, 1)?[0] != 0;
        let waterfall_display = self.read_scope_menu(menu.waterfall_display, 1)?[0] != 0;
        let marker_auto_hide = self.read_scope_menu(menu.marker_auto_hide, 1)?[0] != 0;
        let max_hold = match self.read_scope_menu(menu.max_hold, 1)?[0] {
            0 => ScopeMaxHold::Off,
            1 => ScopeMaxHold::TenSeconds,
            2 => ScopeMaxHold::Continuous,
            value => anyhow::bail!("invalid CI-V max hold value {value}"),
        };
        let center_type = match self.read_scope_menu(menu.center_type, 1)?[0] {
            0 => ScopeCenterType::FilterCenter,
            1 => ScopeCenterType::CarrierPoint,
            2 => ScopeCenterType::CarrierPointAbsolute,
            value => anyhow::bail!("invalid CI-V center type value {value}"),
        };
        let marker_position = match self.read_scope_menu(menu.marker_position, 1)?[0] {
            0 => ScopeMarkerPosition::FilterCenter,
            1 => ScopeMarkerPosition::CarrierPoint,
            value => anyhow::bail!("invalid CI-V marker position value {value}"),
        };
        let averaging = match self.read_scope_menu(menu.averaging, 1)?[0] {
            0 => 0,
            1..=3 => self.read_scope_menu(menu.averaging, 1)?[0] + 1,
            value => anyhow::bail!("invalid CI-V averaging value {value}"),
        };
        let waveform_type = match self.read_scope_menu(menu.waveform_type, 1)?[0] {
            0 => ScopeWaveformType::Fill,
            1 => ScopeWaveformType::FillAndLine,
            value => anyhow::bail!("invalid CI-V waveform type value {value}"),
        };
        let waterfall_peak_level = self.read_scope_menu(menu.waterfall_peak_level, 1)?[0] + 1;
        let state = ScopeConfiguration {
            tx_display: Some(tx_display),
            max_hold: Some(max_hold),
            center_type: Some(center_type),
            marker_position: Some(marker_position),
            averaging: Some(averaging),
            waveform_type: Some(waveform_type),
            waterfall_display: Some(waterfall_display),
            waterfall_size: Some(self.read_scope_menu(menu.waterfall_size, 1)?[0]),
            waterfall_peak_level: Some(waterfall_peak_level),
            marker_auto_hide: Some(marker_auto_hide),
            ..ScopeConfiguration::default()
        };
        Ok(ScopeState {
            configuration: state,
            waveform_color_current: Some(self.read_scope_color(menu.waveform_color_current)?),
            waveform_color_line: Some(self.read_scope_color(menu.waveform_color_line)?),
            waveform_color_max_hold: Some(self.read_scope_color(menu.waveform_color_max_hold)?),
        })
    }

    fn read_scope_menu(&self, index: u16, length: usize) -> Result<Vec<u8>> {
        let [high, low] = encode_civ_menu_index(index);
        let prefix = [0x1A, 0x05, high, low];
        let response = self.transact(&prefix, true)?;
        let data = response_data_after_prefix(&response, &prefix)?;
        anyhow::ensure!(data.len() >= length, "short CI-V scope menu response");
        Ok(data[..length].to_vec())
    }

    fn read_scope_color(&self, index: u16) -> Result<ScopeColor> {
        let data = self.read_scope_menu(index, 6)?;
        decode_scope_color(&data)
    }

    fn set_scope_color(&self, index: u16, color: ScopeColor) -> Result<()> {
        let [high, low] = encode_civ_menu_index(index);
        let mut payload = vec![0x1A, 0x05, high, low];
        payload.extend_from_slice(&encode_scope_color(color));
        self.transact_ack(&payload)
    }

    fn set_operating_mode_blocking(
        &self,
        base_mode: BaseMode,
        data_mode: bool,
        filter: u8,
    ) -> Result<()> {
        let profile = self.active_profile();
        anyhow::ensure!(
            profile.supports_mode(base_mode),
            "mode {base_mode:?} is not documented for {}",
            profile.model.model_name()
        );
        let mode_byte = base_mode_to_civ_mode(base_mode)
            .with_context(|| format!("unsupported base mode for CI-V set: {base_mode:?}"))?;
        anyhow::ensure!(
            profile.control_capabilities.filter_values.contains(&filter)
                || (self.model.is_none() && (1..=3).contains(&filter)),
            "CI-V filter value is not documented for {}",
            profile.model.model_name()
        );
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

    /// Create a model-backed CI-V driver over an externally owned byte
    /// transport, such as an Android USB Host or Bluetooth connection.
    pub fn with_transport<T>(
        model: Option<crate::models::IcomCivModel>,
        controller_address: u8,
        radio_address: u8,
        transport: T,
    ) -> Self
    where
        T: RadioTransport + 'static,
    {
        Self {
            model,
            port: String::new(),
            baud_rate: 0,
            controller_address,
            radio_address,
            serial_port: Arc::new(Mutex::new(None)),
            external_transport: Arc::new(Mutex::new(Some(Box::new(transport)))),
            scope_stream_reader: Arc::new(Mutex::new(ScopeStreamReader::default())),
            transport_state: Arc::new(Mutex::new(IcomTransportState::default())),
            serial_policy: IcomSerialPolicy::default(),
            event_router: RadioEventRouter::default(),
        }
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
            external_transport: Arc::new(Mutex::new(None)),
            scope_stream_reader: Arc::new(Mutex::new(ScopeStreamReader::default())),
            transport_state: Arc::new(Mutex::new(IcomTransportState::default())),
            serial_policy: IcomSerialPolicy::default(),
            event_router: RadioEventRouter::default(),
        }
    }

    pub fn with_radio_address(mut self, radio_address: u8) -> Self {
        self.radio_address = radio_address;
        self
    }

    /// Set host-side serial signaling behavior before the port is opened.
    pub fn with_serial_policy(mut self, policy: IcomSerialPolicy) -> Self {
        self.serial_policy = policy;
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

    /// Return a point-in-time snapshot of link health counters.
    pub fn transport_metrics(&self) -> IcomTransportMetrics {
        self.transport_state
            .lock()
            .map(|state| state.metrics)
            .unwrap_or_default()
    }

    /// Record that a typed CI-V event just arrived from the radio.
    fn note_event_received(&self) {
        if let Ok(mut state) = self.transport_state.lock() {
            state.last_event_at = Some(Instant::now());
        }
    }

    /// How long ago the radio last pushed a typed CI-V event (frequency,
    /// mode, PTT, or meter). `None` means no unsolicited event has been seen
    /// on this connection. A fresh value indicates the radio's event stream
    /// is live, so observed state can be trusted without polling.
    pub fn event_stream_age(&self) -> Option<Duration> {
        self.transport_state
            .lock()
            .ok()
            .and_then(|state| state.last_event_at)
            .map(|instant| instant.elapsed())
    }

    /// True when the radio's unsolicited event stream is live (an event was
    /// seen within `freshness`), so core state can come from events rather
    /// than fresh polls.
    pub fn event_stream_is_live(&self, freshness: Duration) -> bool {
        self.event_stream_age().is_some_and(|age| age <= freshness)
    }

    /// A point-in-time snapshot of scope/waterfall stream health. Use
    /// `ScopeStreamHealth::is_stalled` to detect a waterfall that was flowing
    /// but has stopped, so the UI can re-arm the scope stream.
    pub fn scope_stream_health(&self) -> ScopeStreamHealth {
        self.scope_stream_reader
            .lock()
            .map(|reader| ScopeStreamHealth {
                division_frames: reader.division_frames,
                completed_sweeps: reader.completed_sweeps,
                dropped_sweeps: reader.assembler.dropped_sweeps,
                last_sweep_age: reader.last_sweep_at.map(|instant| instant.elapsed()),
            })
            .unwrap_or_default()
    }

    fn selected_model(&self) -> Result<crate::models::IcomCivModel> {
        self.model
            .context("this CI-V operation requires a selected Icom model profile")
    }

    fn active_profile(&self) -> &'static super::profile::IcomCivProfile {
        self.model
            .map(profile_for_model)
            .unwrap_or(&super::generic::CIV_PROFILE)
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
    /// The command bytes are shared, while availability is profile-owned.
    pub async fn select_vfo(&self, vfo: IcomVfo) -> Result<()> {
        let profile = profile_for_model(self.selected_model()?);
        anyhow::ensure!(
            profile.control_capabilities.supports_vfo,
            "VFO selection is not supported for {}",
            profile.model.model_name()
        );
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

    /// Select an Icom memory channel using the documented CI-V memory-mode
    /// and memory-channel commands. Complete records are handled separately.
    pub fn select_memory_channel(&self, channel: u16) -> Result<()> {
        self.selected_model()?;
        anyhow::ensure!(
            (1..=99).contains(&channel),
            "Icom memory channel must be in the documented range 1..=99"
        );
        let bcd = [
            ((channel % 100 / 10) as u8) << 4 | (channel % 10) as u8,
            0x00,
        ];
        self.transact_ack(&[0x08])?;
        self.transact_ack(&[0x09, bcd[0], bcd[1]])
    }

    /// Read the documented CI-V repeater-tone state and frequency.
    pub fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        self.selected_model()?;
        // CI-V 16/42 and 16/43 are the enable flags; 1B/00 and 1B/01
        // contain the corresponding CTCSS frequencies.
        let repeater = self.read_flag_command(&[0x16, 0x42])?;
        let tone_squelch = self.read_flag_command(&[0x16, 0x43])?;
        let mode = if tone_squelch {
            ToneMode::EncodeDecode
        } else if repeater {
            ToneMode::Encode
        } else {
            ToneMode::Off
        };
        let response = self.transact(&[0x1B, 0x00], true)?;
        let data = response_data_after_prefix(&response, &[0x1B, 0x00])?;
        let shift = self.get_repeater_shift()?;
        let offset_hz = self.get_duplex_offset_hz()?;
        Ok(RepeaterSettings {
            shift,
            offset_hz: Some(offset_hz),
            tone: ToneSettings {
                mode,
                index: 0,
                frequency_tenths_hz: Some(decode_tone_frequency(data)?),
                dtcs_code: None,
                dtcs_reverse: None,
            },
        })
    }

    pub fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        self.selected_model()?;
        let frequency = settings
            .tone
            .frequency_tenths_hz
            .context("Icom CI-V tone frequency is required")?;
        anyhow::ensure!(frequency <= 9999, "Icom tone frequency exceeds CI-V range");
        let offset_hz = settings.offset_hz.unwrap_or_default();
        anyhow::ensure!(
            offset_hz <= 999_999_999,
            "Icom duplex offset must be no more than 999999999 Hz"
        );
        let encoded = encode_tone_frequency(frequency);
        let mut offset_payload = vec![0x0D];
        offset_payload.extend_from_slice(&encode_civ_frequency_bcd(u64::from(offset_hz)));
        self.transact_ack(&offset_payload)?;
        self.set_repeater_shift(settings.shift)?;
        self.transact_ack(&[0x1B, 0x00, encoded[0], encoded[1], encoded[2]])?;
        self.transact_ack(&[0x1B, 0x01, encoded[0], encoded[1], encoded[2]])?;
        self.transact_ack(&[
            0x16,
            0x42,
            u8::from(matches!(
                settings.tone.mode,
                ToneMode::Encode | ToneMode::EncodeDecode
            )),
        ])?;
        self.transact_ack(&[
            0x16,
            0x43,
            u8::from(matches!(settings.tone.mode, ToneMode::EncodeDecode)),
        ])
    }

    fn get_repeater_shift(&self) -> Result<RepeaterShift> {
        let response = self.transact(&[0x0F], true)?;
        let data = response_data_after_prefix(&response, &[0x0F])?;
        decode_repeater_shift(data.first().copied())
    }

    fn set_repeater_shift(&self, shift: RepeaterShift) -> Result<()> {
        self.transact_ack(&[0x0F, encode_repeater_shift(shift)])
    }

    fn get_duplex_offset_hz(&self) -> Result<u32> {
        let response = self.transact(&[0x0C], true)?;
        let data = response_data_after_prefix(&response, &[0x0C])?;
        let offset = decode_civ_frequency_bcd(data).context("invalid Icom duplex offset")?;
        u32::try_from(offset).context("Icom duplex offset exceeds HAL range")
    }

    /// Read the signed RIT offset documented by the Icom CI-V `21 00`
    /// command. The wire value is four packed-BCD bytes in Hz followed by a
    /// sign byte (`00` positive, `01` negative). A CI-V NAK is surfaced as an
    /// unavailable operation rather than being decoded as a malformed value.
    pub fn get_rit_offset_hz(&self) -> Result<i32> {
        self.selected_model()?;
        let response = self.transact(&[0x21, 0x00], true)?;
        let data = response_data_after_prefix(&response, &[0x21, 0x00])?;
        anyhow::ensure!(data.len() >= 5, "Icom RIT response is too short");
        let magnitude = decode_civ_bcd(&data[..4])?;
        anyhow::ensure!(magnitude <= 9_999, "Icom RIT offset is out of range");
        let magnitude = magnitude as i32;
        match data[4] {
            0x00 => Ok(magnitude),
            0x01 => Ok(-magnitude),
            sign => anyhow::bail!("invalid Icom RIT sign {sign:#04x}"),
        }
    }

    pub fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.selected_model()?;
        anyhow::ensure!(
            (-9_999..=9_999).contains(&offset_hz),
            "Icom RIT offset must be -9999..=9999 Hz"
        );
        let magnitude = offset_hz.unsigned_abs();
        let encoded = encode_civ_bcd_fixed(magnitude, 4)?;
        let mut payload = vec![0x21, 0x00];
        payload.extend_from_slice(&encoded);
        payload.push(u8::from(offset_hz < 0));
        self.transact_ack(&payload)
    }

    pub fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        let profile = profile_for_model(self.selected_model()?);
        anyhow::ensure!(
            (1..=99).contains(&channel),
            "Icom memory channel must be 1..=99"
        );
        let channel_bcd = encode_memory_channel(channel);
        let prefix = [0x1A, 0x00];
        let response = self.transact(
            &[prefix[0], prefix[1], channel_bcd[0], channel_bcd[1]],
            true,
        )?;
        decode_icom_memory(&response, &prefix, profile.memory_layout)
    }

    pub fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        let profile = profile_for_model(self.selected_model()?);
        anyhow::ensure!(
            (1..=99).contains(&channel.channel),
            "Icom memory channel must be 1..=99"
        );
        anyhow::ensure!(
            channel
                .name
                .as_deref()
                .is_none_or(|name| name.is_ascii() && name.len() <= 10),
            "Icom memory name must be ASCII and at most 10 characters"
        );
        if profile.memory_layout == MemoryLayout::Hf {
            anyhow::ensure!(
                channel.transmit_frequency_hz.is_none(),
                "Icom HF memory records do not expose a split transmit frequency"
            );
        }
        let (base_mode, data_mode) = hal_mode_to_icom_operating_mode(channel.mode);
        let mode = base_mode_to_civ_mode(base_mode).context("unsupported Icom memory mode")?;
        if profile.memory_layout == MemoryLayout::VhfUhf {
            return self.write_vhf_memory_channel(channel);
        }
        anyhow::ensure!(
            !matches!(channel.repeater.tone.mode, ToneMode::Dtcs),
            "DTCS memory mode is only available on Icom VHF/UHF profiles"
        );
        let tone_type = match channel.repeater.tone.mode {
            ToneMode::Off => 0,
            ToneMode::EncodeDecode => 1,
            ToneMode::Encode => 2,
            ToneMode::Dtcs => 3,
        };
        let tone = encode_tone_frequency(channel.repeater.tone.frequency_tenths_hz.unwrap_or(885));
        let mut payload = vec![0x1A, 0x00];
        payload.extend_from_slice(&encode_memory_channel(channel.channel));
        payload.push(0x00);
        payload.extend_from_slice(&encode_civ_frequency_bcd(channel.frequency_hz));
        payload.extend_from_slice(&[mode, u8::from(data_mode), tone_type]);
        payload.extend_from_slice(&tone);
        payload.extend_from_slice(&tone);
        payload.extend_from_slice(channel.name.as_deref().unwrap_or("").as_bytes());
        self.transact_ack(&payload)
    }

    fn write_vhf_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        let (base_mode, data_mode) = hal_mode_to_icom_operating_mode(channel.mode);
        let mode = base_mode_to_civ_mode(base_mode).context("unsupported Icom memory mode")?;
        let tone_type = match channel.repeater.tone.mode {
            ToneMode::Off => 0,
            ToneMode::Encode => 1,
            ToneMode::EncodeDecode => 2,
            ToneMode::Dtcs => 3,
        };
        let duplex = match channel.repeater.shift {
            RepeaterShift::Simplex => 0,
            RepeaterShift::Minus => 1,
            RepeaterShift::Plus => 2,
        };
        let tone = encode_tone_frequency(channel.repeater.tone.frequency_tenths_hz.unwrap_or(885));
        let dtcs = encode_civ_bcd_fixed(channel.repeater.tone.dtcs_code.unwrap_or(0) as u32, 3)?;
        let offset = encode_civ_bcd_fixed(channel.repeater.offset_hz.unwrap_or(0), 3)?;
        let mut payload = vec![0x1A, 0x00, 0x00, 0x00]; // memory group
        payload.extend_from_slice(&encode_memory_channel(channel.channel));
        payload.extend_from_slice(&[0x00]); // select setting
        payload.extend_from_slice(&encode_civ_frequency_bcd(channel.frequency_hz));
        payload.extend_from_slice(&[mode, 0x01, u8::from(data_mode)]);
        payload.push((duplex << 4) | tone_type);
        payload.push(0x00); // digital squelch off
        payload.extend_from_slice(&tone);
        payload.extend_from_slice(&tone);
        payload.extend_from_slice(&dtcs);
        payload.push(0x00); // DV code
        payload.extend_from_slice(&offset);
        payload.extend(std::iter::repeat_n(b' ', 24)); // DV call signs
        let name = channel.name.as_deref().unwrap_or("").as_bytes();
        payload.extend_from_slice(name);
        payload.extend(std::iter::repeat_n(
            b' ',
            16usize.saturating_sub(name.len()),
        ));
        self.transact_ack(&payload)
    }

    fn read_flag_command(&self, prefix: &[u8]) -> Result<bool> {
        let response = self.transact(prefix, true)?;
        let data = response_data_after_prefix(&response, prefix)?;
        match data.first().copied() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            Some(value) => anyhow::bail!("invalid Icom flag value: {value:#04x}"),
            None => anyhow::bail!("missing Icom flag value"),
        }
    }

    pub fn probe(&self) -> Result<RadioStatus> {
        self.probe_direct_serial()
    }

    /// Try caller-supplied baud rates and radio addresses using harmless
    /// frequency/mode reads. This never changes radio settings and is not
    /// invoked by constructors. It is intended for an explicit connection UI
    /// action on a dedicated CI-V link.
    pub fn probe_candidates(
        &self,
        baud_rates: &[u32],
        radio_addresses: &[u8],
    ) -> Result<IcomProbeResult> {
        anyhow::ensure!(
            !baud_rates.is_empty(),
            "CI-V probe requires a baud candidate"
        );
        anyhow::ensure!(
            !radio_addresses.is_empty(),
            "CI-V probe requires a radio-address candidate"
        );
        anyhow::ensure!(
            self.external_transport
                .lock()
                .map(|transport| transport.is_none())
                .unwrap_or(false),
            "CI-V candidate probing requires a native serial port"
        );

        let mut failures = Vec::new();
        for &baud_rate in baud_rates {
            for &radio_address in radio_addresses {
                let candidate = IcomCiVRadio::new_internal(
                    self.model,
                    self.port.clone(),
                    baud_rate,
                    self.controller_address,
                    radio_address,
                )
                .with_serial_policy(self.serial_policy);
                match candidate.probe() {
                    Ok(status) => {
                        return Ok(IcomProbeResult {
                            baud_rate,
                            radio_address,
                            status,
                        })
                    }
                    Err(error) => failures.push(format!(
                        "baud {baud_rate}, address {radio_address:#04x}: {error}"
                    )),
                }
            }
        }

        Err(anyhow!(
            "CI-V probe failed for all candidates: {}",
            failures.join("; ")
        ))
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
        F: FnMut(&mut dyn RadioTransport) -> Result<T>,
    {
        if let Some(transport) = self
            .external_transport
            .lock()
            .map_err(|_| anyhow::anyhow!("radio external transport lock poisoned"))?
            .as_mut()
        {
            transport
                .set_timeout(timeout)
                .context("failed to update external CI-V transport timeout")?;
            return operation(&mut **transport);
        }

        let mut guard = self
            .serial_port
            .lock()
            .map_err(|_| anyhow::anyhow!("radio serial port lock poisoned"))?;

        if guard.is_none() {
            let configured_port = self.port.trim();
            if configured_port.is_empty() {
                let candidates = enumerate_serial_ports().unwrap_or_default();
                let mut failures = Vec::new();
                for candidate in candidates {
                    match self.open_specific_port_with_timeout(&candidate, timeout) {
                        Ok(mut transport) => match operation(&mut *transport) {
                            Ok(value) => {
                                *guard = Some(transport);
                                return Ok(value);
                            }
                            Err(error) => {
                                failures.push(format!("{candidate}: {error}"));
                                continue;
                            }
                        },
                        Err(error) => failures.push(format!("{candidate}: {error}")),
                    }
                }
                return Err(anyhow!(
                    "failed to find a responsive CI-V radio on Auto port (tried: {})",
                    failures.join("; ")
                ));
            }
            *guard = Some(self.open_port_with_timeout(timeout)?);
        }

        let port = guard
            .as_mut()
            .context("radio serial port slot unavailable")?;
        port.set_timeout(timeout)
            .context("failed to update radio serial timeout")?;
        operation(&mut **port)
    }

    fn open_port_with_timeout(&self, timeout: Duration) -> Result<Box<dyn RadioTransport>> {
        let configured_port = self.port.trim();
        let candidates = if configured_port.is_empty() {
            enumerate_serial_ports().unwrap_or_default()
        } else {
            vec![configured_port.to_string()]
        };

        let mut failures = Vec::new();
        for candidate in &candidates {
            match self.open_specific_port_with_timeout(candidate, timeout) {
                Ok(port) => return Ok(port),
                Err(err) => failures.push(format!("{candidate}: {err}")),
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

    fn open_specific_port_with_timeout(
        &self,
        candidate: &str,
        timeout: Duration,
    ) -> Result<Box<dyn RadioTransport>> {
        let mut port = serialport::new(candidate, self.baud_rate)
            .timeout(timeout)
            .open()
            .with_context(|| format!("failed to open serial port: {candidate}"))?;
        port.set_flow_control(if self.serial_policy.hardware_flow_control {
            serialport::FlowControl::Hardware
        } else {
            serialport::FlowControl::None
        })
        .map_err(std::io::Error::other)
        .context("failed to configure Icom CI-V flow control")?;
        if let Some(enabled) = self.serial_policy.dtr {
            port.write_data_terminal_ready(enabled)
                .map_err(std::io::Error::other)
                .context("failed to configure Icom CI-V DTR")?;
        }
        if let Some(enabled) = self.serial_policy.rts {
            port.write_request_to_send(enabled)
                .map_err(std::io::Error::other)
                .context("failed to configure Icom CI-V RTS")?;
        }
        port.clear(serialport::ClearBuffer::Input)
            .map_err(std::io::Error::other)
            .context("failed to clear initial Icom CI-V input")?;
        thread::sleep(self.serial_policy.startup_settle);
        eprintln!("[rigwright] opened serial port: {candidate}");
        Ok(Box::new(SerialPortTransport(port)))
    }

    fn close_serial_port(&self) {
        if let Ok(mut guard) = self.serial_port.lock() {
            *guard = None;
        }
        if let Ok(mut reader) = self.scope_stream_reader.lock() {
            *reader = ScopeStreamReader::default();
        }
    }

    fn write_frame(&self, port: &mut dyn CiVTransport, frame: &[u8]) -> Result<()> {
        port.write_all(frame)
            .context("failed to write CI-V frame")?;
        Ok(())
    }

    fn read_response_matching<F>(
        &self,
        port: &mut dyn RadioTransport,
        timeout: Duration,
        echo_frame: Option<&[u8]>,
        mut matcher: F,
    ) -> Result<Vec<u8>>
    where
        F: FnMut(&[u8]) -> bool,
    {
        if let Some(frame) = self.take_retained_match(echo_frame, &mut matcher) {
            return Ok(frame);
        }
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 1024];
        let mut pending = Vec::new();

        while Instant::now() < deadline {
            match port.read(&mut buf) {
                Ok(bytes) if bytes > 0 => {
                    if let Ok(mut state) = self.transport_state.lock() {
                        state.metrics.bytes_read =
                            state.metrics.bytes_read.saturating_add(bytes as u64);
                    }
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
                        if let Ok(mut state) = self.transport_state.lock() {
                            state.metrics.frames_received =
                                state.metrics.frames_received.saturating_add(1);
                        }
                        // With CI-V USB Echo Back enabled, the radio/USB
                        // interface returns the exact outbound frame before
                        // the real ACK or data response. It is transport
                        // noise, not a response to match.
                        if echo_frame.is_some_and(|echo| frame == echo) {
                            if let Ok(mut state) = self.transport_state.lock() {
                                state.metrics.echo_frames_ignored =
                                    state.metrics.echo_frames_ignored.saturating_add(1);
                            }
                            continue;
                        }
                        if !is_radio_to_controller_frame(
                            &frame,
                            self.radio_address,
                            self.controller_address,
                        ) {
                            continue;
                        }

                        publish_civ_event(&self.event_router, &frame);
                        self.note_event_received();

                        if matcher(&frame) {
                            return Ok(frame);
                        }
                        self.retain_frame(frame);
                    }
                }
                Ok(_) => {}
                Err(err) if err.kind() == ErrorKind::TimedOut => {
                    // keep waiting until timeout
                    thread::sleep(Duration::from_millis(1));
                }
                Err(err) => return Err(err).context("failed to read matched CI-V response"),
            }
        }

        Ok(Vec::new())
    }

    fn take_retained_match<F>(&self, echo_frame: Option<&[u8]>, matcher: &mut F) -> Option<Vec<u8>>
    where
        F: FnMut(&[u8]) -> bool,
    {
        let mut state = self.transport_state.lock().ok()?;
        let index = state
            .retained_frames
            .iter()
            .position(|frame| echo_frame.is_none_or(|echo| frame != echo) && matcher(frame))?;
        let frame = state.retained_frames.remove(index)?;
        Some(frame)
    }

    fn retain_frame(&self, frame: Vec<u8>) {
        if is_spectrum_data_frame(&frame) {
            return;
        }
        if let Ok(mut state) = self.transport_state.lock() {
            if state.retained_frames.len() >= MAX_RETAINED_CI_V_FRAMES {
                state.retained_frames.pop_front();
                state.metrics.frames_dropped = state.metrics.frames_dropped.saturating_add(1);
            }
            state.retained_frames.push_back(frame);
            state.metrics.frames_retained = state.metrics.frames_retained.saturating_add(1);
        }
    }

    fn response_timeout(&self, expect_data_frame: bool) -> Duration {
        let base: u64 = if expect_data_frame { 1500 } else { 1200 };
        let consecutive = self
            .transport_state
            .lock()
            .map(|state| state.metrics.consecutive_timeouts)
            .unwrap_or(0);
        let multiplier = 1_u32.saturating_add(consecutive.min(2));
        Duration::from_millis(base * u64::from(multiplier))
    }

    fn transact(&self, payload: &[u8], expect_data_frame: bool) -> Result<Vec<u8>> {
        let frame = self.build_frame_payload(payload);
        let response_timeout = self.response_timeout(expect_data_frame);
        let started = Instant::now();
        if let Ok(mut state) = self.transport_state.lock() {
            state.metrics.commands_started = state.metrics.commands_started.saturating_add(1);
        }
        let response = self.with_serial_port(response_timeout, |port| {
            self.write_frame(port, &frame)?;

            if expect_data_frame {
                self.read_response_matching(port, response_timeout, Some(&frame), |response| {
                    frame_matches_request(response, payload)
                })
            } else {
                self.read_response_matching(port, response_timeout, Some(&frame), |response| {
                    is_ack_frame(response) || is_nak_frame(response)
                })
            }
        })?;
        if let Ok(mut state) = self.transport_state.lock() {
            if response.is_empty() {
                state.metrics.response_timeouts = state.metrics.response_timeouts.saturating_add(1);
                state.metrics.consecutive_timeouts =
                    state.metrics.consecutive_timeouts.saturating_add(1);
            } else {
                state.metrics.responses_matched = state.metrics.responses_matched.saturating_add(1);
                state.metrics.total_response_time += started.elapsed();
                state.metrics.consecutive_timeouts = 0;
            }
        }
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

    fn transact_scope_setting(&self, payload: &[u8]) -> Result<()> {
        match self.transact_ack(payload) {
            Ok(()) => Ok(()),
            Err(error)
                if self.active_profile().scope_ack_optional
                    && error.to_string().contains("did not acknowledge") =>
            {
                // Some CI-V firmware accepts scope writes without an ACK; the
                // selected profile owns that exception.
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn set_frequency_blocking(&self, hz: u64) -> Result<()> {
        anyhow::ensure!(
            hz <= 9_999_999_999,
            "frequency {hz} Hz does not fit the five-byte CI-V BCD format"
        );
        let profile = self.active_profile();
        if !profile.supports_frequency(hz) {
            anyhow::bail!(
                "frequency {hz} Hz is outside the documented CAT range for {}",
                profile.model.model_name()
            );
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
        self.get_meter_blocking_with_timeout(id, Duration::from_millis(1_500))
    }

    fn get_meter_blocking_with_timeout(&self, id: MeterId, timeout: Duration) -> Result<u8> {
        let profile = self.active_profile();
        anyhow::ensure!(
            profile.supports_meter(id),
            "meter {id:?} is not supported by this Icom profile"
        );
        let prefix = meter_command_prefix(id);
        let frame = self.build_frame_payload(prefix);
        let response = self.with_serial_port(timeout, |port| {
            self.write_frame(port, &frame)?;
            self.read_response_matching(port, timeout, Some(&frame), |response| {
                frame_matches_request(response, prefix)
            })
        })?;
        let data = response_data_after_prefix(&response, prefix)?;
        decode_level_255_bcd(data).context("invalid CI-V meter payload")
    }

    /// Read a meter without allowing a missing response to monopolize a
    /// shared scope/CI-V worker. Meter polling is intentionally allowed to use
    /// a shorter timeout than normal CAT operations.
    pub async fn get_meter_with_timeout(
        &self,
        id: MeterId,
        timeout: Duration,
    ) -> Result<Option<u8>> {
        Ok(Some(self.get_meter_blocking_with_timeout(id, timeout)?))
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
        let profile = profile_for_model(self.selected_model()?);
        if id == ControlId::Attenuator {
            if let Some(ControlValue::U8(db)) = value.as_ref() {
                anyhow::ensure!(
                    profile.attenuator_values.contains(db),
                    "attenuator setting {db} dB is not documented for {}",
                    profile.model.model_name()
                );
            }
        }
        if id == ControlId::Preamp {
            if let Some(ControlValue::U8(level)) = value.as_ref() {
                anyhow::ensure!(
                    *level <= profile.preamp_max_level,
                    "preamp level {level} exceeds {} levels for {}",
                    profile.preamp_max_level,
                    profile.model.model_name()
                );
            }
        }
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
            anyhow::ensure!(
                profile.control_capabilities.supports_data_mode,
                "DataMode is not supported by this Icom profile"
            );
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
            anyhow::ensure!(
                !profile.control_capabilities.filter_values.is_empty(),
                "Filter is not supported by this Icom profile"
            );
            return match op {
                ControlOp::Set => {
                    let filter = match value.context("missing control value for set operation")? {
                        ControlValue::U8(v)
                            if profile.control_capabilities.filter_values.contains(&v) =>
                        {
                            v
                        }
                        _ => anyhow::bail!("Filter control expects a documented profile value"),
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

        if id == ControlId::TuningStep {
            return match op {
                ControlOp::Set => {
                    let step = match value.context("missing tuning-step value")? {
                        ControlValue::U8(value) if value <= 5 => value,
                        _ => anyhow::bail!("IC-7200 tuning step expects U8 0..=5"),
                    };
                    self.transact_ack(&[0x10, step])?;
                    Ok(None)
                }
                ControlOp::Get => {
                    anyhow::bail!("IC-7200 tuning-step selection is write-only through CI-V")
                }
            };
        }

        let spec = profile
            .control(id)
            .with_context(|| format!("unsupported CI-V control: {id:?}"))?;

        match op {
            ControlOp::Set => {
                let v = value.context("missing control value for set operation")?;
                if id == ControlId::Attenuator {
                    let db = match v {
                        ControlValue::U8(db) if db <= 99 => ((db / 10) << 4) | (db % 10),
                        _ => anyhow::bail!("Icom attenuator expects a documented dB value"),
                    };
                    let _ = self.transact(&[0x11, db], false)?;
                    return Ok(None);
                }
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
            (ControlId::Vfo, ControlOp::Get) => {
                // CI-V 0x07 is a selector command. The IC-7300 accepts the
                // write form (0x07 00/01) but NAKs the read form (0x07), so
                // there is no reliable active-VFO readback through this API.
                anyhow::bail!(
                    "VFO selection is write-only for {}",
                    self.selected_model()?.model_name()
                )
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

    fn start_spectrum_stream_blocking(&self) -> Result<()> {
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
        self.transact_scope_setting(scope.enable_command)?;
        self.transact_scope_setting(scope.stream_command)?;
        Ok(())
    }

    fn enable_spectrum_stream_blocking(&self, timeout: Duration) -> Result<Vec<u8>> {
        self.start_spectrum_stream_blocking()?;
        // USB CI-V scope output is unsolicited once enabled. Some IC-7300
        // firmware accepts 27 10/27 11 but NAKs an immediate 27 00 request;
        // lifecycle start must therefore consume the native stream instead
        // of requiring a request/response waveform exchange. The explicit
        // request_scope_waveform_bins API remains available separately for
        // radios/firmware that implement that form.
        self.try_scope_waveform_bins_stream_blocking(timeout)?
            .context("scope output enabled but no complete scope sweep arrived")
    }

    fn disable_spectrum_stream_blocking(&self) -> Result<()> {
        let scope = profile_for_model(self.selected_model()?)
            .scope
            .context("native CI-V scope streaming is not implemented for this model")?;
        self.transact_scope_setting(scope.disable_stream_command)?;
        self.close_serial_port();
        Ok(())
    }

    pub async fn enable_spectrum_stream(&self, timeout: Duration) -> Result<Vec<u8>> {
        self.enable_spectrum_stream_blocking(timeout)
    }

    /// Enable native scope output without waiting for the first complete
    /// sweep. Network services should use this form so their request loop can
    /// continue forwarding audio and controls while scope frames arrive.
    pub async fn start_spectrum_stream(&self) -> Result<()> {
        self.start_spectrum_stream_blocking()
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
        self.request_scope_waveform_bins_blocking_timeout(Duration::from_millis(1_500))
    }

    fn request_scope_waveform_bins_blocking_timeout(&self, timeout: Duration) -> Result<Vec<u8>> {
        let request = self.build_frame_payload(&[0x27, 0x00]);
        let result = self.with_serial_port(Duration::from_millis(700), |port| {
            self.write_frame(port, &request)?;
            self.read_stream_scope_bins(port, timeout)
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
            Err(err) => Err(err),
        }
    }

    fn try_scope_waveform_bins_stream_blocking(
        &self,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        self.drain_scope_waveform_sweeps_blocking(timeout)
            .map(|sweeps| sweeps.into_iter().next())
    }

    fn drain_scope_waveform_sweeps_blocking(&self, timeout: Duration) -> Result<Vec<Vec<u8>>> {
        self.with_serial_port(Duration::from_millis(25), |port| {
            self.read_stream_scope_sweeps(port, timeout)
        })
    }

    fn read_stream_scope_bins(
        &self,
        port: &mut dyn RadioTransport,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        Ok(self
            .read_stream_scope_sweeps(port, timeout)?
            .into_iter()
            .next())
    }

    fn read_stream_scope_sweeps(
        &self,
        port: &mut dyn RadioTransport,
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
    fn event_router(&self) -> Option<RadioEventRouter> {
        Some(self.event_router.clone())
    }

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

    fn link_health(&self) -> crate::hal::LinkHealth {
        let metrics = self.transport_metrics();
        let avg_response = if metrics.responses_matched > 0 {
            Some(metrics.total_response_time / metrics.responses_matched as u32)
        } else {
            None
        };
        crate::hal::LinkHealth {
            commands_started: Some(metrics.commands_started),
            responses_matched: Some(metrics.responses_matched),
            response_timeouts: Some(metrics.response_timeouts),
            consecutive_timeouts: Some(metrics.consecutive_timeouts),
            avg_response,
            frames_dropped: Some(metrics.frames_dropped),
        }
    }

    fn event_stream_age(&self) -> Option<Duration> {
        self.event_stream_age()
    }

    async fn get_power(&self) -> Result<bool> {
        anyhow::bail!("Icom CI-V power state is write-only")
    }

    async fn set_power(&self, enabled: bool) -> Result<()> {
        self.set_power_blocking(enabled)
    }

    fn supports_scope(&self) -> bool {
        self.supports_scope()
    }

    fn supports_iq_output(&self) -> bool {
        self.model
            .is_some_and(|model| profile_for_model(model).supports_iq_output)
    }

    async fn set_scope_configuration(&self, config: ScopeConfiguration) -> Result<()> {
        IcomCiVRadio::set_scope_configuration(self, config).await
    }

    async fn get_scope_state(&self) -> Result<ScopeState> {
        IcomCiVRadio::get_scope_state(self).await
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
        self.active_profile().supports_meter(id)
    }

    fn filter_bandwidth_hz(&self, mode: Mode, filter: u8) -> Option<u32> {
        self.active_profile().filter_bandwidth_hz(mode, filter)
    }

    fn swr_sweep_setup(&self) -> Option<SwrSweepSetup> {
        self.active_profile().swr_sweep_setup
    }

    fn meter_presentation(&self, id: MeterId, normalized: u8) -> Option<MeterPresentation> {
        self.active_profile().meter_presentation(id, normalized)
    }

    fn meter_poll_spec(&self, id: MeterId) -> Option<MeterPollSpec> {
        self.active_profile()
            .meter_poll_specs
            .iter()
            .find(|spec| spec.meter == id)
            .copied()
    }

    fn scope_metadata(&self) -> Option<ScopeMetadata> {
        self.model()
            .map(profile_for_model)
            .and_then(|profile| profile.scope_metadata())
    }

    fn control_max(&self, id: ControlId) -> Option<u8> {
        self.active_profile().control_max(id)
    }

    fn supported_control_values(&self, id: ControlId) -> Option<&'static [u8]> {
        self.active_profile().supported_control_values(id)
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.active_profile().supports_control(id)
    }

    fn supports_control_read(&self, id: ControlId) -> bool {
        let Some(model) = self.model() else {
            return false;
        };
        self.supports_control(id)
            && id != ControlId::RawCiV
            && id != ControlId::TuningStep
            && (id != ControlId::Vfo || profile_for_model(model).control_capabilities.vfo_readable)
    }

    fn supports_control_write(&self, id: ControlId) -> bool {
        self.supports_control(id) && id != ControlId::RawCiV
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

    async fn select_memory_channel(&self, channel: u16) -> Result<()> {
        IcomCiVRadio::select_memory_channel(self, channel)
    }

    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        IcomCiVRadio::get_repeater_settings(self)
    }

    async fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        IcomCiVRadio::set_repeater_settings(self, settings)
    }

    async fn get_rit_offset_hz(&self) -> Result<i32> {
        IcomCiVRadio::get_rit_offset_hz(self)
    }

    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        IcomCiVRadio::set_rit_offset_hz(self, offset_hz)
    }

    fn supports_repeater_settings(&self) -> bool {
        self.active_profile().supports_repeater_settings
    }

    fn supports_memory_channels(&self) -> bool {
        self.active_profile().supports_memory_channels
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

fn publish_civ_event(router: &RadioEventRouter, frame: &[u8]) {
    let payload = &frame[4..frame.len() - 1];
    match payload.first().copied() {
        Some(0x03) => {
            if let Some(frequency_hz) = decode_civ_frequency_bcd(&payload[1..]) {
                router.publish(RadioEvent::FrequencyChanged { frequency_hz });
            }
        }
        Some(0x06) => {
            if let Some(mode) = decode_event_mode(payload.get(1).copied()) {
                router.publish(RadioEvent::ModeChanged { mode });
            }
        }
        Some(0x1C) if payload.get(1) == Some(&0x00) => {
            if let Some(value) = payload.get(2) {
                router.publish(RadioEvent::PttChanged {
                    enabled: *value != 0,
                });
            }
        }
        Some(0x15) => {
            let id = match payload.get(1).copied() {
                Some(0x02) => Some(MeterId::Signal),
                Some(0x11) => Some(MeterId::Power),
                Some(0x12) => Some(MeterId::Swr),
                Some(0x13) => Some(MeterId::Alc),
                Some(0x14) => Some(MeterId::Compression),
                Some(0x15) => Some(MeterId::Voltage),
                Some(0x16) => Some(MeterId::Current),
                Some(0x17) => Some(MeterId::Temperature),
                _ => None,
            };
            if let (Some(id), Some(value)) = (id, decode_level_255_bcd(&payload[2..])) {
                router.publish(RadioEvent::MeterChanged { id, value });
            }
        }
        Some(0x07)
            if payload
                .get(1)
                .is_some_and(|value| (0xD0..=0xD1).contains(value)) =>
        {
            router.publish(RadioEvent::ReceiverChanged {
                receiver: payload[1] - 0xD0,
            });
        }
        _ => {
            router.publish(RadioEvent::Raw {
                payload: payload.to_vec(),
            });
        }
    }
}

fn decode_event_mode(value: Option<u8>) -> Option<Mode> {
    Some(match value? {
        0x00 => Mode::Lsb,
        0x01 => Mode::Usb,
        0x02 => Mode::Am,
        0x03 => Mode::Cw,
        0x04 => Mode::Rtty,
        0x05 => Mode::Fm,
        0x06 => Mode::CwReverse,
        0x07 => Mode::RttyReverse,
        0x08 => Mode::Data,
        0x09 => Mode::Wfm,
        _ => return None,
    })
}

fn encode_civ_menu_index(index: u16) -> [u8; 2] {
    [
        decimal_to_bcd((index / 100) as u8),
        decimal_to_bcd((index % 100) as u8),
    ]
}

fn scope_center_type_value(value: crate::hal_types::ScopeCenterType) -> u8 {
    match value {
        crate::hal_types::ScopeCenterType::FilterCenter => 0,
        crate::hal_types::ScopeCenterType::CarrierPoint => 1,
        crate::hal_types::ScopeCenterType::CarrierPointAbsolute => 2,
    }
}

fn scope_max_hold_value(value: crate::hal_types::ScopeMaxHold) -> u8 {
    match value {
        crate::hal_types::ScopeMaxHold::Off => 0,
        crate::hal_types::ScopeMaxHold::TenSeconds => 1,
        crate::hal_types::ScopeMaxHold::Continuous => 2,
    }
}

fn scope_marker_position_value(value: crate::hal_types::ScopeMarkerPosition) -> u8 {
    match value {
        crate::hal_types::ScopeMarkerPosition::FilterCenter => 0,
        crate::hal_types::ScopeMarkerPosition::CarrierPoint => 1,
    }
}

fn scope_waveform_type_value(value: crate::hal_types::ScopeWaveformType) -> u8 {
    match value {
        crate::hal_types::ScopeWaveformType::Fill => 0,
        crate::hal_types::ScopeWaveformType::FillAndLine => 1,
    }
}

fn encode_scope_color(color: ScopeColor) -> [u8; 6] {
    let component = |value: u8| [decimal_to_bcd(value / 100), decimal_to_bcd(value % 100)];
    let red = component(color.red);
    let green = component(color.green);
    let blue = component(color.blue);
    [red[0], red[1], green[0], green[1], blue[0], blue[1]]
}

fn decode_scope_color(data: &[u8]) -> Result<ScopeColor> {
    anyhow::ensure!(data.len() >= 6, "short CI-V RGB color payload");
    let decode = |high: u8, low: u8| -> Result<u8> {
        let value = u16::from(high & 0x0F) * 100 + u16::from(low >> 4) * 10 + u16::from(low & 0x0F);
        anyhow::ensure!(value <= 255, "invalid CI-V RGB component {value}");
        Ok(value as u8)
    };
    Ok(ScopeColor {
        red: decode(data[0], data[1])?,
        green: decode(data[2], data[3])?,
        blue: decode(data[4], data[5])?,
    })
}

fn decimal_to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
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

fn encode_tone_frequency(value_tenths_hz: u32) -> [u8; 3] {
    let mut remaining = value_tenths_hz;
    let mut out = [0u8; 3];
    for byte in &mut out {
        let low = (remaining % 10) as u8;
        remaining /= 10;
        let high = (remaining % 10) as u8;
        remaining /= 10;
        *byte = (high << 4) | low;
    }
    out
}

fn decode_tone_frequency(data: &[u8]) -> Result<u32> {
    anyhow::ensure!(data.len() >= 3, "Icom tone frequency response is too short");
    let mut multiplier = 1u32;
    let mut value = 0u32;
    for byte in data.iter().take(3) {
        let low = byte & 0x0f;
        let high = (byte >> 4) & 0x0f;
        anyhow::ensure!(low <= 9 && high <= 9, "invalid Icom tone-frequency BCD");
        value += u32::from(low) * multiplier;
        multiplier *= 10;
        value += u32::from(high) * multiplier;
        multiplier *= 10;
    }
    Ok(value)
}

fn decode_repeater_shift(value: Option<u8>) -> Result<RepeaterShift> {
    match value {
        Some(0x10) | Some(0x00) | Some(0x01) => Ok(RepeaterShift::Simplex),
        Some(0x11) => Ok(RepeaterShift::Minus),
        Some(0x12) => Ok(RepeaterShift::Plus),
        Some(value) => anyhow::bail!("invalid Icom repeater shift: {value:#04x}"),
        None => anyhow::bail!("missing Icom repeater shift"),
    }
}

fn encode_repeater_shift(shift: RepeaterShift) -> u8 {
    match shift {
        RepeaterShift::Simplex => 0x10,
        RepeaterShift::Minus => 0x11,
        RepeaterShift::Plus => 0x12,
    }
}

fn encode_memory_channel(channel: u16) -> [u8; 2] {
    [
        ((channel % 100 / 10) as u8) << 4 | (channel % 10) as u8,
        ((channel / 100) as u8) << 4,
    ]
}

fn decode_icom_memory(
    response: &[u8],
    prefix: &[u8],
    layout: MemoryLayout,
) -> Result<MemoryChannel> {
    let data = response_data_after_prefix(response, prefix)?;
    if layout == MemoryLayout::VhfUhf {
        return decode_vhf_memory_data(data);
    }
    anyhow::ensure!(data.len() >= 27, "Icom memory record is too short");
    let channel = decode_memory_channel(&data[0..2])?;
    let frequency_hz = decode_civ_bcd(&data[3..8])?;
    let mode = {
        let base = civ_mode_to_base_mode(data[8]);
        let data_mode = data[9] != 0;
        match (base, data_mode) {
            (_, true) => Mode::Data,
            (BaseMode::Lsb, false) => Mode::Lsb,
            (BaseMode::Usb, false) => Mode::Usb,
            (BaseMode::Cw, false) => Mode::Cw,
            (BaseMode::Am, false) => Mode::Am,
            (BaseMode::Fm, false) => Mode::Fm,
            (BaseMode::Wfm, false) => Mode::Wfm,
            (BaseMode::Rtty, false) => Mode::Rtty,
            (BaseMode::CwR, false) => Mode::CwReverse,
            (BaseMode::RttyR, false) => Mode::RttyReverse,
            (BaseMode::Unknown(value), _) => {
                anyhow::bail!("unsupported Icom memory mode {value:#04x}")
            }
        }
    };
    let tone = match data[10] {
        0 => ToneMode::Off,
        1 => ToneMode::EncodeDecode,
        2 => ToneMode::Encode,
        value => anyhow::bail!("invalid Icom memory tone mode {value}"),
    };
    let name = String::from_utf8_lossy(&data[17..27]).trim_end().to_owned();
    Ok(MemoryChannel {
        channel,
        name: (!name.is_empty()).then_some(name),
        frequency_hz,
        transmit_frequency_hz: None,
        mode,
        repeater: RepeaterSettings {
            shift: RepeaterShift::Simplex,
            offset_hz: None,
            tone: ToneSettings {
                mode: tone,
                index: 0,
                frequency_tenths_hz: Some(decode_tone_frequency(&data[11..14])?),
                dtcs_code: None,
                dtcs_reverse: None,
            },
        },
    })
}

fn decode_vhf_memory_data(data: &[u8]) -> Result<MemoryChannel> {
    anyhow::ensure!(data.len() >= 68, "Icom VHF/UHF memory record is too short");
    let channel = decode_memory_channel(&data[2..4])?;
    let frequency_hz = decode_civ_bcd(&data[5..10])?;
    let base = civ_mode_to_base_mode(data[10]);
    let mode = if data[12] != 0 {
        Mode::Data
    } else {
        match base {
            BaseMode::Lsb => Mode::Lsb,
            BaseMode::Usb => Mode::Usb,
            BaseMode::Cw => Mode::Cw,
            BaseMode::Am => Mode::Am,
            BaseMode::Fm => Mode::Fm,
            BaseMode::Wfm => Mode::Wfm,
            BaseMode::Rtty => Mode::Rtty,
            BaseMode::CwR => Mode::CwReverse,
            BaseMode::RttyR => Mode::RttyReverse,
            BaseMode::Unknown(value) => anyhow::bail!("unsupported Icom memory mode {value:#04x}"),
        }
    };
    let tone_type = data[13] & 0x0F;
    let tone = match tone_type {
        0 => ToneMode::Off,
        1 => ToneMode::Encode,
        2 => ToneMode::EncodeDecode,
        3 => ToneMode::Dtcs,
        value => anyhow::bail!("invalid Icom VHF/UHF memory tone mode {value}"),
    };
    let shift = match data[13] >> 4 {
        0 => RepeaterShift::Simplex,
        1 => RepeaterShift::Minus,
        2 => RepeaterShift::Plus,
        value => anyhow::bail!("invalid Icom duplex setting {value}"),
    };
    let offset = decode_civ_bcd(&data[25..28])? as u32;
    let dtcs = decode_civ_bcd(&data[21..24])? as u16;
    let name = String::from_utf8_lossy(&data[52..68]).trim_end().to_owned();
    Ok(MemoryChannel {
        channel,
        name: (!name.is_empty()).then_some(name),
        frequency_hz,
        transmit_frequency_hz: match shift {
            RepeaterShift::Simplex => None,
            RepeaterShift::Plus => Some(frequency_hz.saturating_add(u64::from(offset))),
            RepeaterShift::Minus => Some(frequency_hz.saturating_sub(u64::from(offset))),
        },
        mode,
        repeater: RepeaterSettings {
            shift,
            offset_hz: Some(offset),
            tone: ToneSettings {
                mode: tone,
                index: 0,
                frequency_tenths_hz: Some(decode_tone_frequency(&data[15..18])?),
                dtcs_code: Some(dtcs),
                // The memory record carries the DTCS code here; polarity is
                // represented by the separate 1B 02 command and is not the
                // following DV digital-code byte.
                dtcs_reverse: None,
            },
        },
    })
}

fn decode_memory_channel(data: &[u8]) -> Result<u16> {
    anyhow::ensure!(data.len() >= 2, "Icom memory channel is too short");
    for byte in data.iter().take(2) {
        anyhow::ensure!(
            (byte & 0x0f) <= 9 && (byte >> 4) <= 9,
            "invalid Icom memory channel BCD"
        );
    }
    Ok(u16::from(data[0] & 0x0f)
        + u16::from(data[0] >> 4) * 10
        + u16::from(data[1] & 0x0f) * 100
        + u16::from(data[1] >> 4) * 1000)
}

fn decode_civ_bcd(data: &[u8]) -> Result<u64> {
    let mut value = 0u64;
    let mut multiplier = 1u64;
    for byte in data {
        let low = byte & 0x0f;
        let high = byte >> 4;
        anyhow::ensure!(low <= 9 && high <= 9, "invalid Icom frequency BCD");
        value += u64::from(low) * multiplier;
        multiplier *= 10;
        value += u64::from(high) * multiplier;
        multiplier *= 10;
    }
    Ok(value)
}

fn encode_civ_bcd_fixed(value: u32, bytes: usize) -> Result<Vec<u8>> {
    let max = 10u32.pow((bytes * 2) as u32) - 1;
    anyhow::ensure!(value <= max, "value does not fit packed BCD");
    let mut remaining = value;
    let mut out = Vec::with_capacity(bytes);
    for _ in 0..bytes {
        let low = (remaining % 10) as u8;
        remaining /= 10;
        let high = (remaining % 10) as u8;
        remaining /= 10;
        out.push((high << 4) | low);
    }
    Ok(out)
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
    if spec.id == ControlId::Attenuator {
        let value = *data.first().context("missing attenuator control data")?;
        anyhow::ensure!(
            value & 0x0F <= 9 && value >> 4 <= 9,
            "invalid Icom attenuator BCD value {value:#04x}"
        );
        return Ok(ControlValue::U8((value >> 4) * 10 + (value & 0x0F)));
    }
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
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    #[test]
    fn scope_colors_use_four_digit_bcd_components() {
        let color = ScopeColor {
            red: 0,
            green: 127,
            blue: 255,
        };
        assert_eq!(
            encode_scope_color(color),
            [0x00, 0x00, 0x01, 0x27, 0x02, 0x55]
        );
        assert_eq!(
            decode_scope_color(&encode_scope_color(color)).unwrap(),
            color
        );
    }

    #[test]
    fn scope_color_decoder_rejects_values_above_255() {
        assert!(decode_scope_color(&[0x03, 0x00, 0x00, 0x00, 0x00, 0x00]).is_err());
    }

    struct TestTransport {
        reads: VecDeque<Vec<u8>>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl TestTransport {
        fn response(radio_address: u8, controller_address: u8, payload: &[u8]) -> Vec<u8> {
            let mut frame = vec![
                CI_V_FRAME_START,
                CI_V_FRAME_START,
                controller_address,
                radio_address,
            ];
            frame.extend_from_slice(payload);
            frame.push(CI_V_FRAME_END);
            frame
        }

        fn ack(radio_address: u8, controller_address: u8) -> Vec<u8> {
            Self::response(radio_address, controller_address, &[0xFB])
        }

        fn with_reads(reads: Vec<Vec<u8>>) -> (Self, Arc<Mutex<Vec<Vec<u8>>>>) {
            let writes = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    reads: reads.into_iter().collect(),
                    writes: Arc::clone(&writes),
                },
                writes,
            )
        }
    }

    impl Read for TestTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let Some(mut frame) = self.reads.pop_front() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "test transport has no scripted response",
                ));
            };
            let bytes = frame.len().min(buffer.len());
            buffer[..bytes].copy_from_slice(&frame[..bytes]);
            if bytes < frame.len() {
                frame.drain(..bytes);
                self.reads.push_front(frame);
            }
            Ok(bytes)
        }
    }

    impl Write for TestTransport {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.writes.lock().unwrap().push(bytes.to_vec());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl RadioTransport for TestTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

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
    fn candidate_probe_is_explicit_and_rejects_external_transports() {
        let radio =
            IcomCiVRadio::with_transport(None, 0xE0, 0x94, TestTransport::with_reads(Vec::new()).0);
        let error = radio
            .probe_candidates(&[115_200], &[0x94])
            .expect_err("external transports cannot change baud candidates");
        assert!(error.to_string().contains("native serial port"));
    }

    #[test]
    fn adaptive_timeout_expands_after_misses_and_recovers_after_success() {
        let radio = IcomCiVRadio::new_generic("", 115_200, 0xE0, 0x94);
        assert_eq!(radio.response_timeout(false), Duration::from_millis(1200));
        radio
            .transport_state
            .lock()
            .unwrap()
            .metrics
            .consecutive_timeouts = 1;
        assert_eq!(radio.response_timeout(false), Duration::from_millis(2400));
        radio
            .transport_state
            .lock()
            .unwrap()
            .metrics
            .consecutive_timeouts = 0;
        assert_eq!(radio.response_timeout(false), Duration::from_millis(1200));
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
    fn generic_and_profiled_controls_have_distinct_capability_surfaces() {
        let generic = IcomCiVRadio::new_generic("", 115_200, 0xE0, 0x94);
        assert!(!generic.supports_control(ControlId::RfPower));
        assert!(generic.supports_meter(MeterId::Signal));

        for model in [
            crate::models::IcomCivModel::Ic705,
            crate::models::IcomCivModel::Ic7300,
            crate::models::IcomCivModel::Ic7610,
            crate::models::IcomCivModel::Ic9700,
        ] {
            let radio = IcomCiVRadio::new_for_model(model, "", 115_200, 0xE0, 0x94);
            assert!(radio.supports_control(ControlId::RfPower), "{model:?}");
            assert!(radio.supports_control(ControlId::Rit), "{model:?}");
            assert!(radio.supports_meter(MeterId::Signal), "{model:?}");
            assert!(radio.supports_control_read(ControlId::RfPower));
            assert!(radio.supports_control_write(ControlId::RfPower));
            assert!(!radio.supports_control_read(ControlId::RawCiV));
        }

        assert!(!IcomCiVRadio::new_for_model(
            crate::models::IcomCivModel::Ic705,
            "",
            115_200,
            0xE0,
            0x94,
        )
        .supports_control(ControlId::ExternalPreamp));
        assert!(IcomCiVRadio::new_for_model(
            crate::models::IcomCivModel::Ic9700,
            "",
            115_200,
            0xE0,
            0xA2,
        )
        .supports_control(ControlId::ExternalPreamp));
    }

    #[test]
    fn radio_trait_capability_delegates_use_the_selected_profile() {
        let radio = IcomCiVRadio::new_for_model(
            crate::models::IcomCivModel::Ic7300,
            "",
            115_200,
            0xE0,
            0x94,
        );
        assert_eq!(radio.model(), Some(crate::models::IcomCivModel::Ic7300));
        assert_eq!(radio.controller_address(), 0xE0);
        assert_eq!(radio.radio_address(), 0x94);
        assert!(radio.event_router().is_some());
        assert!(!radio.supports_scope() || radio.scope_metadata().is_some());
        assert!(!radio.supports_iq_output());
        assert!(radio.filter_bandwidth_hz(Mode::Usb, 0).is_none());
        assert!(radio.swr_sweep_setup().is_some());
        assert!(radio.meter_presentation(MeterId::Swr, 128).is_some());
        assert!(radio.meter_poll_spec(MeterId::Signal).is_some());
        assert!(radio.meter_metadata(MeterId::Signal).is_none());
        assert!(radio.control_max(ControlId::Agc).is_some());
        assert!(radio.supported_control_values(ControlId::Filter).is_some());
        assert!(radio.supports_control(ControlId::RfPower));
        assert!(radio.supports_control_read(ControlId::RfPower));
        assert!(radio.supports_control_write(ControlId::RfPower));
        assert!(radio.supports_memory_channels());
        assert!(radio.supports_repeater_settings());
        assert!(radio.capabilities().can_get_frequency);
        assert_eq!(radio.link_health().commands_started, Some(0));
        assert!(radio.event_stream_age().is_none());
        assert_eq!(radio.scope_stream_counters(), (0, 0, 0));
        assert!(!radio
            .scope_stream_health()
            .is_stalled(Duration::from_secs(1)));
    }

    #[test]
    fn scope_assembler_rejects_bad_geometry_and_incomplete_sweeps() {
        let geometry = crate::models::IcomScopeGeometry {
            divisions: 2,
            full_chunk_bins: 2,
            last_chunk_bins: 1,
            bins: 3,
            bin_max: 255,
            supports_main_sub_scope: false,
        };
        let make_frame = |division: u8, bins: &[u8]| {
            let mut frame = TestTransport::response(0x94, 0xE0, &[0x27, 0x00, 0, division, 2]);
            frame.extend_from_slice(bins);
            frame.push(CI_V_FRAME_END);
            frame
        };
        let mut assembler = ScopeSweepAssembler::default();
        assert!(assembler
            .push(&make_frame(1, &[1, 2]), Some(geometry))
            .is_none());
        assert!(assembler
            .push(&make_frame(1, &[1, 2]), Some(geometry))
            .is_none());
        assert_eq!(assembler.dropped_sweeps, 1);
        assert!(assembler
            .push(&make_frame(3, &[1]), Some(geometry))
            .is_none());
        assert!(assembler
            .push(&make_frame(2, &[1, 2]), Some(geometry))
            .is_none());
        assert!(assembler.push(&make_frame(2, &[1]), None).is_none());
    }

    #[test]
    fn serial_port_labels_cover_usb_identity_variants() {
        let known = SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 0x0C26,
            pid: 0x0001,
            serial_number: None,
            manufacturer: Some("Vendor".to_string()),
            product: Some("7300 USB Serial".to_string()),
        });
        let (label, model) = describe_port_type(&known);
        assert!(label.contains("Vendor 7300 USB Serial"));
        assert_eq!(model.as_deref(), Some("Icom IC-7300 (CI-V)"));

        let manufacturer_only = SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 1,
            pid: 2,
            serial_number: None,
            manufacturer: Some("Vendor".to_string()),
            product: None,
        });
        assert!(describe_port_type(&manufacturer_only).0.contains("Vendor"));
        assert_eq!(
            describe_port_type(&SerialPortType::PciPort).0,
            "serial device"
        );
    }

    #[test]
    fn profiled_set_controls_emit_exact_model_and_common_ci_v_frames() {
        let (transport, writes) = TestTransport::with_reads(vec![TestTransport::ack(0x94, 0xE0)]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        futures::executor::block_on(radio.set_control(ControlId::RfPower, ControlValue::U8(50)))
            .unwrap();
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            [vec![0xFE, 0xFE, 0x94, 0xE0, 0x14, 0x0A, 0x00, 0x50, 0xFD]]
        );

        let (transport, writes) = TestTransport::with_reads(vec![TestTransport::ack(0x94, 0xE0)]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        futures::executor::block_on(radio.set_control(ControlId::Xit, ControlValue::Bool(true)))
            .unwrap();
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            [vec![0xFE, 0xFE, 0x94, 0xE0, 0x21, 0x02, 0x01, 0xFD]]
        );

        let (transport, writes) = TestTransport::with_reads(vec![TestTransport::ack(0x98, 0xE0)]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7610),
            0xE0,
            0x98,
            transport,
        );
        futures::executor::block_on(radio.set_control(ControlId::Antenna, ControlValue::U8(1)))
            .unwrap();
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            [vec![0xFE, 0xFE, 0x98, 0xE0, 0x12, 0x01, 0xFD]]
        );

        let (transport, writes) = TestTransport::with_reads(vec![TestTransport::ack(0xA2, 0xE0)]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic9700),
            0xE0,
            0xA2,
            transport,
        );
        futures::executor::block_on(radio.set_control(ControlId::Agc, ControlValue::U8(2)))
            .unwrap();
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            [vec![0xFE, 0xFE, 0xA2, 0xE0, 0x16, 0x12, 0x02, 0xFD]]
        );
    }

    #[test]
    fn profiled_get_controls_and_meters_decode_exact_ci_v_payloads() {
        let (transport, writes) = TestTransport::with_reads(vec![
            TestTransport::response(0x94, 0xE0, &[0x14, 0x0A, 0x00, 0x50]),
            TestTransport::response(0x94, 0xE0, &[0x15, 0x02, 0x00, 0x48]),
        ]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        let power = futures::executor::block_on(radio.get_control(ControlId::RfPower))
            .unwrap()
            .unwrap();
        assert_eq!(power, ControlValue::U8(50));
        assert_eq!(
            futures::executor::block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(48)
        );
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            [
                vec![0xFE, 0xFE, 0x94, 0xE0, 0x14, 0x0A, 0xFD],
                vec![0xFE, 0xFE, 0x94, 0xE0, 0x15, 0x02, 0xFD],
            ]
        );
    }

    #[test]
    fn ignores_usb_echo_back_before_civ_data_response() {
        let echo = TestTransport::response(0xE0, 0x94, &[0x15, 0x02]);
        let response = TestTransport::response(0x94, 0xE0, &[0x15, 0x02, 0x00, 0x48]);
        let (transport, writes) = TestTransport::with_reads(vec![echo, response]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );

        assert_eq!(
            futures::executor::block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(48)
        );
        assert_eq!(
            writes.lock().unwrap().as_slice(),
            [vec![0xFE, 0xFE, 0x94, 0xE0, 0x15, 0x02, 0xFD]]
        );
    }

    #[test]
    fn preserves_civ_nak_after_usb_echo_back() {
        let echo = TestTransport::response(0xE0, 0x94, &[0x14, 0x0A, 0x00, 0x50]);
        let nak = TestTransport::response(0x94, 0xE0, &[0xFA, 0x06]);
        let (transport, _) = TestTransport::with_reads(vec![echo, nak]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );

        let error = futures::executor::block_on(
            radio.set_control(ControlId::RfPower, ControlValue::U8(50)),
        )
        .expect_err("CI-V NAK must not be hidden by echo filtering");
        assert!(error.to_string().contains("radio rejected CI-V command"));
    }

    #[test]
    fn retains_unmatched_radio_frames_for_the_next_transaction() {
        let mode = TestTransport::response(0x94, 0xE0, &[0x04, 0x01]);
        let frequency = TestTransport::response(0x94, 0xE0, &[0x03, 0x00, 0x40, 0x07, 0x14, 0x00]);
        let (transport, writes) = TestTransport::with_reads(vec![{
            let mut combined = mode;
            combined.extend_from_slice(&frequency);
            combined
        }]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_074_000
        );
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Usb
        );
        assert_eq!(writes.lock().unwrap().len(), 2);
        let metrics = radio.transport_metrics();
        assert_eq!(metrics.frames_retained, 1);
        assert_eq!(metrics.responses_matched, 2);
    }

    #[test]
    fn model_value_boundaries_reject_before_transport() {
        for (model, attenuator, invalid_attenuator, max_preamp) in [
            (crate::models::IcomCivModel::Ic705, 20, 10, 2_u8),
            (crate::models::IcomCivModel::Ic7200, 20, 10, 1_u8),
            (crate::models::IcomCivModel::Ic7300, 20, 10, 1_u8),
            (crate::models::IcomCivModel::Ic7610, 45, 5, 2_u8),
            (crate::models::IcomCivModel::Ic9700, 10, 20, 1_u8),
        ] {
            let (transport, writes) = TestTransport::with_reads(Vec::new());
            let radio = IcomCiVRadio::with_transport(
                Some(model),
                0xE0,
                match model {
                    crate::models::IcomCivModel::Ic705 => 0xA4,
                    crate::models::IcomCivModel::Ic718 => 0x5E,
                    crate::models::IcomCivModel::Ic7200 => 0x76,
                    crate::models::IcomCivModel::Ic7300 => 0x94,
                    crate::models::IcomCivModel::Ic7610 => 0x98,
                    crate::models::IcomCivModel::Ic9700 => 0xA2,
                    crate::models::IcomCivModel::Generic => unreachable!(),
                },
                transport,
            );
            assert!(futures::executor::block_on(
                radio.set_control(ControlId::Attenuator, ControlValue::U8(invalid_attenuator),)
            )
            .is_err());
            assert!(futures::executor::block_on(radio.set_control(
                ControlId::Preamp,
                ControlValue::U8(max_preamp.saturating_add(1)),
            ))
            .is_err());
            assert!(futures::executor::block_on(
                radio.set_control(ControlId::Filter, ControlValue::U8(4),)
            )
            .is_err());
            assert!(
                writes.lock().unwrap().is_empty(),
                "{model:?} sent invalid CAT"
            );

            let (transport, writes) = TestTransport::with_reads(vec![TestTransport::ack(
                match model {
                    crate::models::IcomCivModel::Ic705 => 0xA4,
                    crate::models::IcomCivModel::Ic718 => 0x5E,
                    crate::models::IcomCivModel::Ic7200 => 0x76,
                    crate::models::IcomCivModel::Ic7300 => 0x94,
                    crate::models::IcomCivModel::Ic7610 => 0x98,
                    crate::models::IcomCivModel::Ic9700 => 0xA2,
                    crate::models::IcomCivModel::Generic => unreachable!(),
                },
                0xE0,
            )]);
            let radio = IcomCiVRadio::with_transport(
                Some(model),
                0xE0,
                match model {
                    crate::models::IcomCivModel::Ic705 => 0xA4,
                    crate::models::IcomCivModel::Ic718 => 0x5E,
                    crate::models::IcomCivModel::Ic7200 => 0x76,
                    crate::models::IcomCivModel::Ic7300 => 0x94,
                    crate::models::IcomCivModel::Ic7610 => 0x98,
                    crate::models::IcomCivModel::Ic9700 => 0xA2,
                    crate::models::IcomCivModel::Generic => unreachable!(),
                },
                transport,
            );
            futures::executor::block_on(
                radio.set_control(ControlId::Attenuator, ControlValue::U8(attenuator)),
            )
            .unwrap();
            assert_eq!(writes.lock().unwrap().len(), 1, "{model:?}");
        }
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
    fn ic7300_meter_queries_match_manual_command_table() {
        assert_eq!(meter_command_prefix(MeterId::Signal), &[0x15, 0x02]);
        assert_eq!(meter_command_prefix(MeterId::Power), &[0x15, 0x11]);
        assert_eq!(meter_command_prefix(MeterId::Swr), &[0x15, 0x12]);
        assert_eq!(meter_command_prefix(MeterId::Alc), &[0x15, 0x13]);
        assert_eq!(meter_command_prefix(MeterId::Compression), &[0x15, 0x14]);
        assert_eq!(meter_command_prefix(MeterId::Voltage), &[0x15, 0x15]);
        assert_eq!(meter_command_prefix(MeterId::Current), &[0x15, 0x16]);
        assert_eq!(meter_command_prefix(MeterId::Temperature), &[0x15, 0x17]);
    }

    #[test]
    fn decodes_documented_icom_memory_record() {
        let mut payload = vec![0x1A, 0x00, 0x01, 0x00, 0x00];
        payload.extend_from_slice(&encode_civ_frequency_bcd(14_074_000));
        payload.extend_from_slice(&[0x01, 0x01, 0x02]);
        payload.extend_from_slice(&encode_tone_frequency(885));
        payload.extend_from_slice(&encode_tone_frequency(1000));
        payload.extend_from_slice(b"FT8 USB   ");
        payload.push(0xFD);
        let mut frame = vec![0xFE, 0xFE, 0xE0, 0x94];
        frame.extend_from_slice(&payload);
        let memory = decode_icom_memory(&frame, &[0x1A, 0x00], MemoryLayout::Hf).unwrap();
        assert_eq!(memory.channel, 1);
        assert_eq!(memory.frequency_hz, 14_074_000);
        assert_eq!(memory.mode, Mode::Data);
        assert_eq!(memory.name.as_deref(), Some("FT8 USB"));
        assert_eq!(memory.repeater.tone.mode, ToneMode::Encode);
        assert_eq!(memory.repeater.tone.frequency_tenths_hz, Some(885));
    }

    #[test]
    fn icom_tone_commands_use_documented_flags_and_frequencies() {
        assert_eq!(encode_tone_frequency(885), [0x85, 0x08, 0x00]);
        assert_eq!(encode_tone_frequency(1000), [0x00, 0x10, 0x00]);
    }

    #[test]
    fn icom_rit_offset_uses_signed_four_byte_bcd() {
        assert_eq!(encode_civ_bcd_fixed(0, 4).unwrap(), vec![0, 0, 0, 0]);
        assert_eq!(
            encode_civ_bcd_fixed(1250, 4).unwrap(),
            vec![0x50, 0x12, 0, 0]
        );
        assert_eq!(
            encode_civ_bcd_fixed(9_999, 4).unwrap(),
            vec![0x99, 0x99, 0, 0]
        );
    }

    #[test]
    fn repeater_shift_mapping_matches_icom_ci_v_values() {
        assert_eq!(
            decode_repeater_shift(Some(0x10)).unwrap(),
            RepeaterShift::Simplex
        );
        assert_eq!(
            decode_repeater_shift(Some(0x11)).unwrap(),
            RepeaterShift::Minus
        );
        assert_eq!(
            decode_repeater_shift(Some(0x12)).unwrap(),
            RepeaterShift::Plus
        );
        assert_eq!(
            decode_repeater_shift(Some(0x00)).unwrap(),
            RepeaterShift::Simplex
        );
        assert_eq!(encode_repeater_shift(RepeaterShift::Simplex), 0x10);
        assert_eq!(encode_repeater_shift(RepeaterShift::Minus), 0x11);
        assert_eq!(encode_repeater_shift(RepeaterShift::Plus), 0x12);
        assert!(decode_repeater_shift(Some(0x13)).is_err());
    }

    #[test]
    fn decodes_icom_vhf_memory_duplex_dtcs_and_name_fields() {
        let mut data = vec![0u8; 68];
        data[2..4].copy_from_slice(&encode_memory_channel(12));
        data[5..10].copy_from_slice(&encode_civ_frequency_bcd(146_520_000));
        data[10] = 0x05; // FM
        data[13] = 0x23; // duplex plus, DTCS
        data[15..18].copy_from_slice(&encode_tone_frequency(885));
        data[21..24].copy_from_slice(&encode_civ_bcd_fixed(23, 3).unwrap());
        data[25..28].copy_from_slice(&encode_civ_bcd_fixed(600_000, 3).unwrap());
        data[52..68].copy_from_slice(b"LOCAL-REPEATER  ");
        let mut frame = vec![0xFE, 0xFE, 0xE0, 0xA4, 0x1A, 0x00];
        frame.extend_from_slice(&data);
        frame.push(0xFD);
        let memory = decode_icom_memory(&frame, &[0x1A, 0x00], MemoryLayout::VhfUhf).unwrap();
        assert_eq!(memory.channel, 12);
        assert_eq!(memory.frequency_hz, 146_520_000);
        assert_eq!(memory.transmit_frequency_hz, Some(147_120_000));
        assert_eq!(memory.repeater.tone.mode, ToneMode::Dtcs);
        assert_eq!(memory.repeater.tone.dtcs_code, Some(23));
        assert_eq!(memory.name.as_deref(), Some("LOCAL-REPEATER"));
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
    fn scope_stream_health_reports_sweep_cadence_and_stall() {
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
        let geometry = Some(crate::models::IcomScopeGeometry {
            divisions: 11,
            bins: 475,
            full_chunk_bins: 50,
            last_chunk_bins: 25,
            bin_max: 160,
            supports_main_sub_scope: false,
        });

        // Before any sweep, the stream is neither live nor stalled.
        let mut reader = ScopeStreamReader::default();
        let mut bytes = Vec::new();
        for division in 1..=11 {
            bytes.extend(frame(division, 40));
        }
        reader.push_bytes(&bytes, 0x94, 0xE0, geometry);
        assert_eq!(reader.completed_sweeps, 1);
        assert!(reader.last_sweep_at.is_some());

        // A just-completed sweep is fresh, not stalled.
        let age = reader.last_sweep_at.unwrap().elapsed();
        let health = ScopeStreamHealth {
            division_frames: reader.division_frames,
            completed_sweeps: reader.completed_sweeps,
            dropped_sweeps: reader.assembler.dropped_sweeps,
            last_sweep_age: Some(age),
        };
        assert!(!health.is_stalled(Duration::from_secs(5)));

        // A stale last-sweep timestamp with prior sweeps is stalled.
        let stalled = ScopeStreamHealth {
            completed_sweeps: 4,
            last_sweep_age: Some(Duration::from_secs(60)),
            ..ScopeStreamHealth::default()
        };
        assert!(stalled.is_stalled(Duration::from_secs(5)));

        // No sweeps ever means "never started", not "stalled".
        let never = ScopeStreamHealth::default();
        assert!(!never.is_stalled(Duration::from_secs(5)));
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

    #[test]
    fn exercises_common_command_paths_and_model_overrides() {
        let response = |payload: &[u8]| TestTransport::response(0x94, 0xE0, payload);
        let (transport, writes) = TestTransport::with_reads(vec![
            response(&[0x03, 0x00, 0x40, 0x07, 0x14, 0x00]),
            response(&[0x26, 0x00, 0x01, 0x01, 0x02]),
            TestTransport::ack(0x94, 0xE0),
            response(&[0x1C, 0x00, 0x01]),
            TestTransport::ack(0x94, 0xE0),
            response(&[0x1C, 0x01, 0x02]),
            response(&[0x26, 0x00, 0x01, 0x01, 0x03]),
            TestTransport::ack(0x94, 0xE0),
        ]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_074_000
        );
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Data
        );
        futures::executor::block_on(radio.set_ptt(true)).unwrap();
        assert!(futures::executor::block_on(radio.get_ptt()).unwrap());
        futures::executor::block_on(radio.set_control(ControlId::Tuner, ControlValue::Bool(true)))
            .unwrap();
        assert_eq!(
            futures::executor::block_on(radio.get_tuner_status()).unwrap(),
            Some(TunerStatus {
                enabled: true,
                tuning: true
            })
        );
        futures::executor::block_on(radio.set_mode(Mode::Usb)).unwrap();
        let writes = writes.lock().unwrap();
        assert!(writes.iter().any(|frame| frame.ends_with(&[0x03, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x1C, 0x00, 0x01, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x1C, 0x01, 0x01, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x26, 0x00, 0x01, 0x00, 0x03, 0xFD])));
    }

    #[test]
    fn exercises_vfo_receiver_external_preamp_rit_and_tuner_commands() {
        let response = |payload: &[u8]| TestTransport::response(0xA2, 0xE0, payload);
        let (transport, writes) = TestTransport::with_reads(vec![
            TestTransport::ack(0xA2, 0xE0),
            TestTransport::ack(0xA2, 0xE0),
            response(&[0x16, 0x02, 0x03]),
            response(&[0x16, 0x02, 0x03]),
            TestTransport::ack(0xA2, 0xE0),
            response(&[0x21, 0x00, 0x50, 0x12, 0x00, 0x00, 0x01]),
            TestTransport::ack(0xA2, 0xE0),
            TestTransport::ack(0xA2, 0xE0),
            response(&[0x1C, 0x01, 0x00]),
        ]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic9700),
            0xE0,
            0xA2,
            transport,
        );
        futures::executor::block_on(radio.set_control(ControlId::Vfo, ControlValue::U8(1)))
            .unwrap();
        futures::executor::block_on(radio.select_receiver(IcomReceiver::Sub)).unwrap();
        assert_eq!(
            futures::executor::block_on(radio.get_control(ControlId::ExternalPreamp)).unwrap(),
            Some(ControlValue::Bool(true))
        );
        futures::executor::block_on(
            radio.set_control(ControlId::ExternalPreamp, ControlValue::Bool(false)),
        )
        .unwrap();
        assert_eq!(radio.get_rit_offset_hz().unwrap(), -1250);
        radio.set_rit_offset_hz(567).unwrap();
        radio.start_tuner_blocking().unwrap();
        assert!(!radio.get_tuner_status_blocking().unwrap().enabled);
        let writes = writes.lock().unwrap();
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x07, 0x01, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x07, 0xD1, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x16, 0x02, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x16, 0x02, 0x01, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x16, 0x02, 0x01, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x1C, 0x01, 0x02, 0xFD])));
    }

    #[test]
    fn exercises_repeater_memory_frequency_power_and_filter_io() {
        let response = |payload: &[u8]| TestTransport::response(0x94, 0xE0, payload);
        let ack = || TestTransport::ack(0x94, 0xE0);
        let mut reads = vec![
            response(&[0x16, 0x42, 0x01]),
            response(&[0x16, 0x43, 0x00]),
            response(&[0x1B, 0x00, 0x85, 0x08, 0x00]),
            response(&[0x0F, 0x12]),
            response(&[0x0C, 0x00, 0x00, 0x06, 0x00]),
        ];
        reads.extend((0..6).map(|_| ack()));
        reads.push(ack());
        reads.push(ack());
        reads.push(response(&[0x26, 0x00, 0x01, 0x00, 0x02]));
        reads.push(ack());
        reads.push(response(&[0x26, 0x00, 0x01, 0x00, 0x02]));
        reads.push(ack());
        let (transport, writes) = TestTransport::with_reads(reads);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        let repeater = radio.get_repeater_settings().unwrap();
        assert_eq!(repeater.shift, RepeaterShift::Plus);
        assert_eq!(repeater.tone.mode, ToneMode::Encode);
        assert_eq!(repeater.offset_hz, Some(60_000));
        radio
            .set_repeater_settings(RepeaterSettings {
                shift: RepeaterShift::Minus,
                offset_hz: Some(600_000),
                tone: ToneSettings {
                    mode: ToneMode::EncodeDecode,
                    frequency_tenths_hz: Some(885),
                    ..ToneSettings::default()
                },
            })
            .unwrap();
        futures::executor::block_on(radio.set_frequency_hz(14_074_000)).unwrap();
        futures::executor::block_on(radio.set_power(false)).unwrap();
        assert_eq!(
            futures::executor::block_on(radio.get_control(ControlId::Filter)).unwrap(),
            Some(ControlValue::U8(2))
        );
        futures::executor::block_on(radio.set_control(ControlId::Filter, ControlValue::U8(3)))
            .unwrap();
        let writes = writes.lock().unwrap();
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x0F, 0x11, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x18, 0x00, 0xFD])));
        assert!(writes
            .iter()
            .any(|frame| frame.ends_with(&[0x05, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD])));
    }

    #[test]
    fn rejects_nak_and_invalid_raw_frames_without_hiding_protocol_errors() {
        let (transport, _) =
            TestTransport::with_reads(vec![TestTransport::response(0x94, 0xE0, &[0xFA])]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        let error = futures::executor::block_on(radio.set_ptt(true)).unwrap_err();
        assert!(error.to_string().contains("rejected CI-V command"));
        let (transport, _) = TestTransport::with_reads(Vec::new());
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        assert!(
            futures::executor::block_on(radio.protocol_write_read(&[0xFE, 0x00, 0xFD])).is_err()
        );
        assert!(futures::executor::block_on(radio.get_power()).is_err());
    }

    #[test]
    fn exercises_scope_configuration_and_selection_validation() {
        let ack = || TestTransport::ack(0x94, 0xE0);
        let (transport, writes) = TestTransport::with_reads((0..32).map(|_| ack()).collect());
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        futures::executor::block_on(radio.set_scope_configuration(ScopeConfiguration {
            center_mode: Some(true),
            span_hz: Some(200_000),
            fixed_edge_number: Some(2),
            hold: Some(false),
            reference_level_tenths_db: Some(-55),
            sweep_speed: Some(2),
            vbw_wide: Some(true),
            fixed_edges_hz: Some((14_000_000, 14_200_000)),
            center_type: Some(ScopeCenterType::CarrierPoint),
            tx_display: Some(true),
            max_hold: Some(ScopeMaxHold::Continuous),
            marker_position: Some(ScopeMarkerPosition::CarrierPoint),
            averaging: Some(2),
            waveform_type: Some(ScopeWaveformType::FillAndLine),
            waterfall_display: Some(true),
            waterfall_size: Some(2),
            waterfall_peak_level: Some(4),
            marker_auto_hide: Some(false),
            waveform_color_current: Some(ScopeColor {
                red: 1,
                green: 2,
                blue: 3,
            }),
            waveform_color_line: Some(ScopeColor {
                red: 4,
                green: 5,
                blue: 6,
            }),
            waveform_color_max_hold: Some(ScopeColor {
                red: 7,
                green: 8,
                blue: 9,
            }),
            ..ScopeConfiguration::default()
        }))
        .unwrap();
        futures::executor::block_on(radio.select_vfo(IcomVfo::A)).unwrap();
        assert!(writes.lock().unwrap().len() == 22);

        let unsupported = IcomCiVRadio::new_for_model(
            crate::models::IcomCivModel::Ic9700,
            "",
            115_200,
            0xE0,
            0xA2,
        );
        assert!(
            futures::executor::block_on(unsupported.set_scope_configuration(ScopeConfiguration {
                span_hz: Some(0),
                ..ScopeConfiguration::default()
            }))
            .is_err()
        );
        assert!(
            futures::executor::block_on(radio.set_scope_configuration(ScopeConfiguration {
                fixed_edge_number: Some(5),
                ..ScopeConfiguration::default()
            }))
            .is_err()
        );
    }

    #[test]
    fn exercises_hf_memory_read_write_and_channel_selection() {
        let mut payload = vec![0x1A, 0x00, 0x07, 0x00, 0x00];
        payload.extend_from_slice(&encode_civ_frequency_bcd(14_074_000));
        payload.extend_from_slice(&[0x01, 0x01, 0x02]);
        payload.extend_from_slice(&encode_tone_frequency(885));
        payload.extend_from_slice(&encode_tone_frequency(1000));
        payload.extend_from_slice(b"FIELD TEST");
        let (transport, writes) = TestTransport::with_reads(vec![
            TestTransport::ack(0x94, 0xE0),
            TestTransport::ack(0x94, 0xE0),
            TestTransport::response(0x94, 0xE0, &payload),
            TestTransport::ack(0x94, 0xE0),
        ]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        radio.select_memory_channel(7).unwrap();
        let memory = radio.read_memory_channel(7).unwrap();
        assert_eq!(memory.frequency_hz, 14_074_000);
        assert_eq!(memory.name.as_deref(), Some("FIELD TEST"));
        radio
            .write_memory_channel(MemoryChannel {
                channel: 7,
                name: Some("FIELD TEST".to_string()),
                frequency_hz: 14_074_000,
                transmit_frequency_hz: None,
                mode: Mode::Data,
                repeater: RepeaterSettings {
                    tone: ToneSettings {
                        mode: ToneMode::Encode,
                        frequency_tenths_hz: Some(885),
                        ..ToneSettings::default()
                    },
                    ..RepeaterSettings::default()
                },
            })
            .unwrap();
        assert_eq!(writes.lock().unwrap().len(), 4);

        let (transport, _) = TestTransport::with_reads(Vec::new());
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        assert!(radio.select_memory_channel(0).is_err());
        assert!(radio
            .write_memory_channel(MemoryChannel {
                channel: 1,
                name: Some("é".to_string()),
                frequency_hz: 14_074_000,
                transmit_frequency_hz: None,
                mode: Mode::Usb,
                repeater: RepeaterSettings::default(),
            })
            .is_err());
    }

    #[test]
    fn exercises_native_scope_stream_lifecycle_over_transport() {
        let mut reads = vec![
            TestTransport::ack(0x94, 0xE0),
            TestTransport::ack(0x94, 0xE0),
        ];
        for division in 1..=11 {
            let mut payload = vec![0x27, 0x00, 0x00, decimal_to_bcd(division), 0x11];
            let count = if division == 1 {
                0
            } else if division == 11 {
                25
            } else {
                50
            };
            payload.extend(std::iter::repeat_n(42, count));
            reads.push(TestTransport::response(0x94, 0xE0, &payload));
        }
        reads.push(TestTransport::ack(0x94, 0xE0));
        let (transport, writes) = TestTransport::with_reads(reads);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        let bins =
            futures::executor::block_on(radio.enable_spectrum_stream(Duration::from_millis(50)))
                .unwrap();
        assert_eq!(bins.len(), 475);
        assert_eq!(radio.scope_stream_counters(), (11, 1, 0));
        futures::executor::block_on(radio.disable_spectrum_stream()).unwrap();
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .any(|frame| frame.ends_with(&[0x27, 0x10, 0x01, 0xFD])));
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .any(|frame| frame.ends_with(&[0x27, 0x11, 0x01, 0xFD])));
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .any(|frame| frame.ends_with(&[0x27, 0x11, 0x00, 0xFD])));
    }

    #[test]
    fn exercises_on_demand_scope_waveform_request_and_empty_drain() {
        let mut reads = Vec::new();
        for division in 1..=11 {
            let mut payload = vec![0x27, 0x00, 0x00, decimal_to_bcd(division), 0x11];
            let count = if division == 1 {
                0
            } else if division == 11 {
                25
            } else {
                50
            };
            payload.extend(std::iter::repeat_n(80, count));
            reads.push(TestTransport::response(0x94, 0xE0, &payload));
        }
        let (transport, writes) = TestTransport::with_reads(reads);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        let bins = futures::executor::block_on(radio.request_scope_waveform_bins()).unwrap();
        assert_eq!(bins.len(), 475);
        assert_eq!(
            futures::executor::block_on(
                radio.drain_scope_waveform_sweeps(Duration::from_millis(1))
            )
            .unwrap(),
            Vec::<Vec<u8>>::new()
        );
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .any(|frame| frame.ends_with(&[0x27, 0x00, 0xFD])));
    }

    #[test]
    fn exercises_probe_and_vhf_uhf_memory_encoding() {
        let (transport, writes) = TestTransport::with_reads(vec![
            TestTransport::response(0xA4, 0xE0, &[0x03, 0x00, 0x40, 0x07, 0x14, 0x00]),
            TestTransport::response(0xA4, 0xE0, &[0x04, 0x01]),
            TestTransport::ack(0xA4, 0xE0),
        ]);
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic705),
            0xE0,
            0xA4,
            transport,
        );
        let status = radio.probe().unwrap();
        assert_eq!(status.frequency_hz, Some(14_074_000));
        assert_eq!(status.mode, Some("USB".to_string()));
        radio
            .write_memory_channel(MemoryChannel {
                channel: 12,
                name: Some("VHF TEST".to_string()),
                frequency_hz: 145_500_000,
                transmit_frequency_hz: Some(145_500_000),
                mode: Mode::Fm,
                repeater: RepeaterSettings {
                    shift: RepeaterShift::Plus,
                    offset_hz: Some(600_000),
                    tone: ToneSettings {
                        mode: ToneMode::Dtcs,
                        dtcs_code: Some(125),
                        dtcs_reverse: Some(true),
                        ..ToneSettings::default()
                    },
                },
            })
            .unwrap();
        assert!(writes
            .lock()
            .unwrap()
            .iter()
            .any(|frame| frame.len() > 70 && frame[4..6] == [0x1A, 0x00]));

        let (transport, _) = TestTransport::with_reads(vec![
            TestTransport::response(0xA4, 0xE0, &[0x03, 0x00, 0x40, 0x07, 0x14, 0x00]),
            TestTransport::response(0xA4, 0xE0, &[0x04, 0x01]),
        ]);
        let stream_radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic705),
            0xE0,
            0xA4,
            transport,
        );
        assert_eq!(
            futures::executor::block_on(stream_radio.probe_stream_status())
                .unwrap()
                .mode,
            Some("USB".to_string())
        );
        assert_eq!(
            IcomCiVRadio::new_for_model_default_address(
                crate::models::IcomCivModel::Ic7300,
                "",
                115_200,
                0xE0,
            )
            .radio_address(),
            0x94
        );
    }

    #[test]
    fn exercises_ic7300_specific_scope_controls_and_validation() {
        let (transport, writes) =
            TestTransport::with_reads((0..8).map(|_| TestTransport::ack(0x94, 0xE0)).collect());
        let radio = IcomCiVRadio::with_transport(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            transport,
        );
        futures::executor::block_on(radio.set_scope_sweep_speed(2)).unwrap();
        futures::executor::block_on(radio.set_scope_hold(true)).unwrap();
        futures::executor::block_on(radio.set_scope_reference_level_tenths_db(-55)).unwrap();
        futures::executor::block_on(radio.set_scope_center_fixed_mode(true)).unwrap();
        futures::executor::block_on(radio.set_scope_fixed_edge_number(2)).unwrap();
        futures::executor::block_on(
            radio.set_scope_fixed_edge_frequencies(2, 14_000_000, 14_200_000),
        )
        .unwrap();
        futures::executor::block_on(radio.set_scope_span_hz(500_000)).unwrap();
        futures::executor::block_on(radio.set_scope_vbw_wide(true)).unwrap();
        assert_eq!(writes.lock().unwrap().len(), 8);
        assert!(futures::executor::block_on(radio.set_scope_sweep_speed(3)).is_err());
        assert!(futures::executor::block_on(radio.set_scope_fixed_edge_number(5)).is_err());
        assert!(futures::executor::block_on(radio.set_scope_reference_level_tenths_db(3)).is_err());
        assert!(
            futures::executor::block_on(radio.set_scope_fixed_edge_frequencies(1, 10, 9)).is_err()
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
