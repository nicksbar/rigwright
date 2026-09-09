//! Model-profiled driver for classic five-byte Yaesu CAT.

use std::{
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
        normalize_meter_level, ControlId, ControlValue, MeterId, MeterMetadata, MeterPollSpec,
        RepeaterSettings, ToneMode,
    },
    models::YaesuLegacyModel,
    protocol::yaesu_legacy_cat::{self, FrequencyModeStatus, LegacyMode, RxStatus, TxStatus},
    transport::{RadioTransport, SerialPortTransport},
};

use super::legacy_profile::{profile_for_model, LegacyCatDialect, YaesuLegacyProfile};

// RS918/mcHF-class FT-817 CAT emulators need time to apply a write before a
// following status query. This is also consistent with the command/mode delay
// used by established FT-817 client integrations.
const LEGACY_WRITE_SETTLE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LegacyYaesuTransportMetrics {
    pub commands_started: u64,
    pub responses_read: u64,
    pub response_errors: u64,
    pub bytes_read: u64,
    pub total_response_time: Duration,
}

/// Fixed CAT line settings required by the classic Yaesu binary protocol.
/// Exposed so clients can render the connection requirements from the driver
/// without duplicating vendor-specific serial knowledge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyYaesuSerialPolicy {
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub flow_control: FlowControl,
}

impl Default for LegacyYaesuSerialPolicy {
    fn default() -> Self {
        Self {
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::Two,
            flow_control: FlowControl::None,
        }
    }
}

#[derive(Default)]
struct TransportState {
    port: Option<Box<dyn RadioTransport>>,
    external: Option<Box<dyn RadioTransport>>,
    metrics: LegacyYaesuTransportMetrics,
}

fn active_transport(state: &mut TransportState) -> Result<&mut dyn RadioTransport> {
    if let Some(port) = state.port.as_mut() {
        Ok(&mut **port)
    } else if let Some(port) = state.external.as_mut() {
        Ok(&mut **port)
    } else {
        bail!("classic Yaesu CAT port unavailable")
    }
}

/// Classic Yaesu CAT driver for fixed five-byte binary commands.
///
/// The serial connection is always configured as the manuals require: 8 data
/// bits, no parity, two stop bits, and no flow control. Unlike modern Yaesu
/// ASCII CAT, commands have no terminator and ordinary set commands return no
/// acknowledgement.
#[derive(Clone)]
pub struct LegacyYaesuRadio {
    model: Option<YaesuLegacyModel>,
    port: String,
    baud_rate: u32,
    transport: Arc<Mutex<TransportState>>,
}

impl std::fmt::Debug for LegacyYaesuRadio {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LegacyYaesuRadio")
            .field("model", &self.model)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .finish_non_exhaustive()
    }
}

impl LegacyYaesuRadio {
    /// Construct a model-neutral classic CAT driver.
    pub fn new_generic(port: impl Into<String>, baud_rate: u32) -> Self {
        Self::new_internal(Some(YaesuLegacyModel::Generic), port, baud_rate)
    }

    /// Compatibility constructor. Prefer [`Self::new_for_model`] when the
    /// radio model is known.
    pub fn new(port: impl Into<String>, baud_rate: u32) -> Self {
        Self::new_generic(port, baud_rate)
    }

