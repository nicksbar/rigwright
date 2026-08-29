//! Model-aware serial drivers and factory.

use std::{
    io::{Read, Write},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use crate::hal::{
    DtmfSequence, MemoryChannel, Mode, NullRadio, Radio, RadioCapabilities, RepeaterSettings,
    TunerStatus,
};
use crate::{
    dxlab::DxLabCommanderRadio,
    icom::civ_radio::IcomCiVRadio,
    kenwood::KenwoodCatRadio,
    models::{
        find_model, IcomCivModel, KenwoodCatModel, Protocol, YaesuCatModel, YaesuLegacyModel,
        GENERIC_ICOM_MODEL, GENERIC_KENWOOD_MODEL, GENERIC_YAESU_CLASSIC_MODEL,
        GENERIC_YAESU_MODEL,
    },
    protocol::ascii_cat,
    rigctld::RigctldRadio,
    yaesu::YaesuCatRadio,
};

pub use crate::yaesu::LegacyYaesuRadio;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compatibility selector for the original shared ASCII driver.
///
/// New Yaesu integrations should use `YaesuCatRadio`; the model factory does
/// so automatically. This type remains because existing Kenwood and external
/// callers still use the original API.
pub enum AsciiCatFlavor {
    Yaesu,
    Kenwood,
}

#[derive(Debug, Clone)]
/// Original minimal ASCII CAT driver retained for source compatibility.
///
/// Model-factory selection uses profile-backed vendor drivers instead.
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
}

#[async_trait]
impl Radio for AsciiCatRadio {
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
        let response = self.command(
            "MD",
            (self.flavor == AsciiCatFlavor::Yaesu).then_some("0"),
            true,
        )?;
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
            (AsciiCatFlavor::Yaesu, Mode::Am) => "05",
            (AsciiCatFlavor::Yaesu, Mode::Fm) => "04",
            (AsciiCatFlavor::Yaesu, Mode::Wfm) => {
                bail!("WFM has no common Yaesu CAT mode mapping")
            }
            (AsciiCatFlavor::Yaesu, Mode::Rtty) => "06",
            (AsciiCatFlavor::Yaesu, Mode::CwReverse) => "07",
            (AsciiCatFlavor::Yaesu, Mode::RttyReverse) => "09",
            (AsciiCatFlavor::Kenwood, Mode::Am) => "5",
            (AsciiCatFlavor::Kenwood, Mode::Fm) => "4",
            (AsciiCatFlavor::Kenwood, Mode::Wfm) => {
                bail!("WFM has no common Kenwood CAT mode mapping")
            }
            (AsciiCatFlavor::Kenwood, Mode::Rtty) => "6",
            (AsciiCatFlavor::Kenwood, Mode::CwReverse) => "7",
            (AsciiCatFlavor::Kenwood, Mode::RttyReverse) => "9",
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
    async fn get_ptt(&self) -> Result<bool> {
        match self.flavor {
            AsciiCatFlavor::Yaesu => {
                let response = self.command("TX", None, true)?;
                match std::str::from_utf8(&response)
                    .context("CAT PTT response is not ASCII")?
                    .strip_prefix("TX")
                    .and_then(|value| value.strip_suffix(';'))
                {
                    Some("0") => Ok(false),
                    Some("1" | "2") => Ok(true),
                    _ => bail!("unexpected Yaesu CAT PTT response"),
                }
            }
            AsciiCatFlavor::Kenwood => {
                bail!("reading PTT is not implemented for generic Kenwood CAT")
            }
        }
    }
    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_ptt: self.flavor == AsciiCatFlavor::Yaesu,
            ..core_capabilities()
        }
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
            '3' => Mode::Cw,
            '4' => Mode::Fm,
            '5' => Mode::Am,
            '6' => Mode::Rtty,
            '7' => Mode::CwReverse,
            '9' => Mode::RttyReverse,
            '8' | 'A' | 'C' | 'E' | 'F' => Mode::Data,
            _ => bail!("unsupported Yaesu CAT mode: {raw}"),
        },
        AsciiCatFlavor::Kenwood => match code {
            '1' => Mode::Lsb,
            '2' => Mode::Usb,
            '3' => Mode::Cw,
            '4' => Mode::Fm,
            '5' => Mode::Am,
            '6' => Mode::Rtty,
            '7' => Mode::CwReverse,
            '9' => Mode::RttyReverse,
            _ => bail!("unsupported Kenwood CAT mode: {raw}"),
        },
    };
    Ok(mode)
}

