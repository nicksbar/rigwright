//! Protocol-neutral amateur-radio control with native vendor drivers.

pub mod controls;
pub mod drivers;
pub mod dxlab;
pub mod hal;
pub mod hal_types;
pub mod icom;
pub mod kenwood;
pub mod models;
pub mod protocol;
pub mod rigctld;
pub mod yaesu;

pub use hal::{NullRadio, Radio, RadioCapabilities, RadioHal, RadioStatus};
pub use hal_types::{BaseMode, ControlId, ControlValue, Mode, OperatingMode};
pub use icom::civ_radio::{
    enumerate_serial_port_descriptors, enumerate_serial_ports, IcomCiVRadio, IcomReceiver, IcomVfo,
    SerialPortDescriptor,
};
pub use kenwood::KenwoodCatRadio;
pub use models::{IcomCivModel, KenwoodCatModel, YaesuCatModel, YaesuLegacyModel};
pub use protocol::yaesu_legacy_cat::{
    FrequencyModeStatus as YaesuLegacyFrequencyModeStatus, LegacyMode as YaesuLegacyMode,
    RxStatus as YaesuLegacyRxStatus, TxStatus as YaesuLegacyTxStatus,
};
pub use yaesu::{LegacyYaesuRadio, YaesuCatRadio};
