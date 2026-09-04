//! Model-neutral transport for modern Yaesu ASCII CAT radios.

use std::{
    collections::VecDeque,
    io::ErrorKind,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use serialport::{DataBits, FlowControl, Parity, StopBits};

use crate::{
    events::{RadioEvent, RadioEventRouter, RadioEventSubscription},
    hal::{Mode, Radio, RadioCapabilities},
    hal_types::{
        denormalize_meter_level, normalize_meter_level, ControlId, ControlValue, CoreState,
        MemoryChannel, MeterId, RepeaterSettings, RepeaterShift, SwrSweepSetup, ToneMode,
        ToneSettings, TunerStatus,
    },
    models::YaesuCatModel,
    protocol::ascii_cat,
    transport::{RadioTransport, SerialPortTransport},
};

use super::profile::{profile_for_model, YaesuCatProfile};

const RESPONSE_TIMEOUT: Duration = Duration::from_millis(1_200);
const MAX_FRAME_LEN: usize = 512;
const RM_RESPONSE_PAYLOAD_LEN: usize = 7;
const MAX_RETAINED_FRAMES: usize = 128;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct YaesuTransportMetrics {
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
pub struct YaesuSerialPolicy {
    pub hardware_flow_control: bool,
    pub dtr: Option<bool>,
    pub rts: Option<bool>,
    pub startup_settle: Duration,
}

impl Default for YaesuSerialPolicy {
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
struct TransportState {
    port: Option<Box<dyn RadioTransport>>,
    external: Option<Box<dyn RadioTransport>>,
    pending: Vec<u8>,
    retained: VecDeque<Vec<u8>>,
    metrics: YaesuTransportMetrics,
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
    hardware_flow_control: Arc<Mutex<bool>>,
    cat_rts_detected: Arc<Mutex<bool>>,
    serial_policy: Arc<Mutex<YaesuSerialPolicy>>,
    transport: Arc<Mutex<TransportState>>,
    event_router: RadioEventRouter,
}

impl std::fmt::Debug for YaesuCatRadio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("YaesuCatRadio")
            .field("model", &self.model)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .field(
                "hardware_flow_control",
                &self
                    .hardware_flow_control
                    .lock()
                    .map(|enabled| *enabled)
                    .unwrap_or(false),
            )
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
    /// hardware flow control. The normal constructor starts without a local
    /// flow-control assumption and auto-detects the FTDX10 CAT RTS setting.
    pub fn new_for_model_with_hardware_flow_control(
        model: YaesuCatModel,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Result<Self> {
        let radio = Self::new_for_model(model, port, baud_rate)?;
        *radio
            .hardware_flow_control
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT flow-control state lock poisoned"))? = true;
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
                retained: VecDeque::new(),
                metrics: YaesuTransportMetrics::default(),
            })),
            hardware_flow_control: Arc::new(Mutex::new(false)),
            cat_rts_detected: Arc::new(Mutex::new(false)),
            serial_policy: Arc::new(Mutex::new(YaesuSerialPolicy {
                hardware_flow_control: false,
                ..Default::default()
            })),
            event_router: RadioEventRouter::default(),
        }
    }

    fn new_internal(model: Option<YaesuCatModel>, port: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            model,
            port: port.into(),
            baud_rate,
            hardware_flow_control: Arc::new(Mutex::new(false)),
            cat_rts_detected: Arc::new(Mutex::new(false)),
            serial_policy: Arc::new(Mutex::new(YaesuSerialPolicy::default())),
            transport: Arc::new(Mutex::new(TransportState::default())),
            event_router: RadioEventRouter::default(),
        }
    }

    pub fn model(&self) -> Option<YaesuCatModel> {
        self.model
    }

    pub fn baud_rate(&self) -> u32 {
        self.baud_rate
    }

    pub fn transport_metrics(&self) -> YaesuTransportMetrics {
        self.transport
            .lock()
            .map(|state| state.metrics)
            .unwrap_or_default()
    }

    pub fn with_serial_policy(self, policy: YaesuSerialPolicy) -> Result<Self> {
        *self
            .serial_policy
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT serial policy lock poisoned"))? = policy;
        *self
            .hardware_flow_control
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT flow-control state lock poisoned"))? =
            policy.hardware_flow_control;
        Ok(self)
    }

    /// Enable Yaesu CAT auto-information and return a subscription for the
    /// decoded state events arriving on the persistent CAT connection.
    pub fn subscribe_events(&self) -> Result<RadioEventSubscription> {
        self.send_set("AI", "1")?;
        Ok(self.event_router.subscribe())
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

    /// Read the radio's core operating state in as few round trips as the
    /// protocol allows. Modern Yaesu models answer `IF;` with the VFO-A
    /// frequency and the operating mode in a single frame, so a full core
    /// refresh costs `IF;` + `TX;` instead of separate `FA;`/`MD;`/`TX;`
    /// reads. Falls back to the individual reads when `IF;` is unavailable
    /// or malformed, so callers always get a best-effort snapshot.
    pub fn read_core_state(&self) -> Result<CoreState> {
        let mut state = CoreState::default();
        match self.read_if_status() {
            Ok(Some((frequency_hz, mode))) => {
                state.frequency_hz = Some(frequency_hz);
                state.mode = Some(mode);
            }
            Ok(None) | Err(_) => {
                // `IF;` is unavailable or returned an unexpected shape; fall
                // back to the per-value reads so a partial answer never
                // leaves the caller with nothing.
                state.frequency_hz = self.frequency_hz_via_fa().ok();
                state.mode = self.mode_via_md().ok();
            }
        }
        state.ptt = self.ptt_via_tx().ok();
        if state.frequency_hz.is_none() && state.mode.is_none() && state.ptt.is_none() {
            bail!("Yaesu core state read returned no usable values");
        }
        Ok(state)
    }

    /// Parse the `IF;` information frame into (frequency_hz, mode). The frame
    /// payload is 25 characters: P1 memory (3), P2 VFO-A frequency (9), P3
    /// clarifier (5), P4 (1), P5 (1), P6 mode (1), P7 (1), P8 (1), P9 fixed
    /// `00` (2), P10 (1). Returns `Ok(None)` when the radio does not answer
    /// with a recognizable `IF` frame so the caller can fall back.
    fn read_if_status(&self) -> Result<Option<(u64, Mode)>> {
        let response = match self.query("IF", None, 25) {
            Ok(response) => response,
            Err(_) => return Ok(None),
        };
        let payload = match parse_payload(&response, "IF") {
            Ok(payload) => payload,
            Err(_) => return Ok(None),
        };
        if payload.len() != 25 {
            return Ok(None);
        }
        let frequency_hz: u64 = match payload[3..12].parse() {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let mode_code = payload.chars().nth(19).expect("length checked above");
        let mode = match self.profile() {
            Some(profile) => profile.decode_mode(mode_code).ok(),
            None => decode_common_mode(mode_code).ok(),
        };
        let Some(mode) = mode else {
            return Ok(None);
        };
        Ok(Some((frequency_hz, mode)))
    }

    fn frequency_hz_via_fa(&self) -> Result<u64> {
        let command = active_vfo_command(self.active_vfo_selector()?);
        parse_payload(&self.query(command, None, 9)?, command)?
            .parse()
            .context("invalid Yaesu FA response")
    }

    fn mode_via_md(&self) -> Result<Mode> {
        let selector = self.active_vfo_selector()?;
        let selector_text = selector.to_string();
        let response = self.query("MD", Some(&selector_text), 2)?;
        let payload = parse_payload(&response, "MD")?;
        let mut chars = payload.chars();
        if chars.next() != Some(char::from(b'0' + selector)) {
            bail!("unexpected Yaesu MD receiver selector: {payload}");
        }
        let code = chars.next().context("missing Yaesu MD mode code")?;
        match self.profile() {
            Some(profile) => profile.decode_mode(code),
            None => decode_common_mode(code),
        }
    }

    fn ptt_via_tx(&self) -> Result<bool> {
        match parse_payload(&self.query("TX", None, 1)?, "TX")? {
            "0" => Ok(false),
            "1" | "2" => Ok(true),
            value => bail!("invalid Yaesu TX response: {value}"),
        }
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

    /// Read the modern Yaesu documented repeater controls. `OS` returns a
    /// receiver selector followed by the direction; the offset itself is a
    /// memory/configuration value and is not reported by this live surface.
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
        let shift = decode_repeater_shift(parse_payload(&self.query("OS", None, 2)?, "OS")?)?;
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
            bail!("Yaesu tone index must be 0..=49");
        }
        anyhow::ensure!(
            settings.offset_hz.is_none() || settings.offset_hz == Some(0),
            "Yaesu live OS control selects shift direction; set non-zero offset through a memory record"
        );
        let tone_mode = match settings.tone.mode {
            ToneMode::Off => "0",
            ToneMode::EncodeDecode => "1",
            ToneMode::Encode => "2",
            ToneMode::Dtcs => anyhow::bail!("DTCS is not supported by this Yaesu profile"),
        };
        let shift = encode_repeater_shift(settings.shift);
        self.send_set("CN", &format!("{:03}", settings.tone.index))?;
        self.send_set("CT", tone_mode)?;
        self.send_set("OS", &format!("0{shift}"))
    }

    /// Select a documented modern Yaesu memory channel. Complete `MR`/`MT`
    /// record codecs remain gated to the hardware-validated FTDX10 layout.
    pub fn select_memory_channel(&self, channel: u16) -> Result<()> {
        if !self.selected_profile()?.supports_memory_channels || channel > 99 {
            bail!("memory channel selection is not documented for this Yaesu model");
        }
        self.send_set("MC", &format!("{channel:03}"))
    }

    pub fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        anyhow::ensure!(
            self.selected_profile()?.supports_memory_channels && (1..=99).contains(&channel),
            "memory records are not profiled for this Yaesu model"
        );
        let response = self.query("MR", Some(&format!("{channel:03}")), 0)?;
        decode_modern_yaesu_memory(parse_payload(&response, "MR")?, self.selected_profile()?)
    }

    pub fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        anyhow::ensure!(
            self.selected_profile()?.supports_memory_channels
                && (1..=99).contains(&channel.channel),
            "memory records are not profiled for this Yaesu model"
        );
        anyhow::ensure!(
            channel.frequency_hz <= 999_999_999,
            "Yaesu memory frequency exceeds CAT width"
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
            "Yaesu memory tag must be ASCII and at most 12 characters"
        );
        let params = format!(
            "{:03}{:09}+{:04}00{}0{}00{}0{}",
            channel.channel, channel.frequency_hz, offset, mode, tone, shift, name
        );
        self.send_set("MT", &params)
    }

    fn selected_profile(&self) -> Result<&'static YaesuCatProfile> {
        self.profile()
            .context("this operation requires a selected Yaesu model profile")
    }

    fn control_command(&self, id: ControlId) -> Result<&'static str> {
        self.selected_profile()?
            .control(id)
            .map(|spec| spec.command)
            .with_context(|| format!("{id:?} is not profiled for this Yaesu model"))
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

    /// Some models expose a CAT RTS (hardware flow control) setting through
    /// their `EX` menu. Read it once over CAT and adapt the serial adapter
    /// before issuing ordinary queries. The `EX` selector and its reply
    /// layout are model-specific (hierarchical `030310` on FTDX10 and
    /// `030313` on FTDX101,
    /// flat menu `033` on FT-991A), so the probe is driven by the profile's
    /// `cat_rts_menu`. Models with no CAT RTS menu (FT-710) skip the probe
    /// and retain their configured/default transport behavior.
    fn ensure_cat_rts_detected(&self, command: &str) -> Result<()> {
        if command.eq_ignore_ascii_case("EX") {
            return Ok(());
        }
        let Some(profile) = self.profile() else {
            return Ok(());
        };
        let Some(menu) = profile.cat_rts_menu else {
            return Ok(());
        };
        // The probe reply is the echoed `EX` selector plus a single value
        // digit; the selector length is model-specific (6 for the
        // hierarchical selectors, 3 for the flat FT-991A `033`).
        let probe_payload_len = menu.len() + 1;
        if self
            .cat_rts_detected
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT RTS state lock poisoned"))?
            .to_owned()
        {
            return Ok(());
        }

        let response = match self.query("EX", Some(menu), probe_payload_len) {
            Ok(response) => response,
            Err(first_error) => {
                // If CAT RTS is already enabled on the radio, a no-flow
                // control probe may never receive a response. Reopen with
                // RTS/CTS and retry the menu read. Some firmware and USB
                // bridge combinations do not answer this EX menu query,
                // even though ordinary CAT commands still work. In that
                // case the probe is advisory: keep the bounded retry, cache
                // detection as unavailable, and let the requested command
                // establish whether the CAT link is usable.
                self.enable_hardware_flow_control().with_context(|| {
                    format!("failed to apply Yaesu CAT RTS/CTS after initial probe: {first_error}")
                })?;
                match self.query("EX", Some(menu), probe_payload_len) {
                    Ok(response) => response,
                    Err(_) => {
                        *self
                            .cat_rts_detected
                            .lock()
                            .map_err(|_| anyhow!("Yaesu CAT RTS state lock poisoned"))? = true;
                        return Ok(());
                    }
                }
            }
        };
        let payload = parse_payload(&response, "EX")?;
        anyhow::ensure!(
            payload.starts_with(menu) && payload.len() == probe_payload_len,
            "unexpected Yaesu CAT RTS response payload: {payload}"
        );
        let enabled = match payload.as_bytes()[menu.len()] {
            b'0' => false,
            b'1' => true,
            value => bail!("unexpected Yaesu CAT RTS value: {}", char::from(value)),
        };
        let flow_control_enabled = self
            .hardware_flow_control
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT flow-control state lock poisoned"))?
            .to_owned();
        if enabled && !flow_control_enabled {
            self.enable_hardware_flow_control()?;
        }
        *self
            .cat_rts_detected
            .lock()
            .map_err(|_| anyhow!("Yaesu CAT RTS state lock poisoned"))? = true;
        Ok(())
    }

    fn enable_hardware_flow_control(&self) -> Result<()> {
        let result = self.with_transport(Duration::from_millis(150), |state| {
            active_transport(state)?
                .set_hardware_flow_control(true)
                .context("failed to enable Yaesu CAT RTS/CTS flow control")
        });
        if result.is_ok() {
            *self
                .hardware_flow_control
                .lock()
                .map_err(|_| anyhow!("Yaesu CAT flow-control state lock poisoned"))? = true;
        }
        result
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
        let started = Instant::now();
        let response_timeout = self.response_timeout();
        self.with_transport(response_timeout, |state| {
            state.metrics.commands_started = state.metrics.commands_started.saturating_add(1);
            active_transport(state)?
                .write_all(request)
                .context("failed to write Yaesu CAT command")?;
            active_transport(state)?
                .flush()
                .context("failed to flush Yaesu CAT command")?;

            if let Some(index) = state.retained.iter().position(|frame| matcher(frame)) {
                let frame = state.retained.remove(index).expect("retained frame index");
                state.metrics.responses_matched = state.metrics.responses_matched.saturating_add(1);
                state.metrics.total_response_time += started.elapsed();
                return Ok(frame);
            }

            let deadline = Instant::now() + response_timeout;
            let mut buffer = [0_u8; 256];
            while Instant::now() < deadline {
                for frame in take_complete_frames(&mut state.pending) {
                    state.metrics.frames_received = state.metrics.frames_received.saturating_add(1);
                    if frame == b"?;" {
                        bail!("Yaesu CAT rejected command {}", display_command(request));
                    }
                    if matcher(&frame) {
                        state.metrics.responses_matched =
                            state.metrics.responses_matched.saturating_add(1);
                        state.metrics.total_response_time += started.elapsed();
                        return Ok(frame);
                    }
                    self.publish_unsolicited(&frame);
                    if state.retained.len() >= MAX_RETAINED_FRAMES {
                        state.retained.pop_front();
                        state.metrics.frames_dropped =
                            state.metrics.frames_dropped.saturating_add(1);
                    }
                    state.retained.push_back(frame);
                    state.metrics.frames_retained = state.metrics.frames_retained.saturating_add(1);
                }

                match active_transport(state)?.read(&mut buffer) {
                    Ok(count) if count > 0 => {
                        state.metrics.bytes_read =
                            state.metrics.bytes_read.saturating_add(count as u64);
                        state.pending.extend_from_slice(&buffer[..count]);
                        if state.pending.len() > MAX_FRAME_LEN * 4 {
                            bail!("Yaesu CAT receive buffer exceeded safety limit");
                        }
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::TimedOut => {}
                    Err(error) => return Err(error).context("failed to read Yaesu CAT response"),
                }
                thread::sleep(Duration::from_millis(1));
            }
            state.metrics.response_timeouts = state.metrics.response_timeouts.saturating_add(1);
            state.metrics.consecutive_timeouts =
                state.metrics.consecutive_timeouts.saturating_add(1);
            bail!(
                "timed out waiting for Yaesu CAT response to {}",
                display_command(request)
            )
        })
    }

    fn response_timeout(&self) -> Duration {
        let consecutive = self
            .transport
            .lock()
            .map(|state| state.metrics.consecutive_timeouts)
            .unwrap_or(0);
        RESPONSE_TIMEOUT.mul_f32(1.0_f32 + consecutive.min(2) as f32)
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
            let policy = *self
                .serial_policy
                .lock()
                .map_err(|_| anyhow!("Yaesu CAT serial policy lock poisoned"))?;
            state.port = Some(Box::new(SerialPortTransport(
                serialport::new(&self.port, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::One)
                    .flow_control(if policy.hardware_flow_control {
                        FlowControl::Hardware
                    } else {
                        FlowControl::None
                    })
                    .timeout(timeout)
                    .open()
                    .with_context(|| format!("failed to open Yaesu CAT port {}", self.port))?,
            )));
            if let Some(enabled) = policy.dtr {
                active_transport(&mut state)?.set_dtr(enabled)?;
            }
            if let Some(enabled) = policy.rts {
                active_transport(&mut state)?.set_rts(enabled)?;
            }
            active_transport(&mut state)?.clear_input()?;
            thread::sleep(policy.startup_settle);
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

    fn publish_unsolicited(&self, frame: &[u8]) {
        let Ok(text) = std::str::from_utf8(frame) else {
            self.event_router.publish(RadioEvent::Raw {
                payload: frame.to_vec(),
            });
            return;
        };
        let Some(payload) = text.strip_suffix(';') else {
            return;
        };
        if let Some(value) = payload.strip_prefix("FA") {
            if let Ok(frequency_hz) = value.parse() {
                self.event_router
                    .publish(RadioEvent::FrequencyChanged { frequency_hz });
                return;
            }
        }
        if let Some(value) = payload.strip_prefix("TX") {
            if matches!(value, "0" | "1" | "2") {
                self.event_router.publish(RadioEvent::PttChanged {
                    enabled: value != "0",
                });
                return;
            }
        }
        if let Some(value) = payload.strip_prefix("MD") {
            if value.len() >= 2 && matches!(&value[..1], "0" | "1") {
                let code = value.chars().nth(1).expect("length checked above");
                if let Some(mode) = self
                    .profile()
                    .and_then(|profile| profile.decode_mode(code).ok())
                {
                    self.event_router.publish(RadioEvent::ModeChanged { mode });
                    return;
                }
            }
        }
        self.event_router.publish(RadioEvent::Raw {
            payload: frame.to_vec(),
        });
    }
}

#[async_trait]
impl Radio for YaesuCatRadio {
    fn filter_bandwidth_hz(&self, mode: Mode, filter: u8) -> Option<u32> {
        self.profile()
            .and_then(|profile| profile.filter_bandwidth_hz(mode, filter))
    }

    fn swr_sweep_setup(&self) -> Option<SwrSweepSetup> {
        self.profile().and_then(|profile| profile.swr_sweep_setup())
    }

    fn control_max(&self, id: ControlId) -> Option<u8> {
        self.profile().and_then(|profile| profile.control_max(id))
    }

    fn supported_control_values(&self, id: ControlId) -> Option<&'static [u8]> {
        self.profile()
            .and_then(|profile| profile.supported_control_values(id))
    }

    fn event_router(&self) -> Option<RadioEventRouter> {
        Some(self.event_router.clone())
    }

    async fn get_frequency_hz(&self) -> Result<u64> {
        self.frequency_hz_via_fa()
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
        let command = active_vfo_command(self.active_vfo_selector()?);
        self.send_set(command, &format!("{hz:09}"))
    }

    async fn get_mode(&self) -> Result<Mode> {
        // Modern Yaesu uses the selected VFO selector in MD reads and writes:
        // MD0; / MD1; select the mode for VFO-A / VFO-B respectively.
        self.mode_via_md()
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let code = match self.profile() {
            Some(profile) => profile.encode_mode(mode)?,
            None => encode_common_mode(mode)?,
        };
        let selector = self.active_vfo_selector()?;
        self.send_set("MD", &format!("{selector}{code}"))
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.send_set("TX", if enabled { "1" } else { "0" })
    }

    async fn get_ptt(&self) -> Result<bool> {
        self.ptt_via_tx()
    }

    async fn read_core_state(&self) -> Result<CoreState> {
        // Modern Yaesu collapses frequency+mode into the `IF;` frame, so a
        // full core refresh costs `IF;` + `TX;` instead of three round trips.
        YaesuCatRadio::read_core_state(self)
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
            ControlId::AfGain | ControlId::RfGain => {
                let command = self.control_command(id)?;
                Ok(Some(ControlValue::U8(self.get_yaesu_level(command, 255)?)))
            }
            ControlId::Squelch => Ok(Some(ControlValue::U8(normalize_percent(
                self.get_yaesu_level(self.control_command(id)?, 100)?,
            )))),
            ControlId::Preamp => Ok(Some(ControlValue::U8(
                self.get_yaesu_selector(self.control_command(id)?)?,
            ))),
            ControlId::Attenuator => Ok(Some(ControlValue::U8(
                self.get_yaesu_selector(self.control_command(id)?)?,
            ))),
            ControlId::NoiseBlanker => Ok(Some(ControlValue::Bool(
                self.get_yaesu_bool_selector(self.control_command(id)?)?,
            ))),
            ControlId::Notch => Ok(Some(ControlValue::Bool(
                self.get_yaesu_bool_selector(self.control_command(id)?)?,
            ))),
            ControlId::ManualNotch => Ok(Some(ControlValue::Bool(
                self.get_yaesu_manual_notch(self.control_command(id)?)?,
            ))),
            ControlId::Filter => Ok(Some(ControlValue::U8(
                self.get_yaesu_width(self.control_command(id)?)?,
            ))),
            ControlId::Rit | ControlId::Xit => Ok(Some(ControlValue::Bool(
                self.get_yaesu_bool(self.control_command(id)?)?,
            ))),
            ControlId::Vfo => {
                let command = self.control_command(id)?;
                let response = self.query(command, None, 1)?;
                let payload = parse_payload(&response, command)?;
                let value = payload
                    .parse::<u8>()
                    .context("invalid Yaesu VFO selector")?;
                anyhow::ensure!(value <= 1, "invalid Yaesu VFO selector: {value}");
                Ok(Some(ControlValue::Vfo(value)))
            }
            ControlId::Tuner => Ok(Some(ControlValue::Bool(
                self.get_yaesu_tuner_enabled(self.control_command(id)?)?,
            ))),
            ControlId::RfPower => {
                let watts = self.get_power_watts()?;
                Ok(Some(ControlValue::U8(self.watts_to_normalized(watts)?)))
            }
            ControlId::Split => Ok(Some(ControlValue::Bool(self.get_split()?))),
            ControlId::Agc => {
                let command = self.control_command(id)?;
                let response = self.query(command, None, 2)?;
                let payload = parse_payload(&response, command)?;
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
                let command = self.control_command(id)?;
                let response = self.query(command, None, 2)?;
                let payload = parse_payload(&response, command)?;
                match payload {
                    "00" => Ok(Some(ControlValue::Bool(false))),
                    "01" => Ok(Some(ControlValue::Bool(true))),
                    value => bail!("invalid Yaesu NR response: {value}"),
                }
            }
            ControlId::NoiseReductionLevel => {
                let command = self.control_command(id)?;
                let response = self.query(command, None, 3)?;
                let payload = parse_payload(&response, command)?;
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
            MeterId::Temperature => Ok(Some(self.get_yaesu_meter(9)?)),
        }
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        let command = || self.control_command(id);
        match (id, value) {
            (ControlId::AfGain, ControlValue::U8(value))
            | (ControlId::RfGain, ControlValue::U8(value)) => {
                self.set_yaesu_level(command()?, value)
            }
            (ControlId::Squelch, ControlValue::U8(value)) => {
                self.send_set(command()?, &format!("0{:03}", denormalize_percent(value)))
            }
            (ControlId::Preamp, ControlValue::U8(value)) if value <= 2 => {
                self.send_set(command()?, &format!("0{value}"))
            }
            (ControlId::Attenuator, ControlValue::U8(value)) if value <= 3 => {
                self.send_set(command()?, &format!("0{value}"))
            }
            (ControlId::NoiseBlanker, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "01" } else { "00" })
            }
            (ControlId::Notch, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "01" } else { "00" })
            }
            (ControlId::ManualNotch, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "00001" } else { "00000" })
            }
            (ControlId::Filter, ControlValue::U8(value)) if value <= 23 => {
                self.send_set(command()?, &format!("00{value:02}"))
            }
            (ControlId::Rit, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "1" } else { "0" })
            }
            (ControlId::Xit, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "1" } else { "0" })
            }
            (ControlId::Vfo, ControlValue::Vfo(value)) if value <= 1 => {
                self.send_set(command()?, &value.to_string())
            }
            (ControlId::Tuner, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "001" } else { "000" })
            }
            (ControlId::RfPower, ControlValue::U8(level)) => {
                self.set_power_watts(self.normalized_to_watts(level)?)
            }
            (ControlId::Split, ControlValue::Bool(enabled)) => self.set_split(enabled),
            (ControlId::Agc, ControlValue::U8(value)) if value <= 4 => {
                self.send_set(command()?, &format!("0{value}"))
            }
            (ControlId::NoiseReduction, ControlValue::Bool(enabled)) => {
                self.send_set(command()?, if enabled { "01" } else { "00" })
            }
            (ControlId::NoiseReductionLevel, ControlValue::U8(value))
                if (1..=15).contains(&value) =>
            {
                self.send_set(command()?, &format!("0{value:02}"))
            }
            (_, value) => bail!("unsupported Yaesu CAT control/value: {id:?} = {value:?}"),
        }
    }

    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        YaesuCatRadio::get_repeater_settings(self)
    }

    async fn get_rit_offset_hz(&self) -> Result<i32> {
        let response = self.query("CF", Some("001"), 8)?;
        let payload = parse_payload(&response, "CF")?;
        let sign = match payload.chars().nth(3) {
            Some('+') => 1,
            Some('-') => -1,
            value => bail!("invalid Yaesu clarifier sign: {value:?}"),
        };
        let value = payload
            .get(4..8)
            .context("invalid Yaesu clarifier offset")?
            .parse::<i32>()?;
        Ok(sign * value)
    }

    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        anyhow::ensure!(
            (-9999..=9999).contains(&offset_hz),
            "Yaesu RIT offset must be -9999..=9999 Hz"
        );
        let sign = if offset_hz < 0 { '-' } else { '+' };
        self.send_set("CF", &format!("001{sign}{:04}", offset_hz.unsigned_abs()))
    }

    async fn get_xit_offset_hz(&self) -> Result<i32> {
        self.get_rit_offset_hz().await
    }

    async fn set_xit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.set_rit_offset_hz(offset_hz).await
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
            .is_some_and(|profile| profile.supports_memory_channels)
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_meter(id))
    }

    fn meter_poll_spec(&self, id: MeterId) -> Option<crate::MeterPollSpec> {
        self.profile()
            .and_then(|profile| profile.meter_poll_spec(id))
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_control(id))
    }

    fn supports_control_read(&self, id: ControlId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_control_read(id))
    }

    fn supports_control_write(&self, id: ControlId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_control_write(id))
    }

    async fn start_tuner(&self) -> Result<()> {
        self.send_set("AC", "002")
    }

    async fn get_tuner_status(&self) -> Result<Option<TunerStatus>> {
        let response = self.query("AC", None, 3)?;
        let payload = parse_payload(&response, "AC")?;
        let state = match payload.chars().last() {
            Some('0') => TunerStatus {
                enabled: false,
                tuning: false,
            },
            Some('1') => TunerStatus {
                enabled: true,
                tuning: false,
            },
            Some('2') => TunerStatus {
                enabled: true,
                tuning: true,
            },
            value => bail!("invalid Yaesu tuner state: {value:?}"),
        };
        Ok(Some(state))
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
    fn active_vfo_selector(&self) -> Result<u8> {
        let response = self.query("VS", None, 1)?;
        let payload = parse_payload(&response, "VS")?;
        let value = payload
            .parse::<u8>()
            .context("invalid Yaesu VFO selector")?;
        anyhow::ensure!(value <= 1, "invalid Yaesu VFO selector: {value}");
        Ok(value)
    }

    fn get_yaesu_level(&self, command: &str, maximum: u8) -> Result<u8> {
        let response = self.query(command, Some("0"), 4)?;
        let payload = parse_payload(&response, command)?;
        let value = payload
            .strip_prefix('0')
            .context("invalid Yaesu level selector")?
            .parse::<u16>()?;
        anyhow::ensure!(value <= u16::from(maximum), "Yaesu level exceeds its range");
        Ok(value as u8)
    }

    fn set_yaesu_level(&self, command: &str, value: u8) -> Result<()> {
        self.send_set(command, &format!("0{value:03}"))
    }

    fn get_yaesu_selector(&self, command: &str) -> Result<u8> {
        let response = self.query(command, Some("0"), 2)?;
        let payload = parse_payload(&response, command)?;
        payload
            .chars()
            .last()
            .context("missing Yaesu selector")?
            .to_digit(10)
            .map(|value| value as u8)
            .context("invalid Yaesu selector")
    }

    fn get_yaesu_bool_selector(&self, command: &str) -> Result<bool> {
        Ok(self.get_yaesu_selector(command)? == 1)
    }

    fn get_yaesu_bool(&self, command: &str) -> Result<bool> {
        match parse_payload(&self.query(command, None, 1)?, command)? {
            "0" => Ok(false),
            "1" => Ok(true),
            value => bail!("invalid Yaesu {command} state: {value}"),
        }
    }

    fn get_yaesu_manual_notch(&self, command: &str) -> Result<bool> {
        let response = self.query(command, Some("00"), 5)?;
        let payload = parse_payload(&response, command)?;
        Ok(payload.get(2..5) == Some("001"))
    }

    fn get_yaesu_width(&self, command: &str) -> Result<u8> {
        let response = self.query(command, Some("0"), 4)?;
        let payload = parse_payload(&response, command)?;
        payload
            .get(2..4)
            .context("invalid Yaesu SH response")?
            .parse()
            .context("invalid Yaesu width index")
    }

    fn get_yaesu_tuner_enabled(&self, command: &str) -> Result<bool> {
        let response = self.query(command, None, 3)?;
        let payload = parse_payload(&response, command)?;
        match payload.chars().last() {
            Some('0') => Ok(false),
            Some('1' | '2') => Ok(true),
            value => bail!("invalid Yaesu tuner state: {value:?}"),
        }
    }

    fn get_yaesu_meter(&self, selector: u8) -> Result<u8> {
        let response = self.query("RM", Some(&selector.to_string()), RM_RESPONSE_PAYLOAD_LEN)?;
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
        normalize_meter_level(watts - minimum, maximum - minimum)
            .context("Yaesu RF power range cannot be normalized")
    }

    fn normalized_to_watts(&self, level: u8) -> Result<u16> {
        let (minimum, maximum) = self
            .selected_profile()?
            .power_range_watts
            .context("RF power control is not profiled for this Yaesu model")?;
        Ok(minimum
            + denormalize_meter_level(level, maximum - minimum)
                .context("Yaesu RF power range cannot be denormalized")?)
    }
}

