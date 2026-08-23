//! Protocol-neutral radio hardware abstraction layer.

pub use crate::hal_types::{BaseMode, ControlId, ControlValue, Mode, OperatingMode};
use anyhow::Result;
use async_trait::async_trait;

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

#[async_trait]
/// Protocol-neutral radio control interface.
///
/// Methods are asynchronous at the API boundary so network and serial drivers
/// can share one interface. Native serial backends currently perform blocking
/// I/O internally; applications should avoid calling them while holding UI or
/// other latency-sensitive locks.
pub trait Radio: Send + Sync {
    async fn get_frequency_hz(&self) -> Result<u64>;
    async fn set_frequency_hz(&self, hz: u64) -> Result<()>;
    async fn get_mode(&self) -> Result<Mode>;
    async fn set_mode(&self, mode: Mode) -> Result<()>;
    async fn set_ptt(&self, enabled: bool) -> Result<()>;
    async fn get_ptt(&self) -> Result<bool> {
        anyhow::bail!("reading PTT state is not supported by this radio")
    }
    async fn get_power(&self) -> Result<bool> {
        anyhow::bail!("reading radio power state is not supported by this radio")
    }
    async fn set_power(&self, _enabled: bool) -> Result<()> {
        anyhow::bail!("setting radio power state is not supported by this radio")
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