fn core_capabilities() -> RadioCapabilities {
    RadioCapabilities {
        can_get_frequency: true,
        can_set_frequency: true,
        can_get_mode: true,
        can_set_mode: true,
        can_get_ptt: false,
        can_set_ptt: true,
        can_get_power: false,
        can_set_power: false,
        can_raw_protocol: false,
    }
}

#[derive(Clone)]
pub enum ConfiguredRadio {
    Icom(IcomCiVRadio),
    Yaesu(YaesuCatRadio),
    Kenwood(KenwoodCatRadio),
    Ascii(AsciiCatRadio),
    DxLab(DxLabCommanderRadio),
    LegacyYaesu(LegacyYaesuRadio),
    Rigctld(RigctldRadio),
    Null(NullRadio),
}

impl ConfiguredRadio {
    pub fn as_icom(&self) -> Option<&IcomCiVRadio> {
        match self {
            Self::Icom(radio) => Some(radio),
            _ => None,
        }
    }
    pub fn as_yaesu(&self) -> Option<&YaesuCatRadio> {
        match self {
            Self::Yaesu(radio) => Some(radio),
            _ => None,
        }
    }
    pub fn as_legacy_yaesu(&self) -> Option<&LegacyYaesuRadio> {
        match self {
            Self::LegacyYaesu(radio) => Some(radio),
            _ => None,
        }
    }
    pub fn as_kenwood(&self) -> Option<&KenwoodCatRadio> {
        match self {
            Self::Kenwood(radio) => Some(radio),
            _ => None,
        }
    }
    pub fn as_rigctld(&self) -> Option<&RigctldRadio> {
        match self {
            Self::Rigctld(radio) => Some(radio),
            _ => None,
        }
    }

    pub fn as_dxlab(&self) -> Option<&DxLabCommanderRadio> {
        match self {
            Self::DxLab(radio) => Some(radio),
            _ => None,
        }
    }
}

#[async_trait]
impl Radio for ConfiguredRadio {
    fn event_router(&self) -> Option<crate::RadioEventRouter> {
        match self {
            Self::Icom(r) => r.event_router(),
            _ => None,
        }
    }

