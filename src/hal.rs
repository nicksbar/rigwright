//! Protocol-neutral radio hardware abstraction layer.

pub use crate::events::RadioEventRouter;
pub use crate::hal_types::{
    BaseMode, ControlId, ControlValue, CoreState, DtmfSequence, FilterBandwidth, MemoryChannel,
    MeterId, MeterMetadata, MeterPollSpec, MeterPresentation, Mode, OperatingMode,
    RepeaterSettings, RepeaterShift, ScopeConfiguration, ScopeMetadata, ScopeState, SwrSweepSetup,
    ToneSettings, TunerStatus,
};
use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// Operations a backend can actually perform.
///
/// Capabilities describe implemented driver behavior, not everything listed in
/// a radio manual. Callers should gate controls on both these flags and the
/// selected model profile.
#[derive(Debug, Clone, Default)]
pub struct RadioCapabilities {
    pub can_get_frequency: bool,
    pub can_set_frequency: bool,
    pub can_get_mode: bool,
    pub can_set_mode: bool,
    pub can_get_ptt: bool,
    pub can_set_ptt: bool,
    pub can_get_power: bool,
    pub can_set_power: bool,
    pub can_raw_protocol: bool,
}

/// A protocol-neutral snapshot of link health, derived from the transport
/// counters each driver already maintains. All fields are optional so drivers
/// surface only what they measure; `None` means "not instrumented", never
/// "zero". Applications can render this directly ("radio link degraded: 3
/// consecutive timeouts") without knowing the vendor protocol.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LinkHealth {
    /// Commands issued to the radio.
    pub commands_started: Option<u64>,
    /// Commands that received a matched response.
    pub responses_matched: Option<u64>,
    /// Commands that timed out waiting for a response.
    pub response_timeouts: Option<u64>,
    /// Current run of consecutive timeouts (reset on any success).
    pub consecutive_timeouts: Option<u32>,
    /// Mean response latency across matched responses.
    pub avg_response: Option<Duration>,
    /// Framing/parse-level events dropped from the retain buffer.
    pub frames_dropped: Option<u64>,
}

impl LinkHealth {
    /// True when the link shows signs of trouble worth surfacing to the
    /// operator: any consecutive-timeout backlog or a timeout on a nontrivial
    /// share of commands.
    pub fn is_degraded(&self) -> bool {
        if self.consecutive_timeouts.unwrap_or(0) > 0 {
            return true;
        }
        match (self.response_timeouts, self.commands_started) {
            (Some(timeouts), Some(started)) if started >= 8 => timeouts * 4 >= started,
            _ => false,
        }
    }
}

