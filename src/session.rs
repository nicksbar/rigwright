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
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
            refresh_interval: Some(DEFAULT_REFRESH_INTERVAL),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    SnapshotChanged(RadioSnapshot),
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
    waiters: Vec<oneshot::Sender<Result<RadioSnapshot, SessionError>>>,
}

struct SharedState {
    queue: VecDeque<QueuedOperation>,
    snapshot: RadioSnapshot,
    closed: bool,
    diagnostics: SessionDiagnostics,
}

struct Shared {
    state: Mutex<SharedState>,
    wake: Condvar,
    events: SessionEventRouter,
    queue_capacity: usize,
}

pub struct RadioSession {
    radio: Arc<dyn Radio>,
    shared: Arc<Shared>,
}

#[async_trait::async_trait]
impl Radio for RadioSession {
    fn event_router(&self) -> Option<crate::RadioEventRouter> {
        self.radio.event_router()
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

    async fn set_control(&self, id: ControlId, value: ControlValue) -> anyhow::Result<()> {
        self.wait(self.set_control(id, value)).await.map(|_| ())
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.radio.supports_control(id)
    }

    fn supports_control_read(&self, id: ControlId) -> bool {
        self.radio.supports_control_read(id)
    }

    fn supports_control_write(&self, id: ControlId) -> bool {
        self.radio.supports_control_write(id)
    }

    fn capabilities(&self) -> RadioCapabilities {
        self.radio.capabilities()
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
            }),
            wake: Condvar::new(),
            events: SessionEventRouter::default(),
            queue_capacity: config.queue_capacity,
        });
        let weak = Arc::downgrade(&shared);
        let worker_radio = Arc::clone(&radio);
        let event_subscription = radio.event_router().map(|router| router.subscribe());
        thread::Builder::new()
            .name("rigwright-radio-session".to_string())
            .spawn(move || {
                worker_loop(
                    weak,
                    worker_radio,
                    config.refresh_interval,
                    event_subscription,
                )
            })
            .map_err(|error| SessionError::Backend(error.to_string()))?;
        Ok(Self { radio, shared })
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
        self.radio.capabilities()
    }

    pub fn diagnostics(&self) -> SessionDiagnostics {
        self.shared
            .state
            .lock()
            .map(|state| state.diagnostics)
            .unwrap_or_default()
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
        self.validate(&operation)?;
        let (sender, receiver) = oneshot::channel();
        let mut state = self.shared.state.lock().map_err(|_| SessionError::Closed)?;
        if state.closed {
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
                return Ok(receiver);
            }
        }
        if operation_matches_observed(&state.snapshot, &operation) {
            let _ = sender.send(Ok(state.snapshot.clone()));
            return Ok(receiver);
        }
        if state.queue.len() >= self.shared.queue_capacity {
            return Err(SessionError::QueueFull);
        }
        update_desired(&mut state.snapshot.desired, &operation);
        let queued = QueuedOperation {
            operation,
            generation: state.diagnostics.generation,
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
                if self.radio.capabilities().can_raw_protocol {
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
            SessionOperation::SetControl(id, _) if self.radio.supports_control_write(*id) => Ok(()),
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
    radio: Arc<dyn Radio>,
    refresh_interval: Option<Duration>,
    event_subscription: Option<crate::RadioEventSubscription>,
) {
    let mut next_refresh = Instant::now();
    loop {
        let Some(shared) = weak.upgrade() else { break };
        ingest_events(&shared, event_subscription.as_ref());
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
        let result = execute(&radio, &queued.operation);
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
                    continue;
                }
                if let Some(observed) = observed {
                    state.snapshot.observed = observed;
                } else {
                    update_observed(&mut state.snapshot.observed, &queued.operation);
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
            }
            Err(error) => {
                if queued.generation != state.diagnostics.generation {
                    for waiter in queued.waiters {
                        let _ = waiter.send(Err(SessionError::StaleGeneration));
                    }
                    continue;
                }
                state.snapshot.status = SessionStatus::Recovering;
                state.snapshot.synchronized = false;
                state.snapshot.last_error = Some(error.to_string());
                state.diagnostics.failed = state.diagnostics.failed.wrapping_add(1);
                state.diagnostics.recoveries = state.diagnostics.recoveries.wrapping_add(1);
                let error = SessionError::Backend(error.to_string());
                for waiter in queued.waiters {
                    let _ = waiter.send(Err(error.clone()));
                }
                shared
                    .events
                    .publish(SessionEvent::SnapshotChanged(state.snapshot.clone()));
            }
        }
    }
}

fn ingest_events(shared: &Shared, subscription: Option<&crate::RadioEventSubscription>) {
    let Some(subscription) = subscription else {
        return;
    };
    let events = subscription.drain();
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
                RadioEvent::PttChanged { enabled } => state.snapshot.observed.ptt = Some(enabled),
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
) -> anyhow::Result<Option<RadioState>> {
    match operation {
        SessionOperation::Refresh => {
            let observed = RadioState {
                frequency_hz: futures::executor::block_on(radio.get_frequency_hz()).ok(),
                mode: futures::executor::block_on(radio.get_mode()).ok(),
                ptt: futures::executor::block_on(radio.get_ptt()).ok(),
                ..RadioState::default()
            };
            if observed.frequency_hz.is_none() && observed.mode.is_none() {
                anyhow::bail!("radio refresh returned no readable core state")
            }
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
    use crate::RadioEventRouter;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeRadio {
        frequency: Mutex<u64>,
        mode: Mutex<Mode>,
        ptt: Mutex<bool>,
        writes: AtomicUsize,
        events: RadioEventRouter,
    }

    #[async_trait::async_trait]
    impl Radio for FakeRadio {
        fn event_router(&self) -> Option<RadioEventRouter> {
            Some(self.events.clone())
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
            events: RadioEventRouter::default(),
        })
    }

    #[test]
    fn coalesces_rapid_frequency_intent_before_transport() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
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
            },
        )
        .unwrap();
        let _frequency = session.set_frequency(1).unwrap();
        assert!(matches!(
            session.set_mode(Mode::Cw),
            Err(SessionError::QueueFull)
        ));
    }

    #[test]
    fn event_router_updates_observed_state_without_client_polling() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
            },
        )
        .unwrap();
        radio.events.publish(RadioEvent::FrequencyChanged {
            frequency_hz: 7_000_000,
        });
        std::thread::sleep(Duration::from_millis(10));
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
    fn one_thousand_state_writes_collapse_to_the_latest_intent() {
        let radio = fake();
        let session = RadioSession::from_radio(
            Arc::clone(&radio),
            SessionConfig {
                queue_capacity: 4,
                refresh_interval: None,
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
}