    pub fn new_for_model(
        model: YaesuLegacyModel,
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

    /// Construct a classic Yaesu CAT radio over an externally configured byte
    /// transport, such as Android USB Host or Bluetooth.
    pub fn with_transport<T>(model: Option<YaesuLegacyModel>, baud_rate: u32, transport: T) -> Self
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
                metrics: LegacyYaesuTransportMetrics::default(),
            })),
        }
    }

    fn new_internal(
        model: Option<YaesuLegacyModel>,
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

    pub fn model(&self) -> Option<YaesuLegacyModel> {
        self.model
    }

    pub fn profile(&self) -> Option<&'static YaesuLegacyProfile> {
        self.model.map(profile_for_model)
    }

    fn dialect(&self) -> LegacyCatDialect {
        self.profile()
            .map(|profile| profile.dialect)
            .unwrap_or(LegacyCatDialect::Classic)
    }

    pub fn transport_metrics(&self) -> LegacyYaesuTransportMetrics {
        self.transport
            .lock()
            .map(|state| state.metrics)
            .unwrap_or_default()
    }

    pub fn serial_policy(&self) -> LegacyYaesuSerialPolicy {
        LegacyYaesuSerialPolicy::default()
    }

    pub fn close(&self) {
        if let Ok(mut transport) = self.transport.lock() {
            transport.port = None;
            transport.external = None;
        }
    }

    pub fn get_rx_status(&self) -> Result<RxStatus> {
        yaesu_legacy_cat::decode_rx_status(&self.transact(yaesu_legacy_cat::read_rx_status(), 1)?)
    }

    pub fn get_frequency_mode_status(&self) -> Result<FrequencyModeStatus> {
        let dialect = self.dialect();
        let (request, response_len) = dialect.frequency_mode_request();
        let response = self.transact(request, response_len)?;
        dialect.decode_frequency_mode(&response).map_err(|error| {
            anyhow!(
                "{error}; {} status response bytes={response:02X?}",
                self.model
                    .map(|model| model.model_name())
                    .unwrap_or("classic Yaesu")
            )
        })
    }

    /// Set an exact classic CAT mode, including packet or narrow FM when the
    /// selected model profile documents it.
    pub fn set_legacy_mode(&self, mode: LegacyMode) -> Result<()> {
        if let Some(profile) = self.profile() {
            if !profile.supports_mode(mode) {
                bail!(
                    "{} does not document CAT mode {mode:?} for writing",
                    profile.model.model_name()
                );
            }
        }
        let frame = self.dialect().set_mode(mode);
        self.transact(frame, 0)?;
        Ok(())
    }

    pub fn get_tx_status(&self) -> Result<TxStatus> {
        let dialect = self.dialect();
        let (request, response_len) = dialect.tx_status_request();
        dialect.decode_tx_status(&self.transact(request, response_len)?)
    }

    pub fn get_split(&self) -> Result<bool> {
        Ok(self.get_tx_status()?.split_enabled)
    }

    pub fn set_split(&self, enabled: bool) -> Result<()> {
        let frame = self.dialect().set_split(enabled);
        self.transact(frame, 0)?;
        Ok(())
    }

    /// Toggle between the classic radio's VFO-A and VFO-B. The binary CAT
    /// protocol does not report which VFO is active, so this deliberately is
    /// not exposed as a selectable `ControlId::Vfo` value.
    pub fn toggle_vfo(&self) -> Result<()> {
        self.transact(yaesu_legacy_cat::toggle_vfo(), 0)?;
        Ok(())
    }

    /// Apply the radio's CAT lock. Classic CAT has no lock-state readback.
    pub fn set_cat_lock(&self, enabled: bool) -> Result<()> {
        self.transact(yaesu_legacy_cat::set_lock(enabled), 0)?;
        Ok(())
    }

    /// Execute one complete five-byte command and read its documented response
    /// length. This is the escape hatch for model commands not in the root HAL.
    pub fn transact_raw(&self, frame: [u8; 5], response_len: usize) -> Result<Vec<u8>> {
        if response_len > 2_048 {
            bail!("legacy Yaesu response length exceeds safety limit");
        }
        self.transact(frame, response_len)
    }

    fn transact(&self, frame: [u8; 5], response_len: usize) -> Result<Vec<u8>> {
        if self.port.trim().is_empty()
            && self
                .transport
                .lock()
                .map_err(|_| anyhow!("classic Yaesu CAT transport lock poisoned"))?
                .external
                .is_none()
        {
            bail!("a serial port is required for classic Yaesu CAT");
        }
        let mut transport = self
            .transport
            .lock()
            .map_err(|_| anyhow!("classic Yaesu CAT transport lock poisoned"))?;
        let started = Instant::now();
        transport.metrics.commands_started = transport.metrics.commands_started.saturating_add(1);
        if transport.port.is_none() && transport.external.is_none() {
            transport.port = Some(Box::new(SerialPortTransport(
                serialport::new(&self.port, self.baud_rate)
                    .data_bits(DataBits::Eight)
                    .parity(Parity::None)
                    .stop_bits(StopBits::Two)
                    .flow_control(FlowControl::None)
                    .timeout(Duration::from_millis(self.dialect().serial_timeout_ms()))
                    .open()
                    .with_context(|| format!("failed to open classic CAT port {}", self.port))?,
            )));
        }

        let result = (|| {
            let port = active_transport(&mut transport)?;
            // Some FT-817-compatible USB implementations echo or leave a
            // partial write response queued. Never let those bytes become the
            // prefix of a subsequent five-byte status frame.
            if response_len > 0 {
                port.clear_input()
                    .context("failed to clear stale classic Yaesu CAT input")?;
            }
            port.write_all(&frame)
                .context("failed to write classic Yaesu CAT command")?;
            port.flush()
                .context("failed to flush classic Yaesu CAT command")?;
            if response_len == 0 {
                thread::sleep(LEGACY_WRITE_SETTLE);
                port.clear_input()
                    .context("failed to clear classic Yaesu CAT write residue")?;
            }
            let mut response = vec![0_u8; response_len];
            if response_len > 0 {
                port.read_exact(&mut response)
                    .context("failed to read classic Yaesu CAT response")?;
            }
            transport.metrics.responses_read = transport.metrics.responses_read.saturating_add(1);
            transport.metrics.bytes_read = transport
                .metrics
                .bytes_read
                .saturating_add(response_len as u64);
            transport.metrics.total_response_time += started.elapsed();
            Ok(response)
        })();

        if result.is_err() {
            transport.metrics.response_errors = transport.metrics.response_errors.saturating_add(1);
            transport.port = None;
        }
        result
    }
}