#[async_trait]
/// Protocol-neutral radio control interface.
///
/// Methods are asynchronous at the API boundary so network and serial drivers
/// can share one interface. Native serial backends currently perform blocking
/// I/O internally; applications should avoid calling them while holding UI or
/// other latency-sensitive locks.
pub trait Radio: Send + Sync {
    /// Shared unsolicited-event source, when the backend has a persistent
    /// protocol event router. Consumers should retain the returned router or
    /// subscription for the lifetime of the connection.
    fn event_router(&self) -> Option<RadioEventRouter> {
        None
    }
    async fn get_frequency_hz(&self) -> Result<u64>;
    async fn set_frequency_hz(&self, hz: u64) -> Result<()>;
    async fn get_mode(&self) -> Result<Mode>;
    async fn set_mode(&self, mode: Mode) -> Result<()>;
    async fn set_ptt(&self, enabled: bool) -> Result<()>;
    async fn get_ptt(&self) -> Result<bool> {
        anyhow::bail!("reading PTT state is not supported by this radio")
    }
    /// Read the radio's core operating state (frequency, mode, PTT) in as few
    /// protocol round trips as the backend allows. The default issues the
    /// individual reads; backends with a combined status frame (such as the
    /// Yaesu `IF;` response) override this to collapse the refresh round
    /// trips. Fields that cannot be read are left `None`.
    async fn read_core_state(&self) -> Result<CoreState> {
        let frequency_hz = self.get_frequency_hz().await.ok();
        let mode = self.get_mode().await.ok();
        let ptt = self.get_ptt().await.ok();
        if frequency_hz.is_none() && mode.is_none() && ptt.is_none() {
            anyhow::bail!("radio refresh returned no readable core state")
        }
        Ok(CoreState {
            frequency_hz,
            mode,
            ptt,
        })
    }
    /// A point-in-time snapshot of link health derived from the driver's
    /// transport counters. The default returns an empty (uninstrumented)
    /// snapshot; serial backends override it with their measured counters.
    fn link_health(&self) -> LinkHealth {
        LinkHealth::default()
    }
    /// How long ago the radio last pushed a typed unsolicited event, when the
    /// backend has a live event stream. `None` means the backend does not
    /// track (or has not seen) unsolicited events. The session uses this to
    /// trust recently-streamed state instead of re-polling on every refresh.
    fn event_stream_age(&self) -> Option<Duration> {
        None
    }
    async fn get_power(&self) -> Result<bool> {
        anyhow::bail!("reading radio power state is not supported by this radio")
    }
    async fn set_power(&self, _enabled: bool) -> Result<()> {
        anyhow::bail!("setting radio power state is not supported by this radio")
    }
    fn supports_scope(&self) -> bool {
        false
    }
    /// Whether the selected model documents a native I/Q output surface.
    /// This reports model capability only; a driver may still require a
    /// separate USB/audio transport before samples can be opened.
    fn supports_iq_output(&self) -> bool {
        false
    }
    fn scope_metadata(&self) -> Option<ScopeMetadata> {
        None
    }
    async fn get_scope_state(&self) -> Result<ScopeState> {
        anyhow::bail!("native scope readback is not supported by this radio")
    }
    /// Return documented bandwidth metadata for a normalized filter choice.
    fn filter_bandwidth_hz(&self, _mode: Mode, _filter: u8) -> Option<u32> {
        None
    }
    /// Return the documented carrier mode and normalized RF power for SWR
    /// measurements. `None` means this radio has no supported procedure.
    fn swr_sweep_setup(&self) -> Option<SwrSweepSetup> {
        None
    }
    /// Convert a normalized meter value to a driver-calibrated presentation.
    fn meter_presentation(&self, _id: MeterId, _normalized: u8) -> Option<MeterPresentation> {
        None
    }
    fn control_max(&self, _id: ControlId) -> Option<u8> {
        None
    }
    fn supported_control_values(&self, _id: ControlId) -> Option<&'static [u8]> {
        None
    }
    fn meter_poll_spec(&self, _id: MeterId) -> Option<MeterPollSpec> {
        None
    }
    fn meter_metadata(&self, _id: MeterId) -> Option<MeterMetadata> {
        None
    }
    async fn set_scope_configuration(&self, _config: ScopeConfiguration) -> Result<()> {
        anyhow::bail!("native scope configuration is not supported by this radio")
    }
    async fn protocol_write_read(&self, _request: &[u8]) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
    async fn get_control(&self, _id: ControlId) -> Result<Option<ControlValue>> {
        Ok(None)
    }
    async fn set_control(&self, _id: ControlId, _value: ControlValue) -> Result<()> {
        Ok(())
    }
    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        anyhow::bail!("repeater tone/offset control is not supported by this radio")
    }
    async fn set_repeater_settings(&self, _settings: RepeaterSettings) -> Result<()> {
        anyhow::bail!("repeater tone/offset control is not supported by this radio")
    }
    async fn get_rit_offset_hz(&self) -> Result<i32> {
        anyhow::bail!("RIT offset control is not supported by this radio")
    }
    async fn set_rit_offset_hz(&self, _offset_hz: i32) -> Result<()> {
        anyhow::bail!("RIT offset control is not supported by this radio")
    }
    async fn get_xit_offset_hz(&self) -> Result<i32> {
        anyhow::bail!("XIT offset control is not supported by this radio")
    }
    async fn set_xit_offset_hz(&self, _offset_hz: i32) -> Result<()> {
        anyhow::bail!("XIT offset control is not supported by this radio")
    }
    async fn select_memory_channel(&self, _channel: u16) -> Result<()> {
        anyhow::bail!("memory/channel control is not supported by this radio")
    }
    async fn read_memory_channel(&self, _channel: u16) -> Result<MemoryChannel> {
        anyhow::bail!("memory/channel read is not supported by this radio")
    }
    async fn write_memory_channel(&self, _channel: MemoryChannel) -> Result<()> {
        anyhow::bail!("memory/channel write is not supported by this radio")
    }
    async fn send_dtmf(&self, _sequence: DtmfSequence) -> Result<()> {
        anyhow::bail!("DTMF control is not supported by this radio")
    }
    fn supports_repeater_settings(&self) -> bool {
        false
    }
    fn supports_memory_channels(&self) -> bool {
        false
    }
    fn supports_send_dtmf(&self) -> bool {
        false
    }
    /// Read a normalized meter level on the HAL's 0..=255 scale.
    async fn get_meter(&self, _id: MeterId) -> Result<Option<u8>> {
        Ok(None)
    }
    /// Report whether this driver has a documented implementation for a
    /// particular normalized meter.
    fn supports_meter(&self, _id: MeterId) -> bool {
        false
    }
    /// Report whether this driver has a documented implementation for a
    /// particular typed control.
    fn supports_control(&self, _id: ControlId) -> bool {
        false
    }
    /// Report whether a typed control has a reliable readback operation.
    /// Defaults to the legacy supported-control behavior for existing drivers.
    fn supports_control_read(&self, id: ControlId) -> bool {
        self.supports_control(id)
    }
    /// Report whether a typed control has a reliable write operation.
    /// Defaults to the legacy supported-control behavior for existing drivers.
    fn supports_control_write(&self, id: ControlId) -> bool {
        self.supports_control(id)
    }
    /// Discover every typed control advertised by this driver.
    fn supported_controls(&self) -> Vec<ControlId> {
        ControlId::ALL
            .iter()
            .copied()
            .filter(|id| self.supports_control(*id))
            .collect()
    }
    /// Discover every normalized meter advertised by this driver.
    fn supported_meters(&self) -> Vec<MeterId> {
        MeterId::ALL
            .iter()
            .copied()
            .filter(|id| self.supports_meter(*id))
            .collect()
    }
    async fn start_tuner(&self) -> Result<()> {
        anyhow::bail!("antenna tuner control is not supported by this radio")
    }
    async fn get_tuner_status(&self) -> Result<Option<TunerStatus>> {
        Ok(None)
    }
    fn capabilities(&self) -> RadioCapabilities;

    /// Compatibility spelling used by the original compact `Radio` API.
    async fn frequency(&self) -> Result<u64> {
        self.get_frequency_hz().await
    }
    /// Compatibility spelling used by the original compact `Radio` API.
    async fn set_frequency(&self, hz: u64) -> Result<()> {
        self.set_frequency_hz(hz).await
    }
    /// Compatibility spelling used by the original compact `Radio` API.
    async fn mode(&self) -> Result<Mode> {
        self.get_mode().await
    }
    /// Compatibility spelling used by the original compact `Radio` API.
    async fn ptt(&self, enabled: bool) -> Result<()> {
        self.set_ptt(enabled).await
    }
}