fn active_vfo_command(selector: u8) -> &'static str {
    match selector {
        0 => "FA",
        1 => "FB",
        _ => unreachable!("active VFO selector is validated to 0 or 1"),
    }
}

fn parse_payload<'a>(frame: &'a [u8], command: &str) -> Result<&'a str> {
    let text = std::str::from_utf8(frame).context("Yaesu CAT response is not ASCII")?;
    text.strip_prefix(command)
        .and_then(|value| value.strip_suffix(';'))
        .context("unexpected Yaesu CAT response")
}

fn normalize_percent(value: u8) -> u8 {
    normalize_meter_level(u16::from(value), 100).expect("percent range is non-zero")
}

fn denormalize_percent(value: u8) -> u8 {
    denormalize_meter_level(value, 100).expect("percent range is non-zero") as u8
}

fn decode_repeater_shift(payload: &str) -> Result<RepeaterShift> {
    match payload.chars().last() {
        Some('0') => Ok(RepeaterShift::Simplex),
        Some('1') => Ok(RepeaterShift::Plus),
        Some('2') => Ok(RepeaterShift::Minus),
        Some(value) => bail!("invalid Yaesu OS repeater shift: {value}"),
        None => bail!("missing Yaesu OS repeater shift"),
    }
}

fn encode_repeater_shift(shift: RepeaterShift) -> &'static str {
    match shift {
        RepeaterShift::Simplex => "0",
        RepeaterShift::Plus => "1",
        RepeaterShift::Minus => "2",
    }
}

