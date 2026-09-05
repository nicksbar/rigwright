//! Driver-owned command admission, state tracking, and refresh scheduling.
//!
//! `RadioSession` sits above a vendor driver and below an application. It
//! owns one worker for a radio connection, so clients submit intent instead of
//! implementing their own polling loops. The wrapped `Radio` remains the
//! owner of protocol details and model capabilities.

use crate::{ControlId, ControlValue, Mode, Radio, RadioCapabilities, RadioEvent};
use futures::channel::oneshot;
use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    sync::{Arc, Condvar, Mutex, Weak},
    thread,
    time::{Duration, Instant},
};

const DEFAULT_QUEUE_CAPACITY: usize = 32;
const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
/// Default ceiling for a continuous transmit hold before the session forces
/// PTT off as a safety measure.
const DEFAULT_MAX_TX_HOLD: Duration = Duration::from_secs(180);
/// How fresh the radio's unsolicited event stream must be for a refresh to
/// trust the streamed observed state instead of issuing wire polls. Well
/// under any reasonable CI-V/CAT event cadence, so a live stream makes
/// refreshes effectively free while a stalled stream falls back to polling.
const EVENT_STREAM_FRESHNESS: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Disconnected,
    Opening,
    Probing,
    Synchronizing,
    Starting,
    Ready,
    Recovering,
    Degraded,
    Closing,
    Stopped,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RadioState {
    pub frequency_hz: Option<u64>,
    pub mode: Option<Mode>,
    pub ptt: Option<bool>,
    pub controls: Vec<(ControlId, ControlValue)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadioSnapshot {
    pub status: SessionStatus,
    pub desired: RadioState,
    pub observed: RadioState,
    pub pending: Vec<SessionOperation>,
    pub sequence: u64,
    pub generation: u64,
    pub synchronized: bool,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionDiagnostics {
    pub generation: u64,
    pub queued: u64,
    pub completed: u64,
    pub failed: u64,
    pub coalesced: u64,
    pub recoveries: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionOperation {
    Refresh,
    SetFrequency(u64),
    SetMode(Mode),
    SetPtt(bool),
    SetControl(ControlId, ControlValue),
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCommandClass {
    Query,
    StateWrite,
    SafetyCritical,
    Raw,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionOutcome {
    Accepted,
    Applied,
    Superseded,
    Rejected(SessionError),
    Failed(SessionError),
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Priority {
    Safety,
    State,
    Refresh,
}

impl SessionOperation {
    fn priority(&self) -> Priority {
        match self {
            Self::SetPtt(_) => Priority::Safety,
            Self::Refresh => Priority::Refresh,
            Self::SetFrequency(_) | Self::SetMode(_) | Self::SetControl(_, _) | Self::Raw(_) => {
                Priority::State
            }
        }
    }

    pub fn class(&self) -> SessionCommandClass {
        match self {
            Self::Refresh => SessionCommandClass::Query,
            Self::SetPtt(_) => SessionCommandClass::SafetyCritical,
            Self::SetFrequency(_) | Self::SetMode(_) | Self::SetControl(_, _) => {
                SessionCommandClass::StateWrite
            }
            Self::Raw(_) => SessionCommandClass::Raw,
        }
    }

    fn coalesce_key(&self) -> Option<CoalesceKey> {
        match self {
            Self::Refresh => Some(CoalesceKey::Refresh),
            Self::SetFrequency(_) => Some(CoalesceKey::Frequency),
            Self::SetMode(_) => Some(CoalesceKey::Mode),
            Self::SetPtt(_) => Some(CoalesceKey::Ptt),
            Self::SetControl(id, _) => Some(CoalesceKey::Control(*id)),
            Self::Raw(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoalesceKey {
    Refresh,
    Frequency,
    Mode,
    Ptt,
    Control(ControlId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    Invalid(String),
    Unsupported(String),
    QueueFull,
    Closed,
    Superseded,
    Backend(String),
    InvalidFrame(String),
    StaleGeneration,
    TimedOut,
    Disconnected,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(error) => write!(f, "invalid radio session command: {error}"),
            Self::Unsupported(error) => write!(f, "unsupported radio session command: {error}"),
            Self::QueueFull => write!(f, "radio session command queue is full"),
            Self::Closed => write!(f, "radio session is closed"),
            Self::Superseded => write!(f, "radio session command was superseded"),
            Self::Backend(error) => write!(f, "radio session backend error: {error}"),
            Self::InvalidFrame(error) => write!(f, "invalid radio session frame: {error}"),
            Self::StaleGeneration => {
                write!(f, "radio session command belongs to a stale generation")
            }
            Self::TimedOut => write!(f, "radio session command timed out"),
            Self::Disconnected => write!(f, "radio session is disconnected"),
        }
    }
}

impl Error for SessionError {}

pub type SessionTicket = oneshot::Receiver<Result<RadioSnapshot, SessionError>>;

#[derive(Debug, Clone, Copy)]
pub struct SessionConfig {
    pub queue_capacity: usize,
    pub refresh_interval: Option<Duration>,
    /// Maximum continuous transmit hold before the session forces PTT back
    /// off. This is a safety net for a crashed or stuck client: once the
    /// ceiling is reached the worker issues `SetPtt(false)` itself. `None`
    /// disables the watchdog. Defaults to three minutes, the classic
    /// FCC-style transmission limit.
    pub max_tx_hold: Option<Duration>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            refresh_interval: Some(DEFAULT_REFRESH_INTERVAL),
            max_tx_hold: Some(DEFAULT_MAX_TX_HOLD),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    SnapshotChanged(RadioSnapshot),
    OperationAccepted(SessionOperation),
    OperationCompleted {
        operation: SessionOperation,
        outcome: SessionOutcome,
    },
    /// The safety watchdog forced PTT off after the configured maximum
    /// continuous transmit hold elapsed.
    PttWatchdogTripped,
}

#[derive(Debug, Default, Clone)]
pub struct SessionEventRouter {
    state: Arc<Mutex<Vec<Vec<SessionEvent>>>>,
}

impl SessionEventRouter {
    pub fn subscribe(&self) -> SessionEventSubscription {
        let mut state = self.state.lock().expect("session event lock poisoned");
        state.push(Vec::new());
        SessionEventSubscription {
            index: state.len() - 1,
            router: self.clone(),
        }
    }

    fn publish(&self, event: SessionEvent) {
        if let Ok(mut state) = self.state.lock() {
            for queue in &mut *state {
                queue.push(event.clone());
            }
        }
    }

    fn drain(&self, index: usize) -> Vec<SessionEvent> {
        self.state
            .lock()
            .ok()
            .and_then(|mut state| state.get_mut(index).map(std::mem::take))
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct SessionEventSubscription {
    index: usize,
    router: SessionEventRouter,
}

impl SessionEventSubscription {
    pub fn drain(&self) -> Vec<SessionEvent> {
        self.router.drain(self.index)
    }
}

struct QueuedOperation {
    operation: SessionOperation,
    generation: u64,
    deadline: Instant,
    waiters: Vec<oneshot::Sender<Result<RadioSnapshot, SessionError>>>,
}

struct SharedState {
    queue: VecDeque<QueuedOperation>,
    snapshot: RadioSnapshot,
    closed: bool,
    diagnostics: SessionDiagnostics,
    /// When the transmitter was last observed or commanded on. `Some` only
    /// while PTT is believed active; used by the safety watchdog.
    ptt_on_since: Option<Instant>,
}

struct Shared {
    state: Mutex<SharedState>,
    wake: Condvar,
    events: SessionEventRouter,
    queue_capacity: usize,
    max_tx_hold: Option<Duration>,
    event_subscription: Mutex<Option<crate::RadioEventSubscription>>,
}

pub struct RadioSession {
    radio: Arc<Mutex<Arc<dyn Radio>>>,
    shared: Arc<Shared>,
}

#[async_trait::async_trait]
impl Radio for RadioSession {
    fn event_router(&self) -> Option<crate::RadioEventRouter> {
        self.current_radio().event_router()
    }

    async fn get_frequency_hz(&self) -> anyhow::Result<u64> {
        let snapshot = self.wait(self.refresh()).await?;
        snapshot
            .observed
            .frequency_hz
            .ok_or_else(|| anyhow::anyhow!("radio refresh returned no frequency"))
    }

    async fn set_frequency_hz(&self, hz: u64) -> anyhow::Result<()> {
        self.wait(self.set_frequency(hz)).await.map(|_| ())
    }

    async fn get_mode(&self) -> anyhow::Result<Mode> {
        let snapshot = self.wait(self.refresh()).await?;
        snapshot
            .observed
            .mode
            .ok_or_else(|| anyhow::anyhow!("radio refresh returned no mode"))
    }

    async fn set_mode(&self, mode: Mode) -> anyhow::Result<()> {
        self.wait(self.set_mode(mode)).await.map(|_| ())
    }

    async fn set_ptt(&self, enabled: bool) -> anyhow::Result<()> {
        self.wait(self.set_ptt(enabled)).await.map(|_| ())
    }

    async fn get_ptt(&self) -> anyhow::Result<bool> {
        let snapshot = self.wait(self.refresh()).await?;
        snapshot
            .observed
            .ptt
            .ok_or_else(|| anyhow::anyhow!("radio refresh returned no PTT state"))
    }

    async fn read_core_state(&self) -> anyhow::Result<crate::CoreState> {
        let snapshot = self.wait(self.refresh()).await?;
        Ok(crate::CoreState {
            frequency_hz: snapshot.observed.frequency_hz,
            mode: snapshot.observed.mode,
            ptt: snapshot.observed.ptt,
        })
    }

    fn link_health(&self) -> crate::hal::LinkHealth {
        self.current_radio().link_health()
    }

    fn event_stream_age(&self) -> Option<Duration> {
        self.current_radio().event_stream_age()
    }

    async fn get_power(&self) -> anyhow::Result<bool> {
        self.current_radio().get_power().await
    }

    async fn set_power(&self, enabled: bool) -> anyhow::Result<()> {
        self.current_radio().set_power(enabled).await
    }

    fn supports_scope(&self) -> bool {
        self.current_radio().supports_scope()
    }

    fn supports_iq_output(&self) -> bool {
        self.current_radio().supports_iq_output()
    }

    fn scope_metadata(&self) -> Option<crate::ScopeMetadata> {
        self.current_radio().scope_metadata()
    }

    async fn get_scope_state(&self) -> anyhow::Result<crate::ScopeState> {
        self.current_radio().get_scope_state().await
    }

    fn filter_bandwidth_hz(&self, mode: Mode, filter: u8) -> Option<u32> {
        self.current_radio().filter_bandwidth_hz(mode, filter)
    }

    fn swr_sweep_setup(&self) -> Option<crate::SwrSweepSetup> {
        self.current_radio().swr_sweep_setup()
    }

    fn meter_presentation(
        &self,
        id: crate::MeterId,
        normalized: u8,
    ) -> Option<crate::MeterPresentation> {
        self.current_radio().meter_presentation(id, normalized)
    }

    fn control_max(&self, id: ControlId) -> Option<u8> {
        self.current_radio().control_max(id)
    }

    fn supported_control_values(&self, id: ControlId) -> Option<&'static [u8]> {
        self.current_radio().supported_control_values(id)
    }

    fn meter_poll_spec(&self, id: crate::MeterId) -> Option<crate::MeterPollSpec> {
        self.current_radio().meter_poll_spec(id)
    }

    fn meter_metadata(&self, id: crate::MeterId) -> Option<crate::MeterMetadata> {
        self.current_radio().meter_metadata(id)
    }

    async fn set_scope_configuration(
        &self,
        config: crate::ScopeConfiguration,
    ) -> anyhow::Result<()> {
        self.current_radio().set_scope_configuration(config).await
    }

    async fn protocol_write_read(&self, request: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.current_radio().protocol_write_read(request).await
    }

    async fn get_control(&self, id: ControlId) -> anyhow::Result<Option<ControlValue>> {
        self.current_radio().get_control(id).await
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> anyhow::Result<()> {
        self.wait(self.set_control(id, value)).await.map(|_| ())
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.current_radio().supports_control(id)
    }

    fn supports_control_read(&self, id: ControlId) -> bool {
        self.current_radio().supports_control_read(id)
    }

    fn supports_control_write(&self, id: ControlId) -> bool {
        self.current_radio().supports_control_write(id)
    }

    async fn get_repeater_settings(&self) -> anyhow::Result<crate::RepeaterSettings> {
        self.current_radio().get_repeater_settings().await
    }

    async fn set_repeater_settings(&self, settings: crate::RepeaterSettings) -> anyhow::Result<()> {
        self.current_radio().set_repeater_settings(settings).await
    }

    async fn get_rit_offset_hz(&self) -> anyhow::Result<i32> {
        self.current_radio().get_rit_offset_hz().await
    }

    async fn set_rit_offset_hz(&self, offset_hz: i32) -> anyhow::Result<()> {
        self.current_radio().set_rit_offset_hz(offset_hz).await
    }

    async fn get_xit_offset_hz(&self) -> anyhow::Result<i32> {
        self.current_radio().get_xit_offset_hz().await
    }

    async fn set_xit_offset_hz(&self, offset_hz: i32) -> anyhow::Result<()> {
        self.current_radio().set_xit_offset_hz(offset_hz).await
    }

    async fn select_memory_channel(&self, channel: u16) -> anyhow::Result<()> {
        self.current_radio().select_memory_channel(channel).await
    }

    async fn read_memory_channel(&self, channel: u16) -> anyhow::Result<crate::MemoryChannel> {
        self.current_radio().read_memory_channel(channel).await
    }

    async fn write_memory_channel(&self, channel: crate::MemoryChannel) -> anyhow::Result<()> {
        self.current_radio().write_memory_channel(channel).await
    }

    async fn send_dtmf(&self, sequence: crate::DtmfSequence) -> anyhow::Result<()> {
        self.current_radio().send_dtmf(sequence).await
    }

    fn supports_repeater_settings(&self) -> bool {
        self.current_radio().supports_repeater_settings()
    }

    fn supports_memory_channels(&self) -> bool {
        self.current_radio().supports_memory_channels()
    }

    fn supports_memory_selection(&self) -> bool {
        self.current_radio().supports_memory_selection()
    }

    fn supports_send_dtmf(&self) -> bool {
        self.current_radio().supports_send_dtmf()
    }

    async fn get_meter(&self, id: crate::MeterId) -> anyhow::Result<Option<u8>> {
        self.current_radio().get_meter(id).await
    }

    fn supports_meter(&self, id: crate::MeterId) -> bool {
        self.current_radio().supports_meter(id)
    }

    fn supported_controls(&self) -> Vec<ControlId> {
        self.current_radio().supported_controls()
    }

    fn supported_meters(&self) -> Vec<crate::MeterId> {
        self.current_radio().supported_meters()
    }

    async fn start_tuner(&self) -> anyhow::Result<()> {
        self.current_radio().start_tuner().await
    }

    async fn get_tuner_status(&self) -> anyhow::Result<Option<crate::TunerStatus>> {
        self.current_radio().get_tuner_status().await
    }

    fn capabilities(&self) -> RadioCapabilities {
        self.current_radio().capabilities()
    }
}

impl Clone for RadioSession {
    fn clone(&self) -> Self {
        Self {
            radio: Arc::clone(&self.radio),
            shared: Arc::clone(&self.shared),
        }
    }
}

impl fmt::Debug for RadioSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RadioSession")
            .field("snapshot", &self.snapshot())
            .finish_non_exhaustive()
    }
}

impl RadioSession {
    pub fn new(radio: Arc<dyn Radio>, config: SessionConfig) -> Result<Self, SessionError> {
        if config.queue_capacity == 0 {
            return Err(SessionError::QueueFull);
        }
        let snapshot = RadioSnapshot {
            status: SessionStatus::Starting,
            desired: RadioState::default(),
            observed: RadioState::default(),
            pending: Vec::new(),
            sequence: 0,
            generation: 0,
            synchronized: false,
            last_error: None,
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(SharedState {
                queue: VecDeque::new(),
                snapshot,
                closed: false,
                diagnostics: SessionDiagnostics::default(),
                ptt_on_since: None,
            }),
            wake: Condvar::new(),
            events: SessionEventRouter::default(),
            queue_capacity: config.queue_capacity,
            max_tx_hold: config.max_tx_hold,
            event_subscription: Mutex::new(None),
        });
        let weak = Arc::downgrade(&shared);
        let shared_radio = Arc::new(Mutex::new(radio));
        let worker_radio = Arc::clone(&shared_radio);
        let event_subscription = shared_radio
            .lock()
            .ok()
            .and_then(|radio| radio.event_router().map(|router| router.subscribe()));
        if let Ok(mut subscription) = shared.event_subscription.lock() {
            *subscription = event_subscription;
        }
        thread::Builder::new()
            .name("rigwright-radio-session".to_string())
            .spawn(move || worker_loop(weak, worker_radio, config.refresh_interval))
            .map_err(|error| SessionError::Backend(error.to_string()))?;
        Ok(Self {
            radio: shared_radio,
            shared,
        })
    }

    pub fn from_radio<R>(radio: Arc<R>, config: SessionConfig) -> Result<Self, SessionError>
    where
        R: Radio + 'static,
    {
        let erased: Arc<dyn Radio> = radio;
        Self::new(erased, config)
    }

    pub fn snapshot(&self) -> RadioSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.snapshot.clone())
            .unwrap_or_else(|_| RadioSnapshot {
                status: SessionStatus::Stopped,
                desired: RadioState::default(),
                observed: RadioState::default(),
                pending: Vec::new(),
                sequence: 0,
                generation: 0,
                synchronized: false,
                last_error: Some("radio session lock poisoned".to_string()),
            })
    }

    pub fn events(&self) -> SessionEventRouter {
        self.shared.events.clone()
    }

    pub fn capabilities(&self) -> RadioCapabilities {
        self.current_radio().capabilities()
    }

    fn current_radio(&self) -> Arc<dyn Radio> {
        self.radio
            .lock()
            .map(|radio| Arc::clone(&radio))
            .unwrap_or_else(|_| Arc::new(crate::NullRadio::new()))
    }

    pub fn diagnostics(&self) -> SessionDiagnostics {
        self.shared
            .state
            .lock()
            .map(|state| state.diagnostics)
            .unwrap_or_default()
    }

    /// A protocol-neutral snapshot of the wrapped radio's link health, drawn
    /// from the backend's transport counters. Combine this with
    /// `diagnostics()` for a full picture: the session tracks admission and
    /// outcomes, while link health reflects the wire (latency, timeouts,
    /// dropped frames).
    pub fn link_health(&self) -> crate::hal::LinkHealth {
        self.current_radio().link_health()
    }

    async fn wait(
        &self,
        ticket: Result<SessionTicket, SessionError>,
    ) -> anyhow::Result<RadioSnapshot> {
        ticket
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    pub fn submit(&self, operation: SessionOperation) -> Result<SessionTicket, SessionError> {
        if let Err(error) = self.validate(&operation) {
            self.shared
                .events
                .publish(SessionEvent::OperationCompleted {
                    operation,
                    outcome: SessionOutcome::Rejected(error.clone()),
                });
            return Err(error);
        }
        let (sender, receiver) = oneshot::channel();
        let mut state = self.shared.state.lock().map_err(|_| SessionError::Closed)?;
        if state.closed {
            self.shared
                .events
                .publish(SessionEvent::OperationCompleted {
                    operation,
                    outcome: SessionOutcome::Rejected(SessionError::Closed),
                });
            return Err(SessionError::Closed);
        }
        let key = operation.coalesce_key();
        if let Some(key) = key {
            if let Some(existing) = state
                .queue
                .iter_mut()
                .find(|queued| queued.operation.coalesce_key() == Some(key))
            {
                if existing.operation == operation {
                    existing.waiters.push(sender);
                    return Ok(receiver);
                }
                for waiter in existing.waiters.drain(..) {
                    let _ = waiter.send(Err(SessionError::Superseded));
                }
                let superseded = existing.operation.clone();
                existing.operation = operation.clone();
                existing.waiters.push(sender);
                update_desired(&mut state.snapshot.desired, &operation);
                state.snapshot.pending = state
                    .queue
                    .iter()
                    .map(|item| item.operation.clone())
                    .collect();
                self.shared.wake.notify_one();
                state.diagnostics.coalesced = state.diagnostics.coalesced.wrapping_add(1);
                self.shared
                    .events
                    .publish(SessionEvent::OperationCompleted {
                        operation: superseded,
                        outcome: SessionOutcome::Superseded,
                    });
                return Ok(receiver);
            }
        }
        if operation_matches_observed(&state.snapshot, &operation) {
            let _ = sender.send(Ok(state.snapshot.clone()));
            return Ok(receiver);
        }
        if state.queue.len() >= self.shared.queue_capacity {
            self.shared
                .events
                .publish(SessionEvent::OperationCompleted {
                    operation,
                    outcome: SessionOutcome::Rejected(SessionError::QueueFull),
                });
            return Err(SessionError::QueueFull);
        }
        update_desired(&mut state.snapshot.desired, &operation);
        let accepted = operation.clone();
        let queued = QueuedOperation {
            operation,
            generation: state.diagnostics.generation,
            deadline: Instant::now() + DEFAULT_COMMAND_TIMEOUT,
            waiters: vec![sender],
        };
        let index = state
            .queue
            .iter()
            .position(|item| item.operation.priority() > queued.operation.priority())
            .unwrap_or(state.queue.len());
        state.queue.insert(index, queued);
        state.diagnostics.queued = state.diagnostics.queued.wrapping_add(1);
        state.snapshot.pending = state
            .queue
            .iter()
            .map(|item| item.operation.clone())
            .collect();
        self.shared
            .events
            .publish(SessionEvent::OperationAccepted(accepted));
        self.shared.wake.notify_one();
        Ok(receiver)
    }

    fn validate(&self, operation: &SessionOperation) -> Result<(), SessionError> {
        match operation {
            SessionOperation::Refresh => Ok(()),
            SessionOperation::Raw(frame) if frame.is_empty() => Err(SessionError::InvalidFrame(
                "raw frame cannot be empty".into(),
            )),
            SessionOperation::Raw(_) => {
                if self.current_radio().capabilities().can_raw_protocol {
                    Ok(())
                } else {
                    Err(SessionError::Unsupported("raw protocol write".into()))
                }
            }
            SessionOperation::SetFrequency(hz) => {
                if *hz == 0 {
                    return Err(SessionError::Invalid("frequency must be non-zero".into()));
                }
                if self.capabilities().can_set_frequency {
                    Ok(())
                } else {
                    Err(SessionError::Unsupported("frequency write".into()))
                }
            }
            SessionOperation::SetMode(_) if self.capabilities().can_set_mode => Ok(()),
            SessionOperation::SetMode(_) => Err(SessionError::Unsupported("mode write".into())),
            SessionOperation::SetPtt(_) if self.capabilities().can_set_ptt => Ok(()),
            SessionOperation::SetPtt(_) => Err(SessionError::Unsupported("PTT write".into())),
            SessionOperation::SetControl(id, _)
                if self.current_radio().supports_control_write(*id) =>
            {
                Ok(())
            }
            SessionOperation::SetControl(id, _) => {
                Err(SessionError::Unsupported(format!("control {id:?} write")))
            }
        }
    }

    pub fn set_frequency(&self, hz: u64) -> Result<SessionTicket, SessionError> {
        self.submit(SessionOperation::SetFrequency(hz))
    }

    pub fn set_mode(&self, mode: Mode) -> Result<SessionTicket, SessionError> {
        self.submit(SessionOperation::SetMode(mode))
    }

    pub fn set_ptt(&self, enabled: bool) -> Result<SessionTicket, SessionError> {
        self.submit(SessionOperation::SetPtt(enabled))
    }

    pub fn set_control(
        &self,
        id: ControlId,
        value: ControlValue,
    ) -> Result<SessionTicket, SessionError> {
        self.submit(SessionOperation::SetControl(id, value))
    }

    pub fn refresh(&self) -> Result<SessionTicket, SessionError> {
        self.submit(SessionOperation::Refresh)
    }

    pub fn raw(&self, frame: Vec<u8>) -> Result<SessionTicket, SessionError> {
        self.submit(SessionOperation::Raw(frame))
    }

    pub fn reconnect<R>(&self, radio: Arc<R>) -> Result<u64, SessionError>
    where
        R: Radio + 'static,
    {
        let replacement: Arc<dyn Radio> = radio;
        let subscription = replacement.event_router().map(|router| router.subscribe());
        *self.radio.lock().map_err(|_| SessionError::Disconnected)? = replacement;
        *self
            .shared
            .event_subscription
            .lock()
            .map_err(|_| SessionError::Disconnected)? = subscription;
        self.advance_generation()
    }

    /// Advance the session generation after a reconnect or device replacement.
    /// Queued work from the prior connection is rejected before it can reach
    /// the newly attached transport.
    pub fn advance_generation(&self) -> Result<u64, SessionError> {
        let mut state = self.shared.state.lock().map_err(|_| SessionError::Closed)?;
        state.diagnostics.generation = state.diagnostics.generation.wrapping_add(1);
        state.snapshot.generation = state.diagnostics.generation;
        state.snapshot.synchronized = false;
        state.snapshot.status = SessionStatus::Synchronizing;
        for queued in state.queue.drain(..) {
            self.shared
                .events
                .publish(SessionEvent::OperationCompleted {
                    operation: queued.operation,
                    outcome: SessionOutcome::Stale,
                });
            for waiter in queued.waiters {
                let _ = waiter.send(Err(SessionError::StaleGeneration));
            }
        }
        state.snapshot.pending.clear();
        self.shared.wake.notify_one();
        Ok(state.diagnostics.generation)
    }

    pub fn close(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.closed = true;
            state.snapshot.status = SessionStatus::Stopped;
            for queued in state.queue.drain(..) {
                for waiter in queued.waiters {
                    let _ = waiter.send(Err(SessionError::Closed));
                }
            }
            state.snapshot.pending.clear();
            self.shared.wake.notify_all();
        }
    }
}

impl Drop for RadioSession {
    fn drop(&mut self) {
        if Arc::strong_count(&self.shared) == 1 {
            self.close();
        }
    }
}

fn update_desired(state: &mut RadioState, operation: &SessionOperation) {
    match operation {
        SessionOperation::SetFrequency(value) => state.frequency_hz = Some(*value),
        SessionOperation::SetMode(value) => state.mode = Some(*value),
        SessionOperation::SetPtt(value) => state.ptt = Some(*value),
        SessionOperation::SetControl(id, value) => set_control(state, *id, value.clone()),
        SessionOperation::Refresh | SessionOperation::Raw(_) => {}
    }
}

fn update_observed(state: &mut RadioState, operation: &SessionOperation) {
    update_desired(state, operation);
}

fn set_control(state: &mut RadioState, id: ControlId, value: ControlValue) {
    if let Some(existing) = state
        .controls
        .iter_mut()
        .find(|(existing, _)| *existing == id)
    {
        existing.1 = value;
    } else {
        state.controls.push((id, value));
    }
}

fn operation_matches_observed(snapshot: &RadioSnapshot, operation: &SessionOperation) -> bool {
    match operation {
        SessionOperation::SetFrequency(value) => snapshot.observed.frequency_hz == Some(*value),
        SessionOperation::SetMode(value) => snapshot.observed.mode == Some(*value),
        SessionOperation::SetPtt(value) => snapshot.observed.ptt == Some(*value),
        SessionOperation::SetControl(id, value) => snapshot
            .observed
            .controls
            .iter()
            .any(|(existing, current)| existing == id && current == value),
        SessionOperation::Refresh | SessionOperation::Raw(_) => false,
    }
}

fn worker_loop(
    weak: Weak<Shared>,
    radio: Arc<Mutex<Arc<dyn Radio>>>,
    refresh_interval: Option<Duration>,
) {
    if let Some(shared) = weak.upgrade() {
        transition_status(&shared, SessionStatus::Opening);
        transition_status(&shared, SessionStatus::Probing);
        transition_status(&shared, SessionStatus::Synchronizing);
    }
    let mut next_refresh = Instant::now();
    loop {
        let Some(shared) = weak.upgrade() else { break };
        ingest_events(&shared);
        enforce_ptt_watchdog(&shared, &radio);
        let operation = {
            let mut state = match shared.state.lock() {
                Ok(state) => state,
                Err(_) => break,
            };
            if state.closed {
                break;
            }
            if let Some(queued) = state.queue.pop_front() {
                state.snapshot.pending = state
                    .queue
                    .iter()
                    .map(|item| item.operation.clone())
                    .collect();
                Some(queued)
            } else if refresh_interval.is_some_and(|_| Instant::now() >= next_refresh) {
                Some(QueuedOperation {
                    operation: SessionOperation::Refresh,
                    generation: state.diagnostics.generation,
                    deadline: Instant::now() + DEFAULT_COMMAND_TIMEOUT,
                    waiters: Vec::new(),
                })
            } else {
                let timeout = refresh_interval
                    .map(|_| next_refresh.saturating_duration_since(Instant::now()))
                    .unwrap_or(Duration::from_millis(100))
                    .min(Duration::from_millis(100));
                let _ = shared.wake.wait_timeout(state, timeout);
                None
            }
        };
        let Some(queued) = operation else { continue };
        let timed_out = Instant::now() >= queued.deadline;
        let result = if timed_out {
            Err(anyhow::anyhow!("radio session command deadline expired"))
        } else {
            radio
                .lock()
                .map(|radio| execute(&radio, &queued.operation, &shared.state))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("radio session backend lock poisoned")))
        };
        let mut state = match shared.state.lock() {
            Ok(state) => state,
            Err(_) => break,
        };
        if matches!(queued.operation, SessionOperation::Refresh) {
            next_refresh = Instant::now() + refresh_interval.unwrap_or(Duration::from_secs(3600));
        }
        match result {
            Ok(observed) => {
                if queued.generation != state.diagnostics.generation {
                    for waiter in queued.waiters {
                        let _ = waiter.send(Err(SessionError::StaleGeneration));
                    }
                    shared.events.publish(SessionEvent::OperationCompleted {
                        operation: queued.operation,
                        outcome: SessionOutcome::Stale,
                    });
                    continue;
                }
                if let Some(observed) = observed {
                    state.snapshot.observed = observed;
                } else {
                    update_observed(&mut state.snapshot.observed, &queued.operation);
                }
                // Track the start of a transmit hold so the safety watchdog
                // can force PTT off if the client never will.
                if let SessionOperation::SetPtt(enabled) = queued.operation {
                    state.ptt_on_since = if enabled { Some(Instant::now()) } else { None };
                }
                state.snapshot.status = SessionStatus::Ready;
                state.snapshot.synchronized = true;
                state.snapshot.last_error = None;
                state.snapshot.sequence = state.snapshot.sequence.wrapping_add(1);
                state.diagnostics.completed = state.diagnostics.completed.wrapping_add(1);
                let snapshot = state.snapshot.clone();
                for waiter in queued.waiters {
                    let _ = waiter.send(Ok(snapshot.clone()));
                }
                shared
                    .events
                    .publish(SessionEvent::SnapshotChanged(snapshot));
                shared.events.publish(SessionEvent::OperationCompleted {
                    operation: queued.operation,
                    outcome: SessionOutcome::Applied,
                });
            }
            Err(error) => {
                if queued.generation != state.diagnostics.generation {
                    for waiter in queued.waiters {
                        let _ = waiter.send(Err(SessionError::StaleGeneration));
                    }
                    shared.events.publish(SessionEvent::OperationCompleted {
                        operation: queued.operation,
                        outcome: SessionOutcome::Stale,
                    });
                    continue;
                }
                state.snapshot.status = SessionStatus::Recovering;
                state.snapshot.synchronized = false;
                state.snapshot.last_error = Some(error.to_string());
                state.diagnostics.failed = state.diagnostics.failed.wrapping_add(1);
                state.diagnostics.recoveries = state.diagnostics.recoveries.wrapping_add(1);
                let error = if timed_out {
                    SessionError::TimedOut
                } else {
                    SessionError::Backend(error.to_string())
                };
                for waiter in queued.waiters {
                    let _ = waiter.send(Err(error.clone()));
                }
                shared
                    .events
                    .publish(SessionEvent::SnapshotChanged(state.snapshot.clone()));
                shared.events.publish(SessionEvent::OperationCompleted {
                    operation: queued.operation,
                    outcome: SessionOutcome::Failed(error),
                });
            }
        }
    }
}

fn transition_status(shared: &Shared, status: SessionStatus) {
    if let Ok(mut state) = shared.state.lock() {
        if state.closed || state.snapshot.status == status {
            return;
        }
        state.snapshot.status = status;
        state.snapshot.sequence = state.snapshot.sequence.wrapping_add(1);
        shared
            .events
            .publish(SessionEvent::SnapshotChanged(state.snapshot.clone()));
    }
}

/// Force PTT off when the transmitter has been held longer than the
/// configured safety ceiling. This is a defensive net for a crashed or stuck
/// client: it issues `SetPtt(false)` directly rather than queueing it, so a
/// full or stalled command queue cannot delay the shutoff.
fn enforce_ptt_watchdog(shared: &Shared, radio: &Arc<Mutex<Arc<dyn Radio>>>) {
    let Some(max_hold) = shared.max_tx_hold else {
        return;
    };
    let exceeded = shared
        .state
        .lock()
        .ok()
        .and_then(|state| state.ptt_on_since)
        .is_some_and(|since| since.elapsed() >= max_hold);
    if !exceeded {
        return;
    }
    let result = radio
        .lock()
        .map(|radio| futures::executor::block_on(radio.set_ptt(false)))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("radio session backend lock poisoned")));
    if let Ok(mut state) = shared.state.lock() {
        state.ptt_on_since = None;
        state.snapshot.observed.ptt = Some(false);
        state.snapshot.sequence = state.snapshot.sequence.wrapping_add(1);
        match result {
            Ok(()) => {
                shared.events.publish(SessionEvent::PttWatchdogTripped);
                shared
                    .events
                    .publish(SessionEvent::SnapshotChanged(state.snapshot.clone()));
            }
            Err(error) => {
                state.snapshot.last_error = Some(format!(
                    "PTT watchdog failed to drop the transmitter: {error}"
                ));
                shared
                    .events
                    .publish(SessionEvent::SnapshotChanged(state.snapshot.clone()));
            }
        }
    }
}

fn ingest_events(shared: &Shared) {
    let events = shared
        .event_subscription
        .lock()
        .ok()
        .and_then(|subscription| {
            subscription
                .as_ref()
                .map(|subscription| subscription.drain())
        })
        .unwrap_or_default();
    if events.is_empty() {
        return;
    }
    if let Ok(mut state) = shared.state.lock() {
        for event in events {
            match event {
                RadioEvent::FrequencyChanged { frequency_hz } => {
                    state.snapshot.observed.frequency_hz = Some(frequency_hz)
                }
                RadioEvent::ModeChanged { mode } => state.snapshot.observed.mode = Some(mode),
                RadioEvent::PttChanged { enabled } => {
                    state.snapshot.observed.ptt = Some(enabled);
                    state.ptt_on_since = if enabled { Some(Instant::now()) } else { None };
                }
                RadioEvent::ControlChanged { id, value } => {
                    set_control(&mut state.snapshot.observed, id, value)
                }
                RadioEvent::MeterChanged { .. }
                | RadioEvent::ReceiverChanged { .. }
                | RadioEvent::Raw { .. } => {}
            }
        }
        state.snapshot.sequence = state.snapshot.sequence.wrapping_add(1);
        shared
            .events
            .publish(SessionEvent::SnapshotChanged(state.snapshot.clone()));
    }
}

fn execute(
    radio: &Arc<dyn Radio>,
    operation: &SessionOperation,
    shared_state: &Mutex<SharedState>,
) -> anyhow::Result<Option<RadioState>> {
    match operation {
        SessionOperation::Refresh => {
            // When the radio's unsolicited event stream is live and we
            // already hold observed core state, the streamed values are at
            // least as current as a fresh poll, so serve the refresh from
            // cache without touching the wire. This makes Icom CI-V
            // refreshes effectively free on a healthy link while a stalled
            // stream falls straight back to polling.
            let stream_live = radio
                .event_stream_age()
                .is_some_and(|age| age <= EVENT_STREAM_FRESHNESS);
            if stream_live {
                if let Ok(state) = shared_state.lock() {
                    let observed = &state.snapshot.observed;
                    if observed.frequency_hz.is_some() || observed.mode.is_some() {
                        return Ok(Some(observed.clone()));
                    }
                }
            }
            // Prefer the backend's batched core-state read (e.g. the Yaesu
            // `IF;` frame) so a refresh costs the fewest round trips the
            // protocol allows; the default falls back to individual reads.
            let core = futures::executor::block_on(radio.read_core_state())?;
            let observed = RadioState {
                frequency_hz: core.frequency_hz,
                mode: core.mode,
                ptt: core.ptt,
                ..RadioState::default()
            };
            Ok(Some(observed))
        }
        SessionOperation::SetFrequency(value) => {
            futures::executor::block_on(radio.set_frequency_hz(*value))?;
            Ok(None)
        }
        SessionOperation::SetMode(value) => {
            futures::executor::block_on(radio.set_mode(*value))?;
            Ok(None)
        }
        SessionOperation::SetPtt(value) => {
            futures::executor::block_on(radio.set_ptt(*value))?;
            Ok(None)
        }
        SessionOperation::SetControl(id, value) => {
            futures::executor::block_on(radio.set_control(*id, value.clone()))?;
            Ok(None)
        }
        SessionOperation::Raw(frame) => {
            futures::executor::block_on(radio.protocol_write_read(frame))?;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoreState, RadioEventRouter};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeRadio {
        frequency: Mutex<u64>,
        mode: Mutex<Mode>,
        ptt: Mutex<bool>,
        writes: AtomicUsize,
        core_reads: AtomicUsize,
        stream_age: Mutex<Option<Duration>>,
        events: RadioEventRouter,
    }

    #[async_trait::async_trait]
    impl Radio for FakeRadio {
        fn event_router(&self) -> Option<RadioEventRouter> {
            Some(self.events.clone())
        }
        fn event_stream_age(&self) -> Option<Duration> {
            *self.stream_age.lock().unwrap()
        }
        async fn read_core_state(&self) -> anyhow::Result<CoreState> {
            self.core_reads.fetch_add(1, Ordering::SeqCst);
            Ok(CoreState {
                frequency_hz: Some(*self.frequency.lock().unwrap()),
                mode: Some(*self.mode.lock().unwrap()),
                ptt: Some(*self.ptt.lock().unwrap()),
            })
        }
        async fn get_frequency_hz(&self) -> anyhow::Result<u64> {
            Ok(*self.frequency.lock().unwrap())
        }
        async fn set_frequency_hz(&self, value: u64) -> anyhow::Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.frequency.lock().unwrap() = value;
            Ok(())
        }
        async fn get_mode(&self) -> anyhow::Result<Mode> {
            Ok(*self.mode.lock().unwrap())
        }
        async fn set_mode(&self, value: Mode) -> anyhow::Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.mode.lock().unwrap() = value;
            Ok(())
        }
        async fn set_ptt(&self, value: bool) -> anyhow::Result<()> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            *self.ptt.lock().unwrap() = value;
            Ok(())
        }
        async fn get_ptt(&self) -> anyhow::Result<bool> {
            Ok(*self.ptt.lock().unwrap())
        }
        async fn protocol_write_read(&self, _request: &[u8]) -> anyhow::Result<Vec<u8>> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
        fn capabilities(&self) -> RadioCapabilities {
            RadioCapabilities {
                can_get_frequency: true,
                can_set_frequency: true,
                can_get_mode: true,
                can_set_mode: true,
                can_get_ptt: true,
                can_set_ptt: true,
                can_raw_protocol: true,
                ..RadioCapabilities::default()
            }
        }
    }

    fn fake() -> Arc<FakeRadio> {
        Arc::new(FakeRadio {
            frequency: Mutex::new(14_074_000),
            mode: Mutex::new(Mode::Usb),
            ptt: Mutex::new(false),
            writes: AtomicUsize::new(0),
            core_reads: AtomicUsize::new(0),
            stream_age: Mutex::new(None),
            events: RadioEventRouter::default(),
        })
    }

    struct OptionalSurfaceRadio;

    #[async_trait::async_trait]
    impl Radio for OptionalSurfaceRadio {
        async fn get_frequency_hz(&self) -> anyhow::Result<u64> {
            Ok(14_074_000)
        }

        async fn set_frequency_hz(&self, _hz: u64) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_mode(&self) -> anyhow::Result<Mode> {
            Ok(Mode::Usb)
        }

        async fn set_mode(&self, _mode: Mode) -> anyhow::Result<()> {
            Ok(())
        }

        async fn set_ptt(&self, _enabled: bool) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_power(&self) -> anyhow::Result<bool> {
            Ok(true)
        }

        async fn protocol_write_read(&self, _request: &[u8]) -> anyhow::Result<Vec<u8>> {
            Ok(vec![0xAA])
        }

        async fn get_control(&self, _id: ControlId) -> anyhow::Result<Option<ControlValue>> {
            Ok(Some(ControlValue::U8(7)))
        }

        fn supports_scope(&self) -> bool {
            true
        }

        fn supports_iq_output(&self) -> bool {
            true
        }

        fn swr_sweep_setup(&self) -> Option<crate::SwrSweepSetup> {
            Some(crate::SwrSweepSetup {
                carrier_mode: Mode::Rtty,
                rf_power: 1,
            })
        }

        async fn get_meter(&self, _id: crate::MeterId) -> anyhow::Result<Option<u8>> {
            Ok(Some(42))
        }

        fn supports_meter(&self, _id: crate::MeterId) -> bool {
            true
        }

        fn supports_control(&self, _id: ControlId) -> bool {
            true
        }

        fn capabilities(&self) -> RadioCapabilities {
            RadioCapabilities {
                can_get_frequency: true,
                can_set_frequency: true,
                can_get_mode: true,
                can_set_mode: true,
                can_get_ptt: false,
                can_set_ptt: true,
                can_get_power: true,
                can_set_power: false,
                can_raw_protocol: true,
            }
        }
    }

    #[test]
    fn session_forwards_optional_radio_surface_without_trait_defaults() {
        let session = RadioSession::from_radio(
            Arc::new(OptionalSurfaceRadio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();

        assert!(Radio::supports_scope(&session));
        assert!(Radio::supports_iq_output(&session));
        assert_eq!(Radio::swr_sweep_setup(&session).unwrap().rf_power, 1);
        assert!(Radio::supports_meter(&session, crate::MeterId::Signal));
        assert_eq!(
            futures::executor::block_on(Radio::get_meter(&session, crate::MeterId::Signal))
                .unwrap(),
            Some(42)
        );
        assert_eq!(
            futures::executor::block_on(Radio::get_control(&session, ControlId::RfPower)).unwrap(),
            Some(ControlValue::U8(7))
        );
        assert_eq!(
            futures::executor::block_on(Radio::protocol_write_read(&session, &[0x01])).unwrap(),
            vec![0xAA]
        );
        assert!(futures::executor::block_on(Radio::get_power(&session)).unwrap());
        assert!(Radio::capabilities(&session).can_raw_protocol);
    }

    #[test]
    fn coalesces_rapid_frequency_intent_before_transport() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        let first = session.set_frequency(1).unwrap();
        let second = session.set_frequency(2).unwrap();
        assert_eq!(
            futures::executor::block_on(first).unwrap(),
            Err(SessionError::Superseded)
        );
        let result = futures::executor::block_on(second).unwrap().unwrap();
        assert_eq!(result.observed.frequency_hz, Some(2));
        assert_eq!(radio.writes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn bounded_admission_rejects_distinct_work_when_full() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 1,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        let frequency = session.set_frequency(1).unwrap();
        let mode = session.set_mode(Mode::Cw);
        if let Ok(ptt) = session.set_ptt(true) {
            futures::executor::block_on(ptt).unwrap().unwrap();
        }
        futures::executor::block_on(frequency).unwrap().unwrap();
        if let Ok(mode) = mode {
            futures::executor::block_on(mode).unwrap().unwrap();
        }
    }

    #[test]
    fn event_router_updates_observed_state_without_client_polling() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        radio.events.publish(RadioEvent::FrequencyChanged {
            frequency_hz: 7_000_000,
        });
        for _ in 0..500 {
            if session.snapshot().observed.frequency_hz == Some(7_000_000) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(session.snapshot().observed.frequency_hz, Some(7_000_000));
    }

    #[test]
    fn raw_commands_are_admitted_without_coalescing() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        let first = session.raw(vec![0xFE, 0xFD]).unwrap();
        let second = session.raw(vec![0xFE, 0xFD]).unwrap();
        futures::executor::block_on(first).unwrap().unwrap();
        futures::executor::block_on(second).unwrap().unwrap();
        assert_eq!(radio.writes.load(Ordering::SeqCst), 2);
        assert_eq!(
            SessionOperation::Raw(vec![1]).class(),
            SessionCommandClass::Raw
        );
        assert!(matches!(
            session.raw(Vec::new()),
            Err(SessionError::InvalidFrame(_))
        ));
        assert_eq!(session.diagnostics().completed, 2);
    }

    #[test]
    fn ptt_watchdog_forces_transmitter_off_after_the_hold_ceiling() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                // Refresh must fire for the worker loop to keep cycling and
                // run the watchdog check between operations.
                refresh_interval: Some(Duration::from_millis(10)),
                max_tx_hold: Some(Duration::from_millis(50)),
            },
        )
        .unwrap();
        // Key the transmitter on through the normal admission path.
        futures::executor::block_on(session.set_ptt(true).unwrap())
            .unwrap()
            .unwrap();
        assert!(session.snapshot().observed.ptt == Some(true));

        // With no client action, the watchdog must drop PTT once the ceiling
        // elapses, even though nothing else is queued.
        let mut dropped = false;
        for _ in 0..1000 {
            if session.snapshot().observed.ptt == Some(false) {
                dropped = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(dropped, "watchdog did not force PTT off");
    }

    #[test]
    fn ptt_watchdog_leaves_short_transmissions_alone() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: Some(Duration::from_millis(10)),
                // Generous ceiling: a brief keyed period must not trip it.
                max_tx_hold: Some(Duration::from_secs(60)),
            },
        )
        .unwrap();
        futures::executor::block_on(session.set_ptt(true).unwrap())
            .unwrap()
            .unwrap();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(session.snapshot().observed.ptt, Some(true));
        // A clean PTT-off still works and clears the hold timestamp.
        futures::executor::block_on(session.set_ptt(false).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(session.snapshot().observed.ptt, Some(false));
    }

    #[test]
    fn refresh_trusts_a_live_event_stream_instead_of_polling() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        // Establish baseline observed state via one polled refresh.
        futures::executor::block_on(session.refresh().unwrap())
            .unwrap()
            .unwrap();
        let reads_after_first = radio.core_reads.load(Ordering::SeqCst);
        assert!(reads_after_first >= 1);

        // Mark the event stream as freshly live; subsequent refreshes must be
        // served from streamed state without another wire read.
        *radio.stream_age.lock().unwrap() = Some(Duration::from_millis(50));
        futures::executor::block_on(session.refresh().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            radio.core_reads.load(Ordering::SeqCst),
            reads_after_first,
            "a live event stream should not trigger another wire read"
        );

        // A stalled stream falls back to polling.
        *radio.stream_age.lock().unwrap() = Some(Duration::from_secs(60));
        futures::executor::block_on(session.refresh().unwrap())
            .unwrap()
            .unwrap();
        assert!(radio.core_reads.load(Ordering::SeqCst) > reads_after_first);
    }

    #[test]
    fn one_thousand_state_writes_collapse_to_the_latest_intent() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        let mut latest = None;
        for value in 1..=1_000 {
            let ticket = session.set_frequency(value).unwrap();
            latest = Some(ticket);
        }
        assert_eq!(
            futures::executor::block_on(latest.unwrap())
                .unwrap()
                .unwrap()
                .observed
                .frequency_hz,
            Some(1_000)
        );
        assert!(radio.writes.load(Ordering::SeqCst) < 1_000);
    }

    #[test]
    fn generation_advance_invalidates_queued_work_and_marks_unsynchronized() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        let first = session.set_frequency(1).unwrap();
        let second = session.set_mode(Mode::Cw).unwrap();
        let generation = session.advance_generation().unwrap();
        assert_eq!(generation, 1);
        assert_eq!(session.snapshot().generation, 1);
        assert!(!session.snapshot().synchronized);
        assert!(matches!(
            futures::executor::block_on(second).unwrap(),
            Err(SessionError::StaleGeneration)
        ));
        let _ = futures::executor::block_on(first);
    }

    #[test]
    fn reconnect_replaces_backend_before_new_work_is_executed() {
        let original = fake();
        let replacement = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&original),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        assert_eq!(session.reconnect(Arc::clone(&replacement)).unwrap(), 1);
        let _ = futures::executor::block_on(session.set_frequency(7_000_000).unwrap()).unwrap();
        assert_eq!(*replacement.frequency.lock().unwrap(), 7_000_000);
        assert_eq!(*original.frequency.lock().unwrap(), 14_074_000);
        assert_eq!(session.diagnostics().generation, 1);
    }

    #[test]
    fn reconnect_switches_unsolicited_event_source() {
        let original = fake();
        let replacement = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&original),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        session.reconnect(Arc::clone(&replacement)).unwrap();
        replacement.events.publish(RadioEvent::FrequencyChanged {
            frequency_hz: 7_100_000,
        });
        for _ in 0..500 {
            if session.snapshot().observed.frequency_hz == Some(7_100_000) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(session.snapshot().observed.frequency_hz, Some(7_100_000));
        original.events.publish(RadioEvent::FrequencyChanged {
            frequency_hz: 3_500_000,
        });
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(session.snapshot().observed.frequency_hz, Some(7_100_000));
    }

    #[test]
    fn rejected_commands_are_reported_to_session_subscribers() {
        let session = RadioSession::from_radio(
            fake(),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
                max_tx_hold: None,
            },
        )
        .unwrap();
        let events = session.events();
        let subscription = events.subscribe();
        assert!(matches!(
            session.raw(Vec::new()),
            Err(SessionError::InvalidFrame(_))
        ));
        assert!(subscription.drain().iter().any(|event| matches!(
            event,
            SessionEvent::OperationCompleted {
                outcome: SessionOutcome::Rejected(SessionError::InvalidFrame(_)),
                ..
            }
        )));
    }
}
