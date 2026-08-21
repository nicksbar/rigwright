//! Model-aware serial drivers and factory.

use std::{
    io::{Read, Write},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use crate::{
    models::{find_model, Protocol},
    protocol::{ascii_cat, yaesu_legacy_cat},
    Mode, RadioCapabilities, RadioHal,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiCatFlavor {
    Yaesu,
    Kenwood,
}

#[derive(Debug, Clone)]
pub struct AsciiCatRadio {
    port: String,
    baud_rate: u32,
    flavor: AsciiCatFlavor,
}

impl AsciiCatRadio {
    pub fn new(port: impl Into<String>, baud_rate: u32, flavor: AsciiCatFlavor) -> Self {
        Self {
            port: port.into(),
            baud_rate,
            flavor,
        }
    }

    fn transact(&self, request: &[u8], expect_response: bool) -> Result<Vec<u8>> {
        if self.port.trim().is_empty() {
            bail!("a serial port is required for CAT control");
        }
        let mut port = serialport::new(&self.port, self.baud_rate)
            .timeout(Duration::from_millis(750))
            .open()
            .with_context(|| format!("failed to open CAT serial port {}", self.port))?;
        port.write_all(request)
            .context("failed to write CAT command")?;
        port.flush().context("failed to flush CAT command")?;
        if !expect_response {
            return Ok(Vec::new());
        }
        let mut response = Vec::with_capacity(32);
        let mut byte = [0_u8; 1];
        loop {
            port.read_exact(&mut byte)
                .context("failed to read CAT response")?;
            response.push(byte[0]);
            if byte[0] == b';' {
                return Ok(response);
            }
            if response.len() >= 128 {
                bail!("CAT response exceeded 128 bytes");
            }
        }
    }

    fn command(&self, command: &str, parameter: Option<&str>, response: bool) -> Result<Vec<u8>> {
        self.transact(&ascii_cat::encode(command, parameter)?, response)
    }

    /// Send a documented FTDX10 CAT set command.
    pub fn ftdx10_set(&self, command: &str, parameters: &str) -> Result<()> {
        self.transact(
            &crate::yaesu::ftdx10::command(command, Some(parameters))?,
            false,
        )?;
        Ok(())
    }

    pub fn ftdx10_read(&self, command: &str) -> Result<Vec<u8>> {
        self.transact(&crate::yaesu::ftdx10::read(command)?, true)
    }

    pub fn set_ftdx10_scope(&self, command: Vec<u8>) -> Result<()> {
        self.transact(&command, false)?;
        Ok(())
    }

    /// Read the FTDX10 RF power setting (5-100 percent).
    pub fn get_ftdx10_power_percent(&self) -> Result<u8> {
        let response = self.transact(crate::yaesu::ftdx10::read_power_percent(), true)?;
        crate::yaesu::ftdx10::parse_power_percent(&response)
    }

    /// Set the FTDX10 RF power setting (5-100 percent).
    pub fn set_ftdx10_power_percent(&self, percent: u8) -> Result<()> {
        self.transact(&crate::yaesu::ftdx10::set_power_percent(percent)?, false)?;
        Ok(())
    }

    /// Read the FTDX10 split state.  A quick-split response is considered on.
    pub fn get_ftdx10_split(&self) -> Result<bool> {
        let response = self.transact(crate::yaesu::ftdx10::read_split(), true)?;
        crate::yaesu::ftdx10::parse_split(&response)
    }

    /// Enable or disable FTDX10 split operation.
    pub fn set_ftdx10_split(&self, enabled: bool) -> Result<()> {
        self.transact(&crate::yaesu::ftdx10::set_split(enabled), false)?;
        Ok(())
    }
}

#[async_trait]
impl RadioHal for AsciiCatRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        let response = self.command("FA", None, true)?;
        let text = std::str::from_utf8(&response).context("CAT frequency response is not ASCII")?;
        text.strip_prefix("FA")
            .and_then(|v| v.strip_suffix(';'))
            .context("unexpected CAT frequency response")?
            .parse()
            .context("invalid CAT frequency")
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        let width = if self.flavor == AsciiCatFlavor::Yaesu {
            9
        } else {
            11
        };
        self.command("FA", Some(&format!("{hz:0width$}")), false)?;
        Ok(())
    }

    async fn get_mode(&self) -> Result<Mode> {
        let response = self.command("MD", None, true)?;
        let text = std::str::from_utf8(&response).context("CAT mode response is not ASCII")?;
        let raw = text
            .strip_prefix("MD")
            .and_then(|v| v.strip_suffix(';'))
            .context("unexpected CAT mode response")?;
        decode_ascii_mode(self.flavor, raw)
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let value = match (self.flavor, mode) {
            (AsciiCatFlavor::Yaesu, Mode::Lsb) => "01",
            (AsciiCatFlavor::Yaesu, Mode::Usb) => "02",
            (AsciiCatFlavor::Yaesu, Mode::Cw) => "03",
            (AsciiCatFlavor::Yaesu, Mode::Data) => "0C",
            (AsciiCatFlavor::Kenwood, Mode::Lsb) => "1",
            (AsciiCatFlavor::Kenwood, Mode::Usb | Mode::Data) => "2",
            (AsciiCatFlavor::Kenwood, Mode::Cw) => "3",
        };
        self.command("MD", Some(value), false)?;
        Ok(())
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        match self.flavor {
            AsciiCatFlavor::Yaesu => {
                self.command("TX", Some(if enabled { "1" } else { "0" }), false)?;
            }
            AsciiCatFlavor::Kenwood => {
                self.command(if enabled { "TX" } else { "RX" }, None, false)?;
            }
        }
        Ok(())
    }

    fn capabilities(&self) -> RadioCapabilities {
        core_capabilities()
    }
}