#[derive(Debug, Clone, Default)]
pub struct RadioStatus {
    pub frequency_hz: Option<u64>,
    pub mode: Option<String>,
    pub mode_details: Option<OperatingMode>,
}

/// In-memory backend for tests, offline UI work, and examples.
#[derive(Debug, Clone)]
pub struct NullRadio {
    state: std::sync::Arc<std::sync::Mutex<(u64, Mode, bool)>>,
}

impl Default for NullRadio {
    fn default() -> Self {
        Self::new()
    }
}

impl NullRadio {
    pub fn new() -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new((14_074_000, Mode::Usb, false))),
        }
    }

    pub fn with_frequency_mode(frequency_hz: u64, mode: Mode) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new((frequency_hz, mode, false))),
        }
    }
}

#[async_trait]
impl Radio for NullRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        Ok(self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("null radio lock poisoned"))?
            .0)
    }
    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("null radio lock poisoned"))?
            .0 = hz;
        Ok(())
    }
    async fn get_mode(&self) -> Result<Mode> {
        Ok(self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("null radio lock poisoned"))?
            .1)
    }
    async fn set_mode(&self, mode: Mode) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("null radio lock poisoned"))?
            .1 = mode;
        Ok(())
    }
    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| anyhow::anyhow!("null radio lock poisoned"))?
            .2 = enabled;
        Ok(())
    }
    fn swr_sweep_setup(&self) -> Option<crate::SwrSweepSetup> {
        Some(crate::SwrSweepSetup {
            carrier_mode: Mode::Rtty,
            rf_power: 77,
        })
    }
    async fn get_ptt(&self) -> Result<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("null radio lock poisoned"))?
            .2)
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
            can_raw_protocol: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MinimalRadio;

    #[async_trait]
    impl Radio for MinimalRadio {
        async fn get_frequency_hz(&self) -> Result<u64> {
            Ok(14_074_000)
        }
        async fn set_frequency_hz(&self, _hz: u64) -> Result<()> {
            Ok(())
        }
        async fn get_mode(&self) -> Result<Mode> {
            Ok(Mode::Usb)
        }
        async fn set_mode(&self, _mode: Mode) -> Result<()> {
            Ok(())
        }
        async fn set_ptt(&self, _enabled: bool) -> Result<()> {
            Ok(())
        }
        fn capabilities(&self) -> RadioCapabilities {
            RadioCapabilities::default()
        }
    }

    #[test]
    fn default_radio_contract_is_explicit_and_exercised() {
        let radio = MinimalRadio;
        assert!(radio.event_router().is_none());
        assert_eq!(
            futures::executor::block_on(radio.frequency()).unwrap(),
            14_074_000
        );
        futures::executor::block_on(radio.set_frequency(14_075_000)).unwrap();
        assert_eq!(
            futures::executor::block_on(radio.mode()).unwrap(),
            Mode::Usb
        );
        futures::executor::block_on(radio.ptt(false)).unwrap();
        assert!(futures::executor::block_on(radio.get_ptt()).is_err());
        assert!(futures::executor::block_on(radio.get_power()).is_err());
        assert!(futures::executor::block_on(radio.set_power(false)).is_err());
        assert!(futures::executor::block_on(radio.protocol_write_read(&[]))
            .unwrap()
            .is_empty());
        assert_eq!(
            futures::executor::block_on(radio.get_control(ControlId::RfPower)).unwrap(),
            None
        );
        futures::executor::block_on(radio.set_control(ControlId::RfPower, ControlValue::U8(1)))
            .unwrap();
        assert!(futures::executor::block_on(radio.get_repeater_settings()).is_err());
        assert!(futures::executor::block_on(
            radio.set_repeater_settings(RepeaterSettings::default())
        )
        .is_err());
        assert!(futures::executor::block_on(radio.get_rit_offset_hz()).is_err());
        assert!(futures::executor::block_on(radio.set_rit_offset_hz(0)).is_err());
        assert!(futures::executor::block_on(radio.get_xit_offset_hz()).is_err());
        assert!(futures::executor::block_on(radio.set_xit_offset_hz(0)).is_err());
        assert!(futures::executor::block_on(radio.select_memory_channel(1)).is_err());
        assert!(futures::executor::block_on(radio.read_memory_channel(1)).is_err());
        assert!(
            futures::executor::block_on(radio.write_memory_channel(MemoryChannel {
                channel: 1,
                name: None,
                frequency_hz: 14_074_000,
                transmit_frequency_hz: None,
                mode: Mode::Usb,
                repeater: RepeaterSettings::default(),
            }))
            .is_err()
        );
        assert!(
            futures::executor::block_on(radio.send_dtmf(DtmfSequence::new("1").unwrap())).is_err()
        );
        assert!(!radio.supports_repeater_settings());
        assert!(!radio.supports_memory_channels());
        assert!(!radio.supports_send_dtmf());
        assert!(
            futures::executor::block_on(radio.get_meter(MeterId::Signal))
                .unwrap()
                .is_none()
        );
        assert!(!radio.supports_meter(MeterId::Signal));
        assert!(!radio.supports_control(ControlId::RfPower));
        assert!(!radio.supports_control_read(ControlId::RfPower));
        assert!(!radio.supports_control_write(ControlId::RfPower));
        assert!(radio.supported_controls().is_empty());
        assert!(radio.supported_meters().is_empty());
        assert!(futures::executor::block_on(radio.start_tuner()).is_err());
        assert!(futures::executor::block_on(radio.get_tuner_status())
            .unwrap()
            .is_none());
        assert!(!radio.capabilities().can_get_frequency);
        let _ = NullRadio::default();
    }

    #[test]
    fn link_health_flags_consecutive_timeouts_as_degraded() {
        let healthy = LinkHealth::default();
        assert!(!healthy.is_degraded());

        let backlog = LinkHealth {
            consecutive_timeouts: Some(2),
            ..LinkHealth::default()
        };
        assert!(backlog.is_degraded());
    }

    #[test]
    fn link_health_flags_a_high_timeout_rate_once_traffic_is_meaningful() {
        // Below the minimum traffic floor, a couple of timeouts on a fresh
        // link should not raise a degradation alarm.
        let quiet = LinkHealth {
            commands_started: Some(4),
            response_timeouts: Some(2),
            ..LinkHealth::default()
        };
        assert!(!quiet.is_degraded());

        // Once enough commands have run, a >=25% timeout rate is degraded.
        let lossy = LinkHealth {
            commands_started: Some(20),
            response_timeouts: Some(5),
            ..LinkHealth::default()
        };
        assert!(lossy.is_degraded());

        let solid = LinkHealth {
            commands_started: Some(20),
            response_timeouts: Some(1),
            ..LinkHealth::default()
        };
        assert!(!solid.is_degraded());
    }

    #[test]
    fn default_link_health_is_uninstrumented() {
        let radio = MinimalRadio;
        let health = radio.link_health();
        assert_eq!(health, LinkHealth::default());
        assert!(!health.is_degraded());
    }

    #[test]
    fn null_radio_preserves_state_and_reports_its_capabilities() {
        let radio = NullRadio::with_frequency_mode(7_074_000, Mode::Cw);
        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            7_074_000
        );
        assert_eq!(
            futures::executor::block_on(radio.get_mode()).unwrap(),
            Mode::Cw
        );
        futures::executor::block_on(radio.set_frequency_hz(14_074_000)).unwrap();
        futures::executor::block_on(radio.set_mode(Mode::Data)).unwrap();
        futures::executor::block_on(radio.set_ptt(true)).unwrap();
        assert!(futures::executor::block_on(radio.get_ptt()).unwrap());
        let capabilities = radio.capabilities();
        assert!(capabilities.can_get_frequency && capabilities.can_set_mode);
        assert!(!capabilities.can_set_power && !capabilities.can_raw_protocol);
    }
}