    async fn get_frequency_hz(&self) -> Result<u64> {
        match self {
            Self::Icom(r) => r.get_frequency_hz().await,
            Self::Yaesu(r) => r.get_frequency_hz().await,
            Self::Kenwood(r) => r.get_frequency_hz().await,
            Self::Ascii(r) => r.get_frequency_hz().await,
            Self::DxLab(r) => r.get_frequency_hz().await,
            Self::LegacyYaesu(r) => r.get_frequency_hz().await,
            Self::Rigctld(r) => r.get_frequency_hz().await,
            Self::Null(r) => r.get_frequency_hz().await,
        }
    }
    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_frequency_hz(hz).await,
            Self::Yaesu(r) => r.set_frequency_hz(hz).await,
            Self::Kenwood(r) => r.set_frequency_hz(hz).await,
            Self::Ascii(r) => r.set_frequency_hz(hz).await,
            Self::DxLab(r) => r.set_frequency_hz(hz).await,
            Self::LegacyYaesu(r) => r.set_frequency_hz(hz).await,
            Self::Rigctld(r) => r.set_frequency_hz(hz).await,
            Self::Null(r) => r.set_frequency_hz(hz).await,
        }
    }
    async fn get_mode(&self) -> Result<Mode> {
        match self {
            Self::Icom(r) => Radio::get_mode(r).await,
            Self::Yaesu(r) => r.get_mode().await,
            Self::Kenwood(r) => r.get_mode().await,
            Self::Ascii(r) => r.get_mode().await,
            Self::DxLab(r) => r.get_mode().await,
            Self::LegacyYaesu(r) => r.get_mode().await,
            Self::Rigctld(r) => r.get_mode().await,
            Self::Null(r) => r.get_mode().await,
        }
    }
    async fn set_mode(&self, mode: Mode) -> Result<()> {
        match self {
            Self::Icom(r) => Radio::set_mode(r, mode).await,
            Self::Yaesu(r) => r.set_mode(mode).await,
            Self::Kenwood(r) => r.set_mode(mode).await,
            Self::Ascii(r) => r.set_mode(mode).await,
            Self::DxLab(r) => r.set_mode(mode).await,
            Self::LegacyYaesu(r) => r.set_mode(mode).await,
            Self::Rigctld(r) => r.set_mode(mode).await,
            Self::Null(r) => r.set_mode(mode).await,
        }
    }
    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_ptt(enabled).await,
            Self::Yaesu(r) => r.set_ptt(enabled).await,
            Self::Kenwood(r) => r.set_ptt(enabled).await,
            Self::Ascii(r) => r.set_ptt(enabled).await,
            Self::DxLab(r) => r.set_ptt(enabled).await,
            Self::LegacyYaesu(r) => r.set_ptt(enabled).await,
            Self::Rigctld(r) => r.set_ptt(enabled).await,
            Self::Null(r) => r.set_ptt(enabled).await,
        }
    }
    async fn get_ptt(&self) -> Result<bool> {
        match self {
            Self::Icom(r) => r.get_ptt().await,
            Self::Yaesu(r) => r.get_ptt().await,
            Self::Kenwood(r) => r.get_ptt().await,
            Self::Ascii(r) => r.get_ptt().await,
            Self::DxLab(r) => r.get_ptt().await,
            Self::LegacyYaesu(r) => r.get_ptt().await,
            Self::Rigctld(r) => r.get_ptt().await,
            Self::Null(r) => r.get_ptt().await,
        }
    }
    async fn get_power(&self) -> Result<bool> {
        match self {
            Self::Icom(r) => r.get_power().await,
            Self::Yaesu(r) => r.get_power().await,
            Self::Kenwood(r) => r.get_power().await,
            Self::Ascii(r) => r.get_power().await,
            Self::DxLab(r) => r.get_power().await,
            Self::LegacyYaesu(r) => r.get_power().await,
            Self::Rigctld(r) => r.get_power().await,
            Self::Null(r) => r.get_power().await,
        }
    }
    async fn set_power(&self, enabled: bool) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_power(enabled).await,
            Self::Yaesu(r) => r.set_power(enabled).await,
            Self::Kenwood(r) => r.set_power(enabled).await,
            Self::Ascii(r) => r.set_power(enabled).await,
            Self::DxLab(r) => r.set_power(enabled).await,
            Self::LegacyYaesu(r) => r.set_power(enabled).await,
            Self::Rigctld(r) => r.set_power(enabled).await,
            Self::Null(r) => r.set_power(enabled).await,
        }
    }
    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Icom(r) => r.protocol_write_read(request).await,
            Self::Yaesu(r) => r.protocol_write_read(request).await,
            Self::Kenwood(r) => r.protocol_write_read(request).await,
            Self::LegacyYaesu(r) => r.protocol_write_read(request).await,
            _ => bail!("raw protocol access is not available for this driver"),
        }
    }
    async fn get_control(&self, id: crate::ControlId) -> Result<Option<crate::ControlValue>> {
        match self {
            Self::Icom(r) => r.get_control(id).await,
            Self::Yaesu(r) => r.get_control(id).await,
            Self::Kenwood(r) => r.get_control(id).await,
            Self::LegacyYaesu(r) => r.get_control(id).await,
            _ => Ok(None),
        }
    }
    async fn set_control(&self, id: crate::ControlId, value: crate::ControlValue) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_control(id, value).await,
            Self::Yaesu(r) => r.set_control(id, value).await,
            Self::Kenwood(r) => r.set_control(id, value).await,
            Self::LegacyYaesu(r) => r.set_control(id, value).await,
            _ => bail!("control {id:?} is not available for this driver"),
        }
    }
    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        match self {
            Self::Icom(r) => r.get_repeater_settings(),
            Self::Yaesu(r) => r.get_repeater_settings(),
            Self::Kenwood(r) => r.get_repeater_settings(),
            Self::LegacyYaesu(r) => r.get_repeater_settings().await,
            _ => bail!("repeater settings are not available for this driver"),
        }
    }
    async fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_repeater_settings(settings),
            Self::Yaesu(r) => r.set_repeater_settings(settings),
            Self::Kenwood(r) => r.set_repeater_settings(settings),
            Self::LegacyYaesu(r) => r.set_repeater_settings(settings).await,
            _ => bail!("repeater settings are not available for this driver"),
        }
    }
    async fn get_rit_offset_hz(&self) -> Result<i32> {
        match self {
            Self::Icom(r) => r.get_rit_offset_hz(),
            _ => bail!("RIT offset control is not available for this driver"),
        }
    }
    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        match self {
            Self::Icom(r) => r.set_rit_offset_hz(offset_hz),
            _ => bail!("RIT offset control is not available for this driver"),
        }
    }
    async fn select_memory_channel(&self, channel: u16) -> Result<()> {
        match self {
            Self::Icom(r) => r.select_memory_channel(channel),
            Self::Yaesu(r) => r.select_memory_channel(channel),
            Self::Kenwood(r) => r.select_memory_channel(channel),
            Self::LegacyYaesu(r) => r.select_memory_channel(channel).await,
            _ => bail!("memory channels are not available for this driver"),
        }
    }
    async fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        match self {
            Self::Icom(r) => r.read_memory_channel(channel),
            Self::Yaesu(r) => r.read_memory_channel(channel),
            Self::Kenwood(r) => r.read_memory_channel(channel),
            Self::LegacyYaesu(r) => r.read_memory_channel(channel).await,
            _ => bail!("memory channels are not available for this driver"),
        }
    }
    async fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        match self {
            Self::Icom(r) => r.write_memory_channel(channel),
            Self::Yaesu(r) => r.write_memory_channel(channel),
            Self::Kenwood(r) => r.write_memory_channel(channel),
            Self::LegacyYaesu(r) => r.write_memory_channel(channel).await,
            _ => bail!("memory channels are not available for this driver"),
        }
    }
    async fn send_dtmf(&self, sequence: DtmfSequence) -> Result<()> {
        match self {
            Self::Icom(r) => r.send_dtmf(sequence).await,
            Self::Yaesu(r) => r.send_dtmf(sequence).await,
            Self::Kenwood(r) => r.send_dtmf(sequence).await,
            Self::LegacyYaesu(r) => r.send_dtmf(sequence).await,
            _ => bail!("DTMF is not available for this driver"),
        }
    }
    async fn get_meter(&self, id: crate::MeterId) -> Result<Option<u8>> {
        match self {
            Self::Icom(r) => r.get_meter(id).await,
            Self::Yaesu(r) => r.get_meter(id).await,
            Self::Kenwood(r) => match id {
                crate::MeterId::Signal => Ok(Some(r.get_signal_meter()?)),
                crate::MeterId::Swr => Ok(Some(r.get_swr_meter()?)),
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }
    fn supports_meter(&self, id: crate::MeterId) -> bool {
        match self {
            Self::Icom(r) => r.supports_meter(id),
            Self::Yaesu(r) => r.model().is_some() && !matches!(id, crate::MeterId::Temperature),
            Self::Kenwood(r) => {
                r.model().is_some() && matches!(id, crate::MeterId::Signal | crate::MeterId::Swr)
            }
            _ => false,
        }
    }
    fn supports_control(&self, id: crate::ControlId) -> bool {
        match self {
            Self::Icom(r) => r.supports_control(id),
            Self::Yaesu(r) => r.supports_control(id),
            Self::Kenwood(r) => r.supports_control(id),
            Self::LegacyYaesu(r) => r.supports_control(id),
            _ => false,
        }
    }
    fn supports_control_read(&self, id: crate::ControlId) -> bool {
        match self {
            Self::Icom(r) => r.supports_control_read(id),
            Self::Yaesu(r) => r.supports_control_read(id),
            Self::Kenwood(r) => r.supports_control_read(id),
            Self::LegacyYaesu(r) => r.supports_control_read(id),
            _ => false,
        }
    }
    fn supports_control_write(&self, id: crate::ControlId) -> bool {
        match self {
            Self::Icom(r) => r.supports_control_write(id),
            Self::Yaesu(r) => r.supports_control_write(id),
            Self::Kenwood(r) => r.supports_control_write(id),
            Self::LegacyYaesu(r) => r.supports_control_write(id),
            _ => false,
        }
    }
    async fn start_tuner(&self) -> Result<()> {
        match self {
            Self::Icom(r) => r.start_tuner().await,
            _ => bail!("antenna tuner control is not available for this driver"),
        }
    }
    async fn get_tuner_status(&self) -> Result<Option<TunerStatus>> {
        match self {
            Self::Icom(r) => r.get_tuner_status().await,
            _ => Ok(None),
        }
    }
    fn capabilities(&self) -> RadioCapabilities {
        match self {
            Self::Icom(r) => r.capabilities(),
            Self::Yaesu(r) => r.capabilities(),
            Self::Kenwood(r) => r.capabilities(),
            Self::Ascii(r) => r.capabilities(),
            Self::DxLab(r) => r.capabilities(),
            Self::LegacyYaesu(r) => r.capabilities(),
            Self::Rigctld(r) => r.capabilities(),
            Self::Null(r) => r.capabilities(),
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

pub fn open_rigctld(address: impl Into<String>) -> ConfiguredRadio {
    ConfiguredRadio::Rigctld(RigctldRadio::new(address))
}

pub fn open_dxlab(address: impl Into<String>) -> ConfiguredRadio {
    ConfiguredRadio::DxLab(DxLabCommanderRadio::new(address))
}

pub fn open_dxlab_localhost() -> ConfiguredRadio {
    ConfiguredRadio::DxLab(DxLabCommanderRadio::localhost())
}

pub fn open_null() -> ConfiguredRadio {
    ConfiguredRadio::Null(NullRadio::new())
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
        Protocol::IcomCiV { default_address } if profile.model == GENERIC_ICOM_MODEL => {
            ConfiguredRadio::Icom(IcomCiVRadio::new_generic(
                port,
                baud_rate,
                controller_address,
                radio_address.unwrap_or(default_address),
            ))
        }
        Protocol::IcomCiV { default_address } => {
            let model = IcomCivModel::from_model_name(profile.model)
                .with_context(|| format!("unsupported Icom CI-V model: {}", profile.model))?;
            ConfiguredRadio::Icom(IcomCiVRadio::new_for_model(
                model,
                port,
                baud_rate,
                controller_address,
                radio_address.unwrap_or(default_address),
            ))
        }
        Protocol::YaesuCat if profile.model == GENERIC_YAESU_MODEL => {
            ConfiguredRadio::Yaesu(YaesuCatRadio::new_generic(port, baud_rate))
        }
        Protocol::YaesuCat => {
            let model = YaesuCatModel::from_model_name(profile.model).with_context(|| {
                format!("unsupported modern Yaesu CAT model: {}", profile.model)
            })?;
            ConfiguredRadio::Yaesu(YaesuCatRadio::new_for_model(model, port, baud_rate)?)
        }
        Protocol::KenwoodCat if profile.model == GENERIC_KENWOOD_MODEL => {
            ConfiguredRadio::Kenwood(KenwoodCatRadio::new_generic(port, baud_rate))
        }
        Protocol::KenwoodCat => {
            let model = KenwoodCatModel::from_model_name(profile.model)
                .with_context(|| format!("unsupported Kenwood CAT model: {}", profile.model))?;
            ConfiguredRadio::Kenwood(KenwoodCatRadio::new_for_model(model, port, baud_rate)?)
        }
        Protocol::YaesuLegacyCat if profile.model == GENERIC_YAESU_CLASSIC_MODEL => {
            ConfiguredRadio::LegacyYaesu(LegacyYaesuRadio::new_generic(port, baud_rate))
        }
        Protocol::YaesuLegacyCat => {
            let model = YaesuLegacyModel::from_model_name(profile.model).with_context(|| {
                format!("unsupported classic Yaesu CAT model: {}", profile.model)
            })?;
            ConfiguredRadio::LegacyYaesu(LegacyYaesuRadio::new_for_model(model, port, baud_rate)?)
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
            ConfiguredRadio::Yaesu(_)
        ));
        assert!(matches!(
            open_model("TS-590SG", "/dev/null", 115_200, 0xE0).unwrap(),
            ConfiguredRadio::Kenwood(_)
        ));
    }
    #[test]
    fn factory_accepts_an_icom_address_override() {
        let radio =
            open_model_with_radio_address("IC-7300", "/dev/null", 115_200, 0xE0, Some(0x95))
                .unwrap();
        let icom = radio.as_icom().expect("Icom driver");
        assert_eq!(icom.controller_address(), 0xE0);
        assert_eq!(icom.radio_address(), 0x95);
    }
    #[test]
    fn factory_selects_protocol_only_generic_profiles() {
        let icom = open_model(GENERIC_ICOM_MODEL, "/dev/null", 115_200, 0xE0).unwrap();
        assert!(icom.as_icom().is_some());
        assert!(icom.as_icom().unwrap().model().is_none());

        let yaesu = open_model(GENERIC_YAESU_MODEL, "/dev/null", 38_400, 0xE0).unwrap();
        assert!(yaesu.as_yaesu().is_some());
        assert!(yaesu.as_yaesu().unwrap().model().is_none());

        let classic = open_model(GENERIC_YAESU_CLASSIC_MODEL, "/dev/null", 4_800, 0xE0).unwrap();
        assert!(classic.as_legacy_yaesu().is_some());
        assert!(classic.as_legacy_yaesu().unwrap().model().is_none());

        let kenwood = open_model(GENERIC_KENWOOD_MODEL, "/dev/null", 9_600, 0xE0).unwrap();
        assert!(kenwood.as_kenwood().is_some());
        assert!(kenwood.as_kenwood().unwrap().model().is_none());
    }
    #[test]
    fn factory_selects_the_exact_yaesu_profile_and_validates_baud() {
        let radio = open_model("FTDX10", "/dev/null", 38_400, 0xE0).unwrap();
        assert_eq!(
            radio.as_yaesu().and_then(YaesuCatRadio::model),
            Some(YaesuCatModel::Ftdx10)
        );
        assert!(open_model("FTDX10", "/dev/null", 115_200, 0xE0).is_err());
        assert!(open_model("FT-710", "/dev/null", 115_200, 0xE0).is_ok());
    }
    #[test]
    fn factory_selects_the_exact_classic_yaesu_profile() {
        let radio = open_model("FT-857D", "/dev/null", 9_600, 0xE0).unwrap();
        assert_eq!(
            radio.as_legacy_yaesu().and_then(LegacyYaesuRadio::model),
            Some(YaesuLegacyModel::Ft857D)
        );
        assert!(open_model("FT-857D", "/dev/null", 19_200, 0xE0).is_err());
    }
    #[test]
    fn factory_selects_the_exact_kenwood_profile_and_validates_baud() {
        let radio = open_model("TS-890S", "/dev/null", 115_200, 0xE0).unwrap();
        assert_eq!(
            radio.as_kenwood().and_then(KenwoodCatRadio::model),
            Some(KenwoodCatModel::Ts890S)
        );
        assert!(open_model("TS-2000", "/dev/null", 115_200, 0xE0).is_err());
        assert!(open_model("TS-2000X", "/dev/null", 9_600, 0xE0).is_err());
    }
    #[test]
    fn configured_radio_reports_profiled_controls_and_meters() {
        let icom = open_model("IC-7300", "/dev/null", 115_200, 0xE0).unwrap();
        assert!(icom.supports_control(crate::ControlId::IpPlus));
        assert!(icom.supports_control(crate::ControlId::NoiseReduction));
        assert!(!icom.supports_control_read(crate::ControlId::Vfo));
        assert!(icom.supports_control_write(crate::ControlId::Vfo));
        assert!(!icom.supports_control_read(crate::ControlId::RawCiV));
        assert!(!icom.supports_control_write(crate::ControlId::RawCiV));
        assert!(icom.supports_meter(crate::MeterId::Swr));
        assert!(icom.supports_meter(crate::MeterId::Signal));
        assert!(icom.supports_meter(crate::MeterId::Power));
        assert!(icom.supports_meter(crate::MeterId::Alc));
        assert!(icom.supports_meter(crate::MeterId::Compression));
        assert!(icom.supports_meter(crate::MeterId::Voltage));
        assert!(icom.supports_meter(crate::MeterId::Current));
        assert!(icom.supports_meter(crate::MeterId::Temperature));

        let ic9700 = open_model("IC-9700", "/dev/null", 115_200, 0xE0).unwrap();
        assert!(ic9700.supports_control(crate::ControlId::MainSub));
        assert!(ic9700.supports_control_read(crate::ControlId::MainSub));
        assert!(ic9700.supports_control_write(crate::ControlId::MainSub));
        assert!(ic9700.supports_control(crate::ControlId::ExternalPreamp));

        let yaesu = open_model("FTDX10", "/dev/null", 38_400, 0xE0).unwrap();
        assert!(yaesu.supports_control(crate::ControlId::Agc));
        assert!(yaesu.supports_control_read(crate::ControlId::Agc));
        assert!(yaesu.supports_control_write(crate::ControlId::Agc));
        assert!(yaesu.supports_control(crate::ControlId::NoiseReductionLevel));
        assert!(yaesu.supports_meter(crate::MeterId::Voltage));
        assert!(!yaesu.supports_meter(crate::MeterId::Temperature));

        let kenwood = open_model("TS-890S", "/dev/null", 115_200, 0xE0).unwrap();
        assert!(kenwood.supports_control(crate::ControlId::RfPower));
        assert!(kenwood.supports_control_read(crate::ControlId::RfPower));
        assert!(kenwood.supports_control_write(crate::ControlId::RfPower));
        assert!(kenwood.supports_control_read(crate::ControlId::Split));
        assert!(kenwood.supports_control_write(crate::ControlId::Split));
        assert!(kenwood.supports_meter(crate::MeterId::Signal));
        assert!(kenwood.supports_meter(crate::MeterId::Swr));
        assert!(!kenwood.supports_meter(crate::MeterId::Alc));

        let generic = open_model(GENERIC_KENWOOD_MODEL, "/dev/null", 9_600, 0xE0).unwrap();
        assert!(!generic.supports_control(crate::ControlId::RfPower));
        assert!(!generic.supports_meter(crate::MeterId::Signal));

        let generic_icom = open_model(GENERIC_ICOM_MODEL, "/dev/null", 9_600, 0xE0).unwrap();
        assert!(!generic_icom.supports_control(crate::ControlId::IpPlus));
        assert!(!generic_icom.supports_meter(crate::MeterId::Swr));

        let generic_yaesu = open_model(GENERIC_YAESU_MODEL, "/dev/null", 9_600, 0xE0).unwrap();
        assert!(!generic_yaesu.supports_control(crate::ControlId::Agc));
        assert!(!generic_yaesu.supports_meter(crate::MeterId::Swr));

        let legacy = open_model("FT-857D", "/dev/null", 9_600, 0xE0).unwrap();
        assert!(legacy.supports_control(crate::ControlId::Split));
        assert!(!legacy.supports_control(crate::ControlId::RfPower));

        let generic_legacy =
            open_model(GENERIC_YAESU_CLASSIC_MODEL, "/dev/null", 4_800, 0xE0).unwrap();
        assert!(generic_legacy.supports_control(crate::ControlId::Split));
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
            decode_ascii_mode(AsciiCatFlavor::Yaesu, "06").unwrap(),
            Mode::Rtty
        );
        assert_eq!(
            decode_ascii_mode(AsciiCatFlavor::Kenwood, "1").unwrap(),
            Mode::Lsb
        );
    }
    #[test]
    fn rigctld_factory_wraps_tcp_driver() {
        let radio = open_rigctld("127.0.0.1:4532");
        assert!(radio.as_rigctld().is_some());
    }
    #[test]
    fn null_factory_wraps_in_memory_driver() {
        let radio = open_null();
        assert!(matches!(radio, ConfiguredRadio::Null(_)));
    }
    #[test]
    fn dxlab_factory_wraps_commander_driver() {
        let radio = open_dxlab_localhost();
        assert!(radio.as_dxlab().is_some());
    }
}