fn decode_ascii_mode(flavor: AsciiCatFlavor, raw: &str) -> Result<Mode> {
    let code = raw
        .trim_start_matches('0')
        .chars()
        .next()
        .unwrap_or('0')
        .to_ascii_uppercase();
    let mode = match flavor {
        AsciiCatFlavor::Yaesu => match code {
            '1' => Mode::Lsb,
            '2' => Mode::Usb,
            '3' | '7' => Mode::Cw,
            '6' | '8' | '9' | 'A' | 'C' | 'E' | 'F' => Mode::Data,
            _ => bail!("unsupported Yaesu CAT mode: {raw}"),
        },
        AsciiCatFlavor::Kenwood => match code {
            '1' => Mode::Lsb,
            '2' => Mode::Usb,
            '3' | '7' => Mode::Cw,
            '6' | '9' => Mode::Data,
            _ => bail!("unsupported Kenwood CAT mode: {raw}"),
        },
    };
    Ok(mode)
}

#[derive(Debug, Clone)]
pub struct LegacyYaesuRadio {
    port: String,
    baud_rate: u32,
}

impl LegacyYaesuRadio {
    pub fn new(port: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port: port.into(),
            baud_rate,
        }
    }
    fn transact(&self, frame: [u8; 5], response_len: usize) -> Result<Vec<u8>> {
        if self.port.trim().is_empty() {
            bail!("a serial port is required for legacy Yaesu CAT");
        }
        let mut port = serialport::new(&self.port, self.baud_rate)
            .timeout(Duration::from_millis(750))
            .open()
            .with_context(|| format!("failed to open legacy CAT serial port {}", self.port))?;
        port.write_all(&frame)
            .context("failed to write legacy CAT command")?;
        port.flush().context("failed to flush legacy CAT command")?;
        let mut response = vec![0_u8; response_len];
        if response_len > 0 {
            port.read_exact(&mut response)
                .context("failed to read legacy CAT response")?;
        }
        Ok(response)
    }
}

#[async_trait]
impl RadioHal for LegacyYaesuRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        Ok(yaesu_legacy_cat::decode_frequency_and_mode(
            &self.transact(yaesu_legacy_cat::read_frequency_and_mode(), 5)?,
        )?
        .frequency_hz)
    }
    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        self.transact(yaesu_legacy_cat::set_frequency(hz)?, 0)?;
        Ok(())
    }
    async fn get_mode(&self) -> Result<Mode> {
        let mode = yaesu_legacy_cat::decode_frequency_and_mode(
            &self.transact(yaesu_legacy_cat::read_frequency_and_mode(), 5)?,
        )?
        .mode;
        Ok(match mode {
            yaesu_legacy_cat::LegacyMode::Lsb => Mode::Lsb,
            yaesu_legacy_cat::LegacyMode::Usb => Mode::Usb,
            yaesu_legacy_cat::LegacyMode::Cw | yaesu_legacy_cat::LegacyMode::CwReverse => Mode::Cw,
            _ => Mode::Data,
        })
    }
    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let mode = match mode {
            Mode::Lsb => yaesu_legacy_cat::LegacyMode::Lsb,
            Mode::Usb => yaesu_legacy_cat::LegacyMode::Usb,
            Mode::Cw => yaesu_legacy_cat::LegacyMode::Cw,
            Mode::Data => yaesu_legacy_cat::LegacyMode::Digital,
        };
        self.transact(yaesu_legacy_cat::set_mode(mode), 0)?;
        Ok(())
    }
    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.transact(yaesu_legacy_cat::set_ptt(enabled), 0)?;
        Ok(())
    }
    fn capabilities(&self) -> RadioCapabilities {
        core_capabilities()
    }
}

fn core_capabilities() -> RadioCapabilities {
    RadioCapabilities {
        can_get_frequency: true,
        can_set_frequency: true,
        can_get_mode: true,
        can_set_mode: true,
        can_get_ptt: false,
        can_set_ptt: true,
        can_raw_protocol: false,
    }
}

#[derive(Clone)]
pub enum ConfiguredRadio {
    Icom(crate::IcomCiVRadio),
    Ascii(AsciiCatRadio),
    LegacyYaesu(LegacyYaesuRadio),
}