#[async_trait]
impl Radio for LegacyYaesuRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        Ok(self.get_frequency_mode_status()?.frequency_hz)
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        if let Some(profile) = self.profile() {
            if !profile.supports_frequency(hz) {
                bail!(
                    "frequency {hz} Hz is outside the documented CAT range for {}",
                    profile.model.model_name()
                );
            }
        }
        let frame = self.dialect().set_frequency(hz)?;
        self.transact(frame, 0)?;
        Ok(())
    }

    async fn get_mode(&self) -> Result<Mode> {
        let mode = self.get_frequency_mode_status()?.mode;
        Ok(legacy_to_hal_mode(mode))
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let mode = hal_to_legacy_mode(mode)?;
        self.set_legacy_mode(mode)
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        let frame = self.dialect().set_ptt(enabled);
        self.transact(frame, 0)?;
        let actual = self.get_tx_status()?.transmitting;
        if actual != enabled {
            bail!(
                "classic Yaesu PTT command did not reach the requested state (requested {enabled}, reported {actual})"
            );
        }
        Ok(())
    }

    async fn get_ptt(&self) -> Result<bool> {
        Ok(self.get_tx_status()?.transmitting)
    }

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        let frame: [u8; 5] = request
            .try_into()
            .context("classic Yaesu CAT command must contain exactly five bytes")?;
        let response_len = self.dialect().response_length(frame);
        self.transact(frame, response_len)
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        match id {
            ControlId::Split => Ok(Some(ControlValue::Bool(self.get_split()?))),
            _ => Ok(None),
        }
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        if !self.supports_control_write(id) {
            bail!("classic Yaesu control {id:?} is not writable for this model");
        }
        match (id, value) {
            (ControlId::Split, ControlValue::Bool(enabled)) => self.set_split(enabled),
            (ControlId::Rit, ControlValue::Bool(enabled)) => {
                self.transact(yaesu_legacy_cat::set_clarifier(enabled), 0)?;
                Ok(())
            }
            (_, value) => bail!("unsupported classic Yaesu control/value: {id:?} = {value:?}"),
        }
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.profile()
            .map(|profile| profile.supports_control(id))
            .unwrap_or(matches!(id, ControlId::Split | ControlId::Rit))
    }

    fn supports_control_read(&self, id: ControlId) -> bool {
        self.profile()
            .map(|profile| profile.supports_control_read(id))
            .unwrap_or(id == ControlId::Split)
    }

    fn supports_control_write(&self, id: ControlId) -> bool {
        self.profile()
            .map(|profile| profile.supports_control_write(id))
            .unwrap_or(matches!(id, ControlId::Split | ControlId::Rit))
    }

    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        anyhow::ensure!(
            self.supports_control_write(ControlId::Rit),
            "classic Yaesu RIT is not writable for this model"
        );
        self.transact(yaesu_legacy_cat::set_clarifier_offset(offset_hz)?, 0)?;
        Ok(())
    }

    async fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        anyhow::ensure!(
            self.profile()
                .is_some_and(|profile| profile.supports_repeater_settings),
            "repeater settings are not profiled for this classic Yaesu model"
        );
        self.transact(yaesu_legacy_cat::set_repeater_shift(settings.shift), 0)?;
        if let Some(offset_hz) = settings.offset_hz {
            self.transact(
                yaesu_legacy_cat::set_repeater_offset_frequency(offset_hz)?,
                0,
            )?;
        }
        let tone_mode = match settings.tone.mode {
            ToneMode::Off => 0x8A,
            ToneMode::Encode => 0x4A,
            ToneMode::EncodeDecode => 0x2A,
            ToneMode::Dtcs => 0x0A,
        };
        self.transact(yaesu_legacy_cat::set_ctcss_dcs_mode(tone_mode), 0)?;
        match settings.tone.mode {
            ToneMode::Off => {}
            ToneMode::Dtcs => {
                let tx = settings
                    .tone
                    .dtcs_code
                    .ok_or_else(|| anyhow!("classic Yaesu DTCS requires a transmit code"))?;
                let rx = settings
                    .tone
                    .dtcs_code
                    .ok_or_else(|| anyhow!("classic Yaesu DTCS requires a receive code"))?;
                self.transact(yaesu_legacy_cat::set_dcs_codes(tx, rx)?, 0)?;
            }
            ToneMode::Encode | ToneMode::EncodeDecode => {
                let tone = settings
                    .tone
                    .frequency_tenths_hz
                    .ok_or_else(|| anyhow!("classic Yaesu CTCSS requires a tone frequency"))?;
                self.transact(yaesu_legacy_cat::set_ctcss_tones(tone, tone)?, 0)?;
            }
        }
        Ok(())
    }

    fn supports_repeater_settings(&self) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_repeater_settings)
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        match id {
            MeterId::Signal => Ok(normalize_meter_level(
                u16::from(self.get_rx_status()?.s_meter),
                15,
            )),
            MeterId::Power => Ok(normalize_meter_level(
                u16::from(self.get_tx_status()?.power_meter),
                15,
            )),
            _ => Ok(None),
        }
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_meter(id))
    }

    fn meter_poll_spec(&self, id: MeterId) -> Option<MeterPollSpec> {
        self.profile()
            .and_then(|profile| profile.meter_poll_spec(id))
    }

    fn meter_metadata(&self, id: MeterId) -> Option<MeterMetadata> {
        self.profile()
            .and_then(|profile| profile.meter_metadata(id))
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
            can_set_power: false,
            can_raw_protocol: true,
        }
    }
}

