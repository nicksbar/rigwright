//! Protocol-neutral amateur-radio control with native vendor drivers.

pub mod android;
pub mod controls;
pub mod drivers;
pub mod dxlab;
pub mod elecraft;
pub mod events;
pub mod hal;
pub mod hal_types;
pub mod icom;
pub mod iq;
pub mod kenwood;
pub mod models;
pub mod probe;
pub mod protocol;
pub mod rigctld;
pub mod session;
pub mod transport;
pub mod yaesu;

pub use android::RadioAndroid;
pub use elecraft::profile::ElecraftModel;
pub use elecraft::transport::{ElecraftSerialPolicy, ElecraftTransportMetrics};
pub use elecraft::ElecraftRadio;
pub use events::{RadioEvent, RadioEventRouter, RadioEventSubscription, SubscriptionId};
pub use hal::{LinkHealth, NullRadio, Radio, RadioCapabilities, RadioStatus};
pub use hal_types::{
    denormalize_meter_level, normalize_meter_level, BaseMode, ControlId, ControlValue, CoreState,
    DtmfSequence, FilterBandwidth, MemoryChannel, MeterId, MeterPollSpec, MeterPresentation, Mode,
    OperatingMode, RepeaterSettings, RepeaterShift, ScopeCenterType, ScopeColor,
    ScopeConfiguration, ScopeEdgeBank, ScopeMarkerPosition, ScopeMaxHold, ScopeMetadata,
    ScopeState, ScopeWaveformType, SwrSweepSetup, ToneMode, ToneSettings, TunerStatus,
};
pub use icom::civ_radio::{
    enumerate_serial_port_descriptors, enumerate_serial_ports, IcomCiVRadio, IcomProbeResult,
    IcomReceiver, IcomSerialPolicy, IcomTransportMetrics, IcomVfo, ScopeStreamHealth,
    SerialPortDescriptor,
};
pub use iq::{decode_interleaved_iq, IqSampleBlock, IqSampleFormat};
pub use kenwood::cat_radio::{KenwoodSerialPolicy, KenwoodTransportMetrics};
pub use kenwood::KenwoodCatRadio;
pub use models::{IcomCivModel, KenwoodCatModel, YaesuCatModel, YaesuLegacyModel};
pub use protocol::yaesu_legacy_cat::{
    FrequencyModeStatus as YaesuLegacyFrequencyModeStatus, LegacyMode as YaesuLegacyMode,
    RxStatus as YaesuLegacyRxStatus, TxStatus as YaesuLegacyTxStatus,
};
pub use session::{
    RadioSession, RadioSnapshot, RadioState, SessionCommandClass, SessionConfig,
    SessionDiagnostics, SessionError, SessionEvent, SessionEventRouter, SessionEventSubscription,
    SessionOperation, SessionOutcome, SessionStatus, SessionTicket,
};
pub use transport::RadioTransport;
pub use yaesu::cat_radio::{YaesuSerialPolicy, YaesuTransportMetrics};
pub use yaesu::legacy_radio::{LegacyYaesuSerialPolicy, LegacyYaesuTransportMetrics};
pub use yaesu::{LegacyYaesuRadio, YaesuCatRadio};