impl ConfiguredRadio {
    pub fn as_icom(&self) -> Option<&crate::IcomCiVRadio> {
        match self {
            Self::Icom(radio) => Some(radio),
            _ => None,
        }
    }
}

#[async_trait]
impl RadioHal for ConfiguredRadio {
    async fn get_frequency_hz(&self) -> Result<u64> {
        match self {
            Self::Icom(r) => r.get_frequency_hz().await,
            Self::Ascii(r) => r.get_frequency_hz().await,
            Self::LegacyYaesu(r) => r.get_frequency_hz().await,
        }
    }
    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_frequency_hz(hz).await,
            Self::Ascii(r) => r.set_frequency_hz(hz).await,
            Self::LegacyYaesu(r) => r.set_frequency_hz(hz).await,
        }
    }
    async fn get_mode(&self) -> Result<Mode> {
        match self {
            Self::Icom(r) => RadioHal::get_mode(r).await,
            Self::Ascii(r) => r.get_mode().await,
            Self::LegacyYaesu(r) => r.get_mode().await,
        }
    }
    async fn set_mode(&self, mode: Mode) -> Result<()> {
        match self {
            Self::Icom(r) => RadioHal::set_mode(r, mode).await,
            Self::Ascii(r) => r.set_mode(mode).await,
            Self::LegacyYaesu(r) => r.set_mode(mode).await,
        }
    }
    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_ptt(enabled).await,
            Self::Ascii(r) => r.set_ptt(enabled).await,
            Self::LegacyYaesu(r) => r.set_ptt(enabled).await,
        }
    }
    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Icom(r) => r.protocol_write_read(request).await,
            _ => bail!("raw protocol access is not available for this driver"),
        }
    }
    async fn get_control(&self, id: crate::ControlId) -> Result<Option<crate::ControlValue>> {
        match self {
            Self::Icom(r) => r.get_control(id).await,
            _ => Ok(None),
        }
    }
    async fn set_control(&self, id: crate::ControlId, value: crate::ControlValue) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_control(id, value).await,
            _ => bail!("control {id:?} is not available for this driver"),
        }
    }
    fn capabilities(&self) -> RadioCapabilities {
        match self {
            Self::Icom(r) => r.capabilities(),
            Self::Ascii(r) => r.capabilities(),
            Self::LegacyYaesu(r) => r.capabilities(),
        }
    }
}

pub fn open_model(
    model: &str,
    port: impl Into<String>,
    baud_rate: u32,
    controller_address: u8,
) -> Result<ConfiguredRadio> {
    open_model_with_radio_address(model, port, baud_rate, controller_address, None)
}

pub fn open_model_with_radio_address(
    model: &str,
    port: impl Into<String>,
    baud_rate: u32,
    controller_address: u8,
    radio_address: Option<u8>,
) -> Result<ConfiguredRadio> {
    let profile = find_model(model).with_context(|| format!("unknown radio model: {model}"))?;
    let port = port.into();
    Ok(match profile.protocol {
        Protocol::IcomCiV { default_address } => ConfiguredRadio::Icom(
            crate::IcomCiVRadio::new(port, baud_rate, controller_address)
                .with_radio_address(radio_address.unwrap_or(default_address)),
        ),
        Protocol::YaesuCat => {
            ConfiguredRadio::Ascii(AsciiCatRadio::new(port, baud_rate, AsciiCatFlavor::Yaesu))
        }
        Protocol::KenwoodCat => {
            ConfiguredRadio::Ascii(AsciiCatRadio::new(port, baud_rate, AsciiCatFlavor::Kenwood))
        }
        Protocol::YaesuLegacyCat => {
            ConfiguredRadio::LegacyYaesu(LegacyYaesuRadio::new(port, baud_rate))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factory_selects_protocol_family() {
        assert!(matches!(
            open_model("FT-857D", "/dev/null", 9_600, 0xE0).unwrap(),
            ConfiguredRadio::LegacyYaesu(_)
        ));
        assert!(matches!(
            open_model("FTDX101D", "/dev/null", 38_400, 0xE0).unwrap(),
            ConfiguredRadio::Ascii(_)
        ));
        assert!(matches!(
            open_model("TS-590SG", "/dev/null", 115_200, 0xE0).unwrap(),
            ConfiguredRadio::Ascii(_)
        ));
    }
    #[test]
    fn factory_accepts_an_icom_address_override() {
        let radio =
            open_model_with_radio_address("IC-7300", "/dev/null", 115_200, 0xE0, Some(0x95))
                .unwrap();
        assert!(radio.as_icom().is_some());
    }
    #[test]
    fn decodes_common_ascii_modes() {
        assert_eq!(
            decode_ascii_mode(AsciiCatFlavor::Yaesu, "02").unwrap(),
            Mode::Usb
        );
        assert_eq!(
            decode_ascii_mode(AsciiCatFlavor::Yaesu, "0C").unwrap(),
            Mode::Data
        );
        assert_eq!(
            decode_ascii_mode(AsciiCatFlavor::Kenwood, "1").unwrap(),
            Mode::Lsb
        );
    }
}