fn hal_to_legacy_mode(mode: Mode) -> Result<LegacyMode> {
    match mode {
        Mode::Lsb => Ok(LegacyMode::Lsb),
        Mode::Usb => Ok(LegacyMode::Usb),
        Mode::Cw => Ok(LegacyMode::Cw),
        Mode::CwReverse => Ok(LegacyMode::CwReverse),
        Mode::Am => Ok(LegacyMode::Am),
        Mode::Fm => Ok(LegacyMode::Fm),
        Mode::Data | Mode::Rtty | Mode::RttyReverse => Ok(LegacyMode::Digital),
        Mode::Wfm => bail!("classic Yaesu manuals do not document WFM for the mode-set command"),
    }
}

fn legacy_to_hal_mode(mode: LegacyMode) -> Mode {
    match mode {
        LegacyMode::Lsb => Mode::Lsb,
        LegacyMode::Usb => Mode::Usb,
        LegacyMode::Cw | LegacyMode::CwNarrow => Mode::Cw,
        LegacyMode::CwReverse => Mode::CwReverse,
        LegacyMode::Am => Mode::Am,
        LegacyMode::Fm | LegacyMode::FmNarrow => Mode::Fm,
        LegacyMode::Wfm => Mode::Wfm,
        LegacyMode::Digital | LegacyMode::Packet => Mode::Data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RepeaterShift, ToneSettings};
    use std::collections::VecDeque;
    use std::io::{Read, Write};

    struct ScriptedTransport {
        input: VecDeque<u8>,
    }

    impl ScriptedTransport {
        fn new() -> Self {
            Self {
                input: VecDeque::new(),
            }
        }
    }

    impl Read for ScriptedTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.input.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "scripted legacy transport has no response",
                ));
            }
            let count = buffer.len().min(self.input.len());
            for slot in &mut buffer[..count] {
                *slot = self.input.pop_front().expect("response byte");
            }
            Ok(count)
        }
    }

    impl Write for ScriptedTransport {
        fn write(&mut self, frame: &[u8]) -> std::io::Result<usize> {
            match frame.last().copied() {
                Some(0x03) => self.input.extend([0x01, 0x40, 0x74, 0x00, 0x01]),
                Some(0xE7) => self.input.push_back(0x8F),
                Some(0xF7) => self.input.push_back(0xA0),
                _ => {}
            }
            Ok(frame.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl crate::transport::RadioTransport for ScriptedTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn constructors_validate_model_baud_rates() {
        assert!(LegacyYaesuRadio::new_for_model(YaesuLegacyModel::Ft857D, "test", 38_400).is_ok());
        assert!(LegacyYaesuRadio::new_for_model(YaesuLegacyModel::Ft857D, "test", 19_200).is_err());
    }

    #[test]
    fn model_profiles_gate_frequency_and_modes_before_io() {
        let ft817 =
            LegacyYaesuRadio::new_for_model(YaesuLegacyModel::Ft817Nd, "test", 4_800).unwrap();
        assert!(!ft817.profile().unwrap().supports_frequency(40_000_000));
        assert!(ft817.profile().unwrap().supports_mode(LegacyMode::Digital));
        assert!(!ft817.profile().unwrap().supports_mode(LegacyMode::FmNarrow));
    }

    #[test]
    fn mode_conversion_preserves_reverse_and_read_only_variants() {
        assert_eq!(legacy_to_hal_mode(LegacyMode::CwReverse), Mode::CwReverse);
        assert_eq!(legacy_to_hal_mode(LegacyMode::Wfm), Mode::Wfm);
        assert_eq!(legacy_to_hal_mode(LegacyMode::FmNarrow), Mode::Fm);
        assert!(hal_to_legacy_mode(Mode::Wfm).is_err());
    }

    #[test]
    fn scripted_transport_covers_classic_radio_operation_surface() {
        let radio = LegacyYaesuRadio::with_transport(
            Some(YaesuLegacyModel::Ft857D),
            9_600,
            ScriptedTransport::new(),
        );

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            14_074_000
        );
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Usb
        );
        futures::executor::block_on(radio.set_frequency_hz(14_074_000)).unwrap();
        futures::executor::block_on(radio.set_mode(Mode::CwReverse)).unwrap();
        assert!(!futures::executor::block_on(radio.get_ptt()).unwrap());
        futures::executor::block_on(radio.set_ptt(false)).unwrap();
        assert!(!radio.get_split().unwrap());
        radio.set_split(false).unwrap();
        radio.toggle_vfo().unwrap();
        radio.set_cat_lock(true).unwrap();
        futures::executor::block_on(radio.set_rit_offset_hz(-120)).unwrap();

        assert_eq!(
            futures::executor::block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(255)
        );
        assert_eq!(
            futures::executor::block_on(radio.get_meter(MeterId::Power)).unwrap(),
            Some(0)
        );
        assert!(radio.supports_control(ControlId::Split));
        assert!(radio.supports_control_write(ControlId::Rit));
        assert!(radio.meter_poll_spec(MeterId::Signal).is_some());
        assert!(radio.meter_metadata(MeterId::Power).is_some());
        assert!(radio.capabilities().can_raw_protocol);

        futures::executor::block_on(radio.set_repeater_settings(RepeaterSettings {
            shift: crate::RepeaterShift::Plus,
            offset_hz: Some(600_000),
            tone: crate::ToneSettings {
                mode: ToneMode::Encode,
                frequency_tenths_hz: Some(885),
                ..crate::ToneSettings::default()
            },
        }))
        .unwrap();
        let raw =
            futures::executor::block_on(radio.protocol_write_read(&[0x01, 0x40, 0x74, 0x00, 0x03]))
                .unwrap();
        assert_eq!(raw.len(), 5);
        radio.close();
    }

    #[test]
    fn rejects_unsupported_legacy_requests_before_transport() {
        let radio = LegacyYaesuRadio::with_transport(
            Some(YaesuLegacyModel::Ft817Nd),
            4_800,
            ScriptedTransport::new(),
        );
        assert!(futures::executor::block_on(radio.set_frequency_hz(40_000_000)).is_err());
        assert!(radio.set_legacy_mode(LegacyMode::FmNarrow).is_err());
        assert!(futures::executor::block_on(radio.set_mode(Mode::Wfm)).is_err());
        assert!(futures::executor::block_on(radio.protocol_write_read(&[0x01, 0x02])).is_err());
        assert!(radio.transact_raw([0; 5], 2_049).is_err());
        assert!(futures::executor::block_on(
            radio.set_control(ControlId::Split, ControlValue::U8(1),)
        )
        .is_err());
        assert!(
            futures::executor::block_on(radio.get_control(ControlId::Rit))
                .unwrap()
                .is_none()
        );

        let ft1000 = LegacyYaesuRadio::with_transport(
            Some(YaesuLegacyModel::Ft1000),
            4_800,
            ScriptedTransport::new(),
        );
        assert!(futures::executor::block_on(
            ft1000.set_control(ControlId::Rit, ControlValue::Bool(true))
        )
        .is_err());
        assert!(futures::executor::block_on(ft1000.set_rit_offset_hz(100)).is_err());
    }

    #[test]
    fn repeater_tone_modes_cover_optional_and_invalid_parameters() {
        let radio = LegacyYaesuRadio::with_transport(
            Some(YaesuLegacyModel::Ft857D),
            9_600,
            ScriptedTransport::new(),
        );
        futures::executor::block_on(radio.set_repeater_settings(RepeaterSettings {
            shift: RepeaterShift::Minus,
            offset_hz: None,
            tone: ToneSettings {
                mode: ToneMode::Off,
                ..ToneSettings::default()
            },
        }))
        .unwrap();
        futures::executor::block_on(radio.set_repeater_settings(RepeaterSettings {
            shift: RepeaterShift::Simplex,
            offset_hz: Some(600_000),
            tone: ToneSettings {
                mode: ToneMode::EncodeDecode,
                frequency_tenths_hz: Some(1000),
                ..ToneSettings::default()
            },
        }))
        .unwrap();
        assert!(
            futures::executor::block_on(radio.set_repeater_settings(RepeaterSettings {
                tone: ToneSettings {
                    mode: ToneMode::Dtcs,
                    ..ToneSettings::default()
                },
                ..RepeaterSettings::default()
            }))
            .is_err()
        );
        futures::executor::block_on(radio.set_repeater_settings(RepeaterSettings {
            tone: ToneSettings {
                mode: ToneMode::Dtcs,
                dtcs_code: Some(23),
                ..ToneSettings::default()
            },
            ..RepeaterSettings::default()
        }))
        .unwrap();
        futures::executor::block_on(radio.set_control(ControlId::Rit, ControlValue::Bool(true)))
            .unwrap();
        assert!(
            futures::executor::block_on(radio.get_meter(MeterId::Voltage))
                .unwrap()
                .is_none()
        );
    }
}