fn decode_modern_yaesu_memory(payload: &str, profile: &YaesuCatProfile) -> Result<MemoryChannel> {
    anyhow::ensure!(payload.len() >= 25, "Yaesu MR response is too short");
    let channel = payload[0..3]
        .parse::<u16>()
        .context("invalid Yaesu memory channel")?;
    let frequency_hz = payload[3..12]
        .parse::<u64>()
        .context("invalid Yaesu memory frequency")?;
    let sign = match &payload[12..13] {
        "+" => 1i32,
        "-" => -1i32,
        value => bail!("invalid Yaesu clarifier direction: {value}"),
    };
    let offset = payload[13..17]
        .parse::<i32>()
        .context("invalid Yaesu memory offset")?;
    let mode = profile.decode_mode(
        payload[19..20]
            .chars()
            .next()
            .context("missing Yaesu memory mode")?,
    )?;
    let tone = match &payload[21..22] {
        "0" => ToneMode::Off,
        "1" => ToneMode::EncodeDecode,
        "2" => ToneMode::Encode,
        value => bail!("invalid Yaesu memory tone mode: {value}"),
    };
    let shift = match &payload[24..25] {
        "0" => RepeaterShift::Simplex,
        "1" => RepeaterShift::Plus,
        "2" => RepeaterShift::Minus,
        value => bail!("invalid Yaesu memory shift: {value}"),
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
    use crate::yaesu::profile::{
        FT710_PROFILE, FT991A_PROFILE, FTDX101D_PROFILE, FTDX101MP_PROFILE, FTDX10_PROFILE,
    };
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    struct ScriptedTransport {
        input: Vec<u8>,
        output: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for ScriptedTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let frame_len = self
                .input
                .iter()
                .position(|byte| *byte == b';')
                .map_or(self.input.len(), |index| index + 1);
            let count = buffer.len().min(self.input.len()).min(frame_len);
            buffer[..count].copy_from_slice(&self.input[..count]);
            self.input.drain(..count);
            Ok(count)
        }
    }

    impl Write for ScriptedTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.output.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl crate::transport::RadioTransport for ScriptedTransport {
        fn set_timeout(&mut self, _timeout: std::time::Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FlowControlTransport {
        scripted: ScriptedTransport,
        flow_control: Arc<Mutex<Vec<bool>>>,
    }

    impl Read for FlowControlTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.scripted.read(buffer)
        }
    }

    impl Write for FlowControlTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.scripted.write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.scripted.flush()
        }
    }

    impl crate::transport::RadioTransport for FlowControlTransport {
        fn set_timeout(&mut self, timeout: std::time::Duration) -> std::io::Result<()> {
            self.scripted.set_timeout(timeout)
        }

        fn set_hardware_flow_control(&mut self, enabled: bool) -> std::io::Result<()> {
            self.flow_control.lock().unwrap().push(enabled);
            Ok(())
        }
    }

    struct UnansweredRtsProbeTransport {
        input: Vec<u8>,
        output: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for UnansweredRtsProbeTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.input.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "scripted timeout",
                ));
            }
            let count = buffer.len().min(self.input.len());
            buffer[..count].copy_from_slice(&self.input[..count]);
            self.input.drain(..count);
            Ok(count)
        }
    }

    impl Write for UnansweredRtsProbeTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.output.lock().unwrap().extend_from_slice(buffer);
            match buffer {
                b"VS;" => self.input.extend_from_slice(b"VS1;"),
                b"FB;" => self.input.extend_from_slice(b"FB014250000;"),
                b"FA;" => self.input.extend_from_slice(b"FA014250000;"),
                _ => {}
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl crate::transport::RadioTransport for UnansweredRtsProbeTransport {
        fn set_timeout(&mut self, _timeout: std::time::Duration) -> std::io::Result<()> {
            Ok(())
        }

        fn set_hardware_flow_control(&mut self, _enabled: bool) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct FailingTransport;

    impl Read for FailingTransport {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "radio disconnected",
            ))
        }
    }

    impl Write for FailingTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl crate::transport::RadioTransport for FailingTransport {
        fn set_timeout(&mut self, _timeout: std::time::Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

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
    fn vfo_selection_routes_frequency_and_mode_to_the_selected_vfo() {
        assert_eq!(active_vfo_command(0), "FA");
        assert_eq!(active_vfo_command(1), "FB");
        assert_eq!(parse_payload(b"MD02;", "MD").unwrap(), "02");
        assert_eq!(parse_payload(b"MD12;", "MD").unwrap(), "12");
    }

    #[test]
    fn selected_vfo_is_used_by_live_frequency_and_mode_operations() {
        let frequency_output = Arc::new(Mutex::new(Vec::new()));
        let frequency_radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            ScriptedTransport {
                input: b"EX0303101;VS1;FB014250000;".to_vec(),
                output: frequency_output.clone(),
            },
        );
        assert_eq!(
            futures::executor::block_on(frequency_radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*frequency_output.lock().unwrap(), b"EX030310;VS;FB;");

        let mode_output = Arc::new(Mutex::new(Vec::new()));
        let mode_radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            ScriptedTransport {
                input: b"EX0303101;VS1;MD1C;".to_vec(),
                output: mode_output.clone(),
            },
        );
        assert_eq!(
            futures::executor::block_on(mode_radio.get_mode()).unwrap(),
            Mode::Data
        );
        assert_eq!(&*mode_output.lock().unwrap(), b"EX030310;VS;MD1;");
    }

    #[test]
    fn ftdx10_rts_probe_observes_disabled_setting_without_enabling_flow_control() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let flow_control = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            FlowControlTransport {
                scripted: ScriptedTransport {
                    input: b"EX0303100;VS0;FA014250000;".to_vec(),
                    output,
                },
                flow_control: Arc::clone(&flow_control),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert!(flow_control.lock().unwrap().is_empty());
    }

    #[test]
    fn ftdx10_rts_probe_applies_enabled_setting_before_normal_cat() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let flow_control = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            FlowControlTransport {
                scripted: ScriptedTransport {
                    input: b"EX0303101;VS0;FA014250000;".to_vec(),
                    output,
                },
                flow_control: Arc::clone(&flow_control),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*flow_control.lock().unwrap(), &[true]);
    }

    #[test]
    fn ftdx10_rts_probe_retries_with_flow_control_after_rejected_probe() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let flow_control = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            FlowControlTransport {
                scripted: ScriptedTransport {
                    input: b"?;EX0303101;VS0;FA014250000;".to_vec(),
                    output,
                },
                flow_control: Arc::clone(&flow_control),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*flow_control.lock().unwrap(), &[true]);
    }

    #[test]
    fn ftdx10_unanswered_rts_probe_does_not_block_normal_cat_queries() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            UnansweredRtsProbeTransport {
                input: Vec::new(),
                output: Arc::clone(&output),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*output.lock().unwrap(), b"EX030310;EX030310;VS;FB;");
    }

    #[test]
    fn ft991a_rts_probe_uses_the_flat_ex033_selector_and_applies_flow_control() {
        // The FT-991A documents CAT RTS as flat menu 033 (answer `EX033<v>;`),
        // not the hierarchical selectors used by the FTDX10/FTDX101 family.
        let output = Arc::new(Mutex::new(Vec::new()));
        let flow_control = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ft991A),
            38_400,
            FlowControlTransport {
                scripted: ScriptedTransport {
                    input: b"EX0331;VS0;FA014250000;".to_vec(),
                    output: Arc::clone(&output),
                },
                flow_control: Arc::clone(&flow_control),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        // The probe issues the model-specific `EX033;`, then enables RTS/CTS
        // because the radio reports CAT RTS enabled.
        assert_eq!(&*output.lock().unwrap(), b"EX033;VS;FA;");
        assert_eq!(&*flow_control.lock().unwrap(), &[true]);
    }

    #[test]
    fn ft991a_rts_probe_observes_disabled_setting_without_enabling_flow_control() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let flow_control = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ft991A),
            38_400,
            FlowControlTransport {
                scripted: ScriptedTransport {
                    input: b"EX0330;VS0;FA014250000;".to_vec(),
                    output: Arc::clone(&output),
                },
                flow_control: Arc::clone(&flow_control),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*output.lock().unwrap(), b"EX033;VS;FA;");
        assert!(flow_control.lock().unwrap().is_empty());
    }

    #[test]
    fn ftdx101_rts_probe_uses_the_hierarchical_ex030313_selector() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let flow_control = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx101D),
            38_400,
            FlowControlTransport {
                scripted: ScriptedTransport {
                    input: b"EX0303131;VS0;FA014250000;".to_vec(),
                    output: Arc::clone(&output),
                },
                flow_control: Arc::clone(&flow_control),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*output.lock().unwrap(), b"EX030313;VS;FA;");
        assert_eq!(&*flow_control.lock().unwrap(), &[true]);
    }

    #[test]
    fn ft710_skips_the_cat_rts_probe_because_the_model_has_no_such_menu() {
        // The FT-710 documents no CAT RTS menu (RTS on its standard port is a
        // PTT source via RPTT SELECT), so ordinary queries must not be
        // preceded by any EX probe.
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ft710),
            38_400,
            ScriptedTransport {
                input: b"VS0;FA014250000;".to_vec(),
                output: Arc::clone(&output),
            },
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(&*output.lock().unwrap(), b"VS;FA;");
    }

    #[test]
    fn core_state_read_collapses_frequency_and_mode_into_one_if_frame() {
        // `IF;` returns VFO-A frequency and mode in one frame; PTT comes from
        // `TX;`. The whole core refresh costs two round trips instead of
        // three, and issues no `FA;`/`MD;` reads at all. Payload layout:
        // mem(3)=000, freq(9)=014250000, clar(5)=+0000, P4=0, P5=0, mode=C
        // (DATA-USB), P7=0, P8=0, P9=00, P10=0.
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ft991A),
            38_400,
            ScriptedTransport {
                input: b"EX0330;IF000014250000+000000C00000;TX0;".to_vec(),
                output: Arc::clone(&output),
            },
        );

        let state = YaesuCatRadio::read_core_state(&radio).unwrap();
        assert_eq!(state.frequency_hz, Some(14_250_000));
        assert_eq!(state.mode, Some(Mode::Data));
        assert_eq!(state.ptt, Some(false));
        assert_eq!(&*output.lock().unwrap(), b"EX033;IF;TX;");
    }

    #[test]
    fn core_state_read_falls_back_to_individual_reads_when_if_is_unanswered() {
        // When `IF;` produces no usable frame, the reader falls back to the
        // individual FA/MD/TX reads so a partial answer never yields nothing.
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            UnansweredRtsProbeTransport {
                input: Vec::new(),
                output: Arc::clone(&output),
            },
        );

        let state = YaesuCatRadio::read_core_state(&radio).unwrap();
        assert_eq!(state.frequency_hz, Some(14_250_000));
        // The fallback path drives the individual reads after `IF;` fails.
        // This scripted radio sits on VFO-B (`VS1`), so the frequency read is
        // `FB;` rather than `FA;`, followed by the mode read `MD1;`.
        let written = output.lock().unwrap().clone();
        assert!(
            written.windows(3).any(|w| w == b"FB;"),
            "expected FB frequency read: {written:?}"
        );
        assert!(
            written.windows(4).any(|w| w == b"MD1;"),
            "expected MD mode read: {written:?}"
        );
    }

    #[test]
    fn live_meter_queries_accept_the_documented_rm_reply_shape() {
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            ScriptedTransport {
                input: b"EX0303101;RM1128000;".to_vec(),
                output: Arc::new(Mutex::new(Vec::new())),
            },
        );
        assert_eq!(
            futures::executor::block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(128)
        );
    }

    #[test]
    fn ftdx101_exposes_the_documented_temperature_meter() {
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx101D),
            38_400,
            ScriptedTransport {
                input: b"RM9007000;".to_vec(),
                output: Arc::new(Mutex::new(Vec::new())),
            },
        );

        assert!(radio.supports_meter(MeterId::Temperature));
        assert_eq!(
            futures::executor::block_on(radio.get_meter(MeterId::Temperature)).unwrap(),
            Some(7)
        );
        for model in [
            YaesuCatModel::Ft710,
            YaesuCatModel::Ftdx10,
            YaesuCatModel::Ft991A,
        ] {
            assert!(!YaesuCatRadio::new_for_model(model, "test", 38_400)
                .unwrap()
                .supports_meter(MeterId::Temperature));
        }
    }

    #[test]
    fn core_control_and_meter_reads_decode_documented_replies() {
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            ScriptedTransport {
                input: b"EX0303101;AG0128;RG0255;SQ0100;PA02;RA03;NB01;BC01;BP00001;SH0012;RT1;XT0;VS1;AC001;PC100;ST2;GT04;NR01;RL007;RM1128000;RM3033000;RM4044000;RM5055000;RM6066000;RM7077000;RM8088000;".to_vec(),
                output: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let read = |id| futures::executor::block_on(radio.get_control(id)).unwrap();
        assert_eq!(read(ControlId::AfGain), Some(ControlValue::U8(128)));
        assert_eq!(read(ControlId::RfGain), Some(ControlValue::U8(255)));
        assert_eq!(read(ControlId::Squelch), Some(ControlValue::U8(255)));
        assert_eq!(read(ControlId::Preamp), Some(ControlValue::U8(2)));
        assert_eq!(read(ControlId::Attenuator), Some(ControlValue::U8(3)));
        assert_eq!(
            read(ControlId::NoiseBlanker),
            Some(ControlValue::Bool(true))
        );
        assert_eq!(read(ControlId::Notch), Some(ControlValue::Bool(true)));
        assert_eq!(read(ControlId::ManualNotch), Some(ControlValue::Bool(true)));
        assert_eq!(read(ControlId::Filter), Some(ControlValue::U8(12)));
        assert_eq!(read(ControlId::Rit), Some(ControlValue::Bool(true)));
        assert_eq!(read(ControlId::Xit), Some(ControlValue::Bool(false)));
        assert_eq!(read(ControlId::Vfo), Some(ControlValue::Vfo(1)));
        assert_eq!(read(ControlId::Tuner), Some(ControlValue::Bool(true)));
        assert_eq!(read(ControlId::RfPower), Some(ControlValue::U8(255)));
        assert_eq!(read(ControlId::Split), Some(ControlValue::Bool(true)));
        assert_eq!(read(ControlId::Agc), Some(ControlValue::U8(4)));
        assert_eq!(
            read(ControlId::NoiseReduction),
            Some(ControlValue::Bool(true))
        );
        assert_eq!(
            read(ControlId::NoiseReductionLevel),
            Some(ControlValue::U8(7))
        );

        for (id, expected) in [
            (MeterId::Signal, 128),
            (MeterId::Compression, 33),
            (MeterId::Alc, 44),
            (MeterId::Power, 55),
            (MeterId::Swr, 66),
            (MeterId::Current, 77),
            (MeterId::Voltage, 88),
        ] {
            assert_eq!(
                futures::executor::block_on(radio.get_meter(id)).unwrap(),
                Some(expected)
            );
        }
    }

    #[test]
    fn core_control_writes_match_documented_command_fields() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            ScriptedTransport {
                input: Vec::new(),
                output: output.clone(),
            },
        );
        let write = |id, value| {
            futures::executor::block_on(radio.set_control(id, value)).unwrap();
        };
        write(ControlId::AfGain, ControlValue::U8(128));
        write(ControlId::RfGain, ControlValue::U8(255));
        write(ControlId::Squelch, ControlValue::U8(255));
        write(ControlId::Preamp, ControlValue::U8(2));
        write(ControlId::Attenuator, ControlValue::U8(3));
        write(ControlId::NoiseBlanker, ControlValue::Bool(true));
        write(ControlId::Notch, ControlValue::Bool(true));
        write(ControlId::ManualNotch, ControlValue::Bool(true));
        write(ControlId::Filter, ControlValue::U8(12));
        write(ControlId::Rit, ControlValue::Bool(true));
        write(ControlId::Xit, ControlValue::Bool(true));
        write(ControlId::Vfo, ControlValue::Vfo(1));
        write(ControlId::Tuner, ControlValue::Bool(true));
        write(ControlId::RfPower, ControlValue::U8(255));
        write(ControlId::Split, ControlValue::Bool(true));
        write(ControlId::Agc, ControlValue::U8(4));
        write(ControlId::NoiseReduction, ControlValue::Bool(true));
        write(ControlId::NoiseReductionLevel, ControlValue::U8(7));
        assert_eq!(
            &*output.lock().unwrap(),
            b"AG0128;RG0255;SQ0100;PA02;RA03;NB01;BC01;BP00001;SH0012;RT1;XT1;VS1;AC001;PC100;ST1;GT04;NR01;RL007;"
        );

        assert!(futures::executor::block_on(
            radio.set_control(ControlId::Preamp, ControlValue::U8(3),)
        )
        .is_err());
        assert!(futures::executor::block_on(
            radio.set_control(ControlId::Attenuator, ControlValue::U8(4),)
        )
        .is_err());
        assert!(futures::executor::block_on(
            radio.set_control(ControlId::NoiseReductionLevel, ControlValue::U8(0),)
        )
        .is_err());
    }

    #[test]
    fn live_queries_skip_unsolicited_frames_and_keep_their_response() {
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            ScriptedTransport {
                input: b"EX0303101;FA014074000;VS1;FB014250000;".to_vec(),
                output: Arc::new(Mutex::new(Vec::new())),
            },
        );
        let subscription = radio.event_router.subscribe();
        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_250_000
        );
        assert_eq!(
            subscription.drain(),
            vec![RadioEvent::FrequencyChanged {
                frequency_hz: 14_074_000
            }]
        );
    }

    #[test]
    fn transport_failures_are_reported_and_pending_frames_are_cleared() {
        let radio = YaesuCatRadio::with_external_transport(
            Some(YaesuCatModel::Ftdx10),
            38_400,
            FailingTransport,
        );
        assert!(radio.query_raw("VS", None).is_err());
        assert!(radio.transport.lock().unwrap().pending.is_empty());
    }

    #[test]
    fn modern_mode_table_matches_manual_receiver_codes() {
        for profile in [
            &FT710_PROFILE,
            &FTDX10_PROFILE,
            &FTDX101D_PROFILE,
            &FTDX101MP_PROFILE,
        ] {
            for (code, mode) in [
                ('1', Mode::Lsb),
                ('2', Mode::Usb),
                ('3', Mode::Cw),
                ('4', Mode::Fm),
                ('5', Mode::Am),
                ('6', Mode::Rtty),
                ('7', Mode::CwReverse),
                ('9', Mode::RttyReverse),
                ('B', Mode::Fm),
                ('C', Mode::Data),
                ('D', Mode::Am),
                ('E', Mode::Data),
                ('F', Mode::Data),
            ] {
                assert_eq!(profile.decode_mode(code).unwrap(), mode);
            }
        }
        assert_eq!(FT991A_PROFILE.decode_mode('E').unwrap(), Mode::Data);
        assert!(FT991A_PROFILE.decode_mode('F').is_err());
    }

    #[test]
    fn manual_meter_selectors_are_preserved() {
        for selector in 1..=9 {
            let frame = format!("RM{selector:03};");
            assert_eq!(parse_payload(frame.as_bytes(), "RM").unwrap(), &frame[2..5]);
        }
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
    fn auto_information_frames_become_typed_events() {
        let radio = YaesuCatRadio::new_for_model(YaesuCatModel::Ftdx10, "test", 38_400).unwrap();
        let subscription = radio.event_router.subscribe();
        radio.publish_unsolicited(b"FA014074000;");
        radio.publish_unsolicited(b"MD02;");
        radio.publish_unsolicited(b"TX1;");
        assert_eq!(
            subscription.drain(),
            vec![
                RadioEvent::FrequencyChanged {
                    frequency_hz: 14_074_000
                },
                RadioEvent::ModeChanged { mode: Mode::Usb },
                RadioEvent::PttChanged { enabled: true },
            ]
        );
    }

    #[test]
    fn repeater_os_preserves_main_band_selector_and_decodes_direction() {
        assert_eq!(decode_repeater_shift("00").unwrap(), RepeaterShift::Simplex);
        assert_eq!(decode_repeater_shift("01").unwrap(), RepeaterShift::Plus);
        assert_eq!(decode_repeater_shift("12").unwrap(), RepeaterShift::Minus);
        assert_eq!(encode_repeater_shift(RepeaterShift::Plus), "1");
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
        let channel = decode_modern_yaesu_memory(&payload, &FTDX10_PROFILE).unwrap();
        assert_eq!(channel.channel, 1);
        assert_eq!(channel.frequency_hz, 14_074_000);
        assert_eq!(channel.mode, Mode::Usb);
        assert_eq!(channel.repeater.tone.mode, ToneMode::Encode);
        assert_eq!(channel.repeater.shift, RepeaterShift::Plus);
    }

    #[test]
    fn modern_profiles_decode_the_shared_memory_record_layout() {
        let payload = format!(
            "{:03}{:09}+{:04}00{}0{}00{}",
            1, 14_074_000, 0, '2', '2', '1'
        );
        for profile in [
            &FT710_PROFILE,
            &FTDX10_PROFILE,
            &FTDX101D_PROFILE,
            &FTDX101MP_PROFILE,
            &FT991A_PROFILE,
        ] {
            assert_eq!(
                decode_modern_yaesu_memory(&payload, profile)
                    .unwrap()
                    .channel,
                1
            );
        }
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
        assert_eq!(ftdx10.watts_to_normalized(53).unwrap(), 129);
        assert_eq!(ftdx10.normalized_to_watts(128).unwrap(), 53);

        let mp = YaesuCatRadio::new_for_model(YaesuCatModel::Ftdx101Mp, "test", 38_400).unwrap();
        assert_eq!(mp.normalized_to_watts(255).unwrap(), 200);
    }

    #[test]
    fn percent_controls_use_the_shared_hal_rounding_policy() {
        assert_eq!(normalize_percent(50), 128);
        assert_eq!(denormalize_percent(128), 50);
    }
}
