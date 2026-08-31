//! Shared semicolon-framed Elecraft transceiver driver.

use crate::{
    events::{RadioEvent, RadioEventRouter},
    hal::{Mode, Radio, RadioCapabilities},
    hal_types::{ControlId, ControlValue, MeterId},
    transport::RadioTransport,
};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use super::{
    profile::{profile_for_model, ElecraftModel, ElecraftProfile},
    transport::ElecraftTransport,
};

pub struct ElecraftRadio {
    model: Option<ElecraftModel>,
    port: String,
    baud_rate: u32,
    transport: ElecraftTransport,
    event_router: RadioEventRouter,
}

impl Clone for ElecraftRadio {
    fn clone(&self) -> Self {
        Self {
            model: self.model,
            port: self.port.clone(),
            baud_rate: self.baud_rate,
            transport: self.transport.clone(),
            event_router: self.event_router.clone(),
        }
    }
}

impl std::fmt::Debug for ElecraftRadio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElecraftRadio")
            .field("model", &self.model)
            .field("port", &self.port)
            .field("baud_rate", &self.baud_rate)
            .finish_non_exhaustive()
    }
}

impl ElecraftRadio {
    pub fn new_generic(port: impl Into<String>, baud_rate: u32) -> Self {
        Self::new_internal(None, port, baud_rate)
    }

    pub fn new_for_model(
        model: ElecraftModel,
        port: impl Into<String>,
        baud_rate: u32,
    ) -> Result<Self> {
        profile_for_model(model).validate_baud(baud_rate)?;
        Ok(Self::new_internal(Some(model), port, baud_rate))
    }

    pub fn with_external_transport<T>(
        model: Option<ElecraftModel>,
        baud_rate: u32,
        transport: T,
    ) -> Result<Self>
    where
        T: RadioTransport + 'static,
    {
        if let Some(model) = model {
            profile_for_model(model).validate_baud(baud_rate)?;
        }
        Ok(Self {
            model,
            port: String::new(),
            baud_rate,
            transport: ElecraftTransport::external(transport),
            event_router: RadioEventRouter::default(),
        })
    }

    pub fn model(&self) -> Option<ElecraftModel> {
        self.model
    }

    fn new_internal(model: Option<ElecraftModel>, port: impl Into<String>, baud_rate: u32) -> Self {
        let port = port.into();
        Self {
            model,
            port: port.clone(),
            baud_rate,
            transport: ElecraftTransport::serial(port, baud_rate),
            event_router: RadioEventRouter::default(),
        }
    }

    fn profile(&self) -> Option<ElecraftProfile> {
        self.model.map(profile_for_model)
    }

    fn query(&self, command: &str) -> Result<Vec<u8>> {
        let router = self.event_router.clone();
        let model = self.model;
        self.transport.query_with_handler(command, move |frame| {
            publish_event(&router, model, frame);
        })
    }

    fn query_with_response_prefix(&self, command: &str, response_prefix: &str) -> Result<Vec<u8>> {
        self.transport
            .query_with_response_prefix(command, response_prefix)
    }

    fn set(&self, command: &str, parameter: &str) -> Result<()> {
        self.transport.set(command, parameter)
    }

    /// Configure the documented Elecraft Auto-Info mode. Modes 1 and 2 can
    /// produce unsolicited frames while another command is being queried;
    /// those frames are routed through `Radio::event_router()`.
    pub fn set_auto_info(&self, mode: u8) -> Result<()> {
        anyhow::ensure!(mode <= 3, "Elecraft Auto-Info mode must be 0..=3");
        self.set("AI", &mode.to_string())
    }

    pub fn event_router(&self) -> RadioEventRouter {
        self.event_router.clone()
    }

    /// Query the Elecraft compatibility identifier. A model-specific `K` or
    /// `OM` probe should be added by callers when they need option-aware
    /// identification; this method deliberately returns the raw CAT reply.
    pub fn identify(&self) -> Result<Vec<u8>> {
        self.query("ID")
    }

    fn selected_frequency(&self) -> &'static str {
        "FA"
    }

    fn parse_frequency(profile: ElecraftProfile, response: &[u8]) -> Result<u64> {
        let text =
            std::str::from_utf8(response).context("Elecraft frequency response is not ASCII")?;
        let value: u64 = text
            .strip_prefix("FA")
            .and_then(|v| v.strip_suffix(';'))
            .context("unexpected Elecraft frequency response")?
            .parse()
            .context("invalid Elecraft frequency")?;
        Ok(value * profile.frequency_scale_hz)
    }

    fn parse_mode(profile: ElecraftProfile, response: &[u8]) -> Result<Mode> {
        let text = std::str::from_utf8(response).context("Elecraft mode response is not ASCII")?;
        let code = text
            .strip_prefix("MD")
            .and_then(|v| v.strip_suffix(';'))
            .and_then(|v| v.chars().next())
            .context("unexpected Elecraft mode response")?;
        profile.decode_mode(code)
    }

    fn parse_numeric(response: &[u8], prefix: &str) -> Result<u16> {
        let text = std::str::from_utf8(response).context("Elecraft response is not ASCII")?;
        text.strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(';'))
            .context("unexpected Elecraft numeric response")?
            .parse()
            .context("invalid Elecraft numeric response")
    }

    fn parse_if_state(response: &[u8]) -> Result<(i32, bool, bool)> {
        let text = std::str::from_utf8(response).context("Elecraft IF response is not ASCII")?;
        let payload = text
            .strip_prefix("IF")
            .and_then(|value| value.strip_suffix(';'))
            .context("unexpected Elecraft IF response")?;
        let bytes = payload.as_bytes();
        anyhow::ensure!(bytes.len() >= 19, "short Elecraft IF response");
        let sign = match bytes[12] {
            b'+' | b' ' => 1,
            b'-' => -1,
            _ => bail!("invalid Elecraft RIT/XIT sign"),
        };
        let offset = std::str::from_utf8(&bytes[13..17])?.parse::<i32>()? * sign;
        Ok((offset, bytes[17] == b'1', bytes[18] == b'1'))
    }

    fn parse_meter_value(response: &[u8], prefix: &str, maximum: u16) -> Result<u8> {
        let text = std::str::from_utf8(response).context("Elecraft meter response is not ASCII")?;
        let raw = text
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(';'))
            .context("unexpected Elecraft meter response")?;
        let digits = raw
            .char_indices()
            .find(|(_, character)| !character.is_ascii_digit())
            .map_or(raw, |(index, _)| &raw[..index]);
        let value = digits.parse::<u16>()?;
        crate::normalize_meter_level(value, maximum)
            .context("Elecraft meter value is outside its documented range")
    }

    fn decode_control(profile: ElecraftProfile, id: ControlId, response: &[u8]) -> Result<u8> {
        let text =
            std::str::from_utf8(response).context("Elecraft control response is not ASCII")?;
        let raw = match id {
            ControlId::AfGain => text.strip_prefix("AG"),
            ControlId::RfGain => text.strip_prefix("RG"),
            ControlId::Squelch => text.strip_prefix("SQ"),
            ControlId::RfPower => text.strip_prefix("PC"),
            ControlId::Preamp => text.strip_prefix("PA"),
            ControlId::Attenuator => text.strip_prefix("RA"),
            ControlId::Filter => text.strip_prefix("BW").or_else(|| text.strip_prefix("FW")),
            _ => None,
        }
        .context("unexpected Elecraft receiver-control response")?
        .trim_end_matches(';');
        let native = raw.strip_prefix('-').unwrap_or(raw).parse::<u16>()?;
        let maximum = match id {
            ControlId::AfGain => profile.af_gain_max,
            ControlId::RfGain => profile.rf_gain_max,
            ControlId::Squelch => Some(profile.squelch_max),
            ControlId::RfPower => profile.power_max_watts,
            ControlId::Preamp => profile.preamp_max.map(u16::from),
            ControlId::Attenuator => profile.attenuator_max.map(u16::from),
            ControlId::Filter => profile.filter_max_hz,
            _ => None,
        }
        .context("Elecraft control is not profiled")?;
        if native > maximum {
            bail!("Elecraft control value exceeds profile maximum");
        }
        Ok(if id == ControlId::RfPower {
            ((u32::from(native) * 255) / u32::from(maximum)) as u8
        } else if id == ControlId::RfGain && profile.rf_gain_is_attenuation {
            ((u32::from(maximum - native) * 255) / u32::from(maximum)) as u8
        } else {
            ((u32::from(native) * 255) / u32::from(maximum)) as u8
        })
    }

    fn encode_control(profile: ElecraftProfile, id: ControlId, value: u8) -> Result<String> {
        let maximum = match id {
            ControlId::AfGain => profile.af_gain_max,
            ControlId::RfGain => profile.rf_gain_max,
            ControlId::Squelch => Some(profile.squelch_max),
            ControlId::RfPower => profile.power_max_watts,
            ControlId::Preamp => profile.preamp_max.map(u16::from),
            ControlId::Attenuator => profile.attenuator_max.map(u16::from),
            ControlId::Filter => profile.filter_max_hz,
            _ => None,
        }
        .context("Elecraft control is not profiled")?;
        let native = if id == ControlId::RfGain && profile.rf_gain_is_attenuation {
            maximum - (((u32::from(value) * u32::from(maximum)) / 255) as u16)
        } else {
            ((u32::from(value) * u32::from(maximum)) / 255) as u16
        };
        Ok(match id {
            ControlId::RfPower => format!("{native:03}"),
            ControlId::Preamp => format!("{native}"),
            ControlId::Attenuator => format!("{native:02}"),
            ControlId::Filter => format!("{native:04}"),
            ControlId::AfGain | ControlId::Squelch => format!("{native:03}"),
            ControlId::RfGain if profile.rf_gain_is_attenuation => format!("-{native:02}"),
            ControlId::RfGain => format!("{native:03}"),
            _ => unreachable!(),
        })
    }

    fn encode_power(profile: ElecraftProfile, value: u8) -> Result<String> {
        let maximum = profile
            .power_max_watts
            .context("Elecraft RF power is not profiled")?;
        Ok(format!("{:03}", (u16::from(value) * maximum) / 255))
    }

    fn is_split(&self) -> Result<bool> {
        let rx = Self::parse_numeric(&self.query("FR")?, "FR")?;
        let tx = Self::parse_numeric(&self.query("FT")?, "FT")?;
        Ok(rx != tx)
    }
}

fn publish_event(router: &RadioEventRouter, model: Option<ElecraftModel>, frame: &[u8]) {
    let text = String::from_utf8_lossy(frame);
    let payload = text.strip_suffix(';').unwrap_or(&text);
    if let Some(value) = payload
        .strip_prefix("FA")
        .and_then(|value| value.parse().ok())
    {
        router.publish(RadioEvent::FrequencyChanged {
            frequency_hz: value,
        });
    } else if let Some(code) = payload
        .strip_prefix("MD")
        .and_then(|value| value.chars().next())
    {
        if let Some(profile) = model.map(profile_for_model) {
            if let Ok(mode) = profile.decode_mode(code) {
                router.publish(RadioEvent::ModeChanged { mode });
                return;
            }
        }
        router.publish(RadioEvent::Raw {
            payload: frame.to_vec(),
        });
    } else if let Some(value) = payload.strip_prefix("TQ") {
        match value {
            "0" => router.publish(RadioEvent::PttChanged { enabled: false }),
            "1" => router.publish(RadioEvent::PttChanged { enabled: true }),
            _ => router.publish(RadioEvent::Raw {
                payload: frame.to_vec(),
            }),
        }
    } else if let Some(id) = [
        (ControlId::AfGain, "AG"),
        (ControlId::RfGain, "RG"),
        (ControlId::Squelch, "SQ"),
        (ControlId::RfPower, "PC"),
    ]
    .into_iter()
    .find(|(_, prefix)| payload.starts_with(prefix))
    .map(|(id, _)| id)
    {
        if let Some(profile) = model.map(profile_for_model) {
            if let Ok(value) = ElecraftRadio::decode_control(profile, id, frame) {
                router.publish(RadioEvent::ControlChanged {
                    id,
                    value: ControlValue::U8(value),
                });
                return;
            }
        }
        router.publish(RadioEvent::Raw {
            payload: frame.to_vec(),
        });
    } else if let Some(value) = payload
        .strip_prefix("SM")
        .and_then(|value| value.parse::<u16>().ok())
    {
        let maximum = if model == Some(ElecraftModel::K2) {
            15
        } else {
            30
        };
        if let Some(value) = crate::normalize_meter_level(value, maximum) {
            router.publish(RadioEvent::MeterChanged {
                id: MeterId::Signal,
                value,
            });
        } else {
            router.publish(RadioEvent::Raw {
                payload: frame.to_vec(),
            });
        }
    } else {
        router.publish(RadioEvent::Raw {
            payload: frame.to_vec(),
        });
    }
}

#[async_trait]
impl Radio for ElecraftRadio {
    fn event_router(&self) -> Option<RadioEventRouter> {
        Some(self.event_router.clone())
    }
    async fn get_frequency_hz(&self) -> Result<u64> {
        let profile = self
            .profile()
            .context("Elecraft frequency profile is unavailable")?;
        anyhow::ensure!(
            profile.can_get_frequency,
            "Elecraft frequency readback is not supported"
        );
        Self::parse_frequency(profile, &self.query(self.selected_frequency())?)
    }

    async fn set_frequency_hz(&self, frequency_hz: u64) -> Result<()> {
        if let Some(profile) = self.profile() {
            anyhow::ensure!(
                profile.can_set_frequency,
                "Elecraft frequency writes are not supported"
            );
            if !profile.supports_frequency(frequency_hz) {
                bail!(
                    "{} frequency is outside the profiled range",
                    profile.model.model_name()
                );
            }
        }
        let profile = self
            .profile()
            .context("Elecraft frequency profile is unavailable")?;
        anyhow::ensure!(
            frequency_hz.is_multiple_of(profile.frequency_scale_hz),
            "Elecraft frequency has unsupported resolution"
        );
        let value = frequency_hz / profile.frequency_scale_hz;
        self.set(
            "FA",
            &format!("{value:0width$}", width = profile.frequency_width),
        )
    }

    async fn get_mode(&self) -> Result<Mode> {
        let profile = self
            .profile()
            .context("Elecraft mode decoding requires a selected model")?;
        anyhow::ensure!(
            profile.can_get_mode,
            "Elecraft mode readback is not supported"
        );
        Self::parse_mode(profile, &self.query("MD")?)
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let profile = self
            .profile()
            .context("Elecraft mode encoding requires a selected model")?;
        anyhow::ensure!(
            profile.can_set_mode,
            "Elecraft mode writes are not supported"
        );
        self.set("MD", &profile.encode_mode(mode)?.to_string())
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        anyhow::ensure!(
            self.profile().is_some_and(|profile| profile.can_set_ptt),
            "Elecraft PTT writes are not supported"
        );
        self.set(if enabled { "TX" } else { "RX" }, "")
    }

    async fn get_ptt(&self) -> Result<bool> {
        anyhow::ensure!(
            self.profile().is_some_and(|profile| profile.can_get_ptt),
            "Elecraft PTT readback is not supported"
        );
        let response = self.query("TQ")?;
        let text = std::str::from_utf8(&response).context("Elecraft TQ response is not ASCII")?;
        match text
            .strip_prefix("TQ")
            .and_then(|v| v.strip_suffix(';').or(Some(v)))
        {
            Some("0") => Ok(false),
            Some("1") => Ok(true),
            _ => bail!("unexpected Elecraft TQ response: {text}"),
        }
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        let Some(profile) = self.profile() else {
            return Ok(None);
        };
        let command = match id {
            ControlId::AfGain => "AG",
            ControlId::RfGain => "RG",
            ControlId::Squelch => "SQ",
            ControlId::RfPower => "PC",
            ControlId::Vfo => "FR",
            ControlId::Split => return Ok(Some(ControlValue::Bool(self.is_split()?))),
            ControlId::Rit | ControlId::Xit => "IF",
            ControlId::Preamp => "PA",
            ControlId::Attenuator => "RA",
            ControlId::NoiseBlanker => "NB",
            ControlId::Agc => "GT",
            ControlId::Filter => profile.filter_command,
            ControlId::Tuner => "AT",
            ControlId::TuningStep => "VT$X",
            _ => return Ok(None),
        };
        if matches!(id, ControlId::Vfo) {
            let value = Self::parse_numeric(&self.query(command)?, command)?;
            anyhow::ensure!(value <= 1, "invalid Elecraft receive VFO");
            return Ok(Some(ControlValue::Vfo(value as u8)));
        }
        if id == ControlId::TuningStep {
            let response = self.query_with_response_prefix("VT$X", "VT$")?;
            let text = std::str::from_utf8(&response)?;
            let payload = text
                .strip_prefix("VT$")
                .and_then(|value| value.strip_suffix(';'))
                .context("unexpected Elecraft tuning-step response")?;
            let value = payload
                .chars()
                .next()
                .and_then(|value| value.to_digit(10))
                .context("invalid Elecraft tuning-step response")?;
            anyhow::ensure!(value <= 5, "Elecraft tuning-step index is out of range");
            return Ok(Some(ControlValue::U8(value as u8)));
        }
        if matches!(id, ControlId::Rit | ControlId::Xit) {
            let (_, rit, xit) = Self::parse_if_state(&self.query(command)?)?;
            return Ok(Some(ControlValue::Bool(if id == ControlId::Rit {
                rit
            } else {
                xit
            })));
        }
        if id == ControlId::NoiseBlanker {
            let value = Self::parse_numeric(&self.query(command)?, command)?;
            return Ok(Some(ControlValue::Bool(value != 0)));
        }
        if id == ControlId::Agc {
            let value = Self::parse_numeric(&self.query(command)?, command)?;
            return Ok(Some(ControlValue::U8(if value <= 2 { 0 } else { 255 })));
        }
        if id == ControlId::Tuner {
            let value = Self::parse_numeric(&self.query(command)?, command)?;
            return Ok(Some(ControlValue::Bool(value == 2)));
        }
        Ok(Some(ControlValue::U8(Self::decode_control(
            profile,
            id,
            &self.query(command)?,
        )?)))
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        let profile = self
            .profile()
            .context("Elecraft controls require a selected model")?;
        match (id, value) {
            (ControlId::RfPower, ControlValue::U8(value)) => {
                self.set("PC", &Self::encode_power(profile, value)?)
            }
            (ControlId::Vfo, ControlValue::Vfo(value)) if value <= 1 => {
                anyhow::ensure!(
                    profile.supports_vfo_b || value == 0,
                    "Elecraft VFO B is not supported"
                );
                self.set("FR", &value.to_string())
            }
            (ControlId::Split, ControlValue::Bool(enabled)) if profile.supports_split => self.set(
                if enabled { "FT" } else { "FR" },
                if enabled { "1" } else { "0" },
            ),
            (ControlId::Rit, ControlValue::Bool(enabled)) if profile.supports_rit_xit => {
                self.set("RT", if enabled { "1" } else { "0" })
            }
            (ControlId::NoiseBlanker, ControlValue::Bool(enabled))
                if profile.supports_noise_blanker =>
            {
                self.set("NB", if enabled { "1" } else { "0" })
            }
            (ControlId::Agc, ControlValue::U8(value)) if profile.supports_agc => {
                self.set("GT", if value < 128 { "002" } else { "004" })
            }
            (ControlId::Filter, ControlValue::U8(value)) if profile.filter_max_hz.is_some() => self
                .set(
                    profile.filter_command,
                    &Self::encode_control(profile, ControlId::Filter, value)?,
                ),
            (ControlId::Tuner, ControlValue::Bool(enabled)) if profile.supports_tuner => {
                self.set("AT", if enabled { "2" } else { "1" })
            }
            (ControlId::TuningStep, ControlValue::U8(value))
                if profile.supports_tuning_step && value <= 5 =>
            {
                let mode = self.get_mode().await?;
                let mode_code = profile.encode_mode(mode)?;
                self.set("VT$", &format!("{value}{mode_code}"))
            }
            (id @ (ControlId::Preamp | ControlId::Attenuator), ControlValue::U8(value)) => self
                .set(
                    if id == ControlId::Preamp { "PA" } else { "RA" },
                    &Self::encode_control(profile, id, value)?,
                ),
            (ControlId::Xit, ControlValue::Bool(enabled)) if profile.supports_rit_xit => {
                self.set("XT", if enabled { "1" } else { "0" })
            }
            (
                id @ (ControlId::AfGain | ControlId::RfGain | ControlId::Squelch),
                ControlValue::U8(value),
            ) => self.set(
                match id {
                    ControlId::AfGain => "AG",
                    ControlId::RfGain => "RG",
                    ControlId::Squelch => "SQ",
                    _ => unreachable!(),
                },
                &Self::encode_control(profile, id, value)?,
            ),
            _ => bail!("Elecraft control {id:?} is not implemented"),
        }
    }

    async fn get_rit_offset_hz(&self) -> Result<i32> {
        Ok(Self::parse_if_state(&self.query("IF")?)?.0)
    }

    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        anyhow::ensure!(
            (-9_999..=9_999).contains(&offset_hz),
            "Elecraft RIT/XIT offset must be -9999..=9999 Hz"
        );
        let magnitude = offset_hz.unsigned_abs();
        self.set(
            "RO",
            &format!("{}{magnitude:04}", if offset_hz < 0 { '-' } else { '+' }),
        )
    }

    async fn get_xit_offset_hz(&self) -> Result<i32> {
        self.get_rit_offset_hz().await
    }

    async fn set_xit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.set_rit_offset_hz(offset_hz).await
    }

    async fn start_tuner(&self) -> Result<()> {
        anyhow::ensure!(
            self.profile().is_some_and(|profile| profile.supports_tuner),
            "Elecraft tuner control is not profiled"
        );
        self.set("TU", "3")
    }

    async fn get_tuner_status(&self) -> Result<Option<crate::TunerStatus>> {
        let Some(profile) = self.profile() else {
            return Ok(None);
        };
        if !profile.supports_tuner {
            return Ok(None);
        }
        let mode = Self::parse_numeric(&self.query("AT")?, "AT")?;
        Ok(Some(crate::TunerStatus {
            enabled: mode == 2,
            tuning: false,
        }))
    }

    async fn get_repeater_settings(&self) -> Result<crate::RepeaterSettings> {
        anyhow::ensure!(
            self.profile()
                .is_some_and(|profile| profile.supports_repeater),
            "Elecraft repeater control is not profiled"
        );
        let response = self.query("RP")?;
        let text =
            std::str::from_utf8(&response).context("Elecraft repeater response is not ASCII")?;
        let payload = text
            .strip_prefix("RP")
            .and_then(|value| value.strip_suffix(';'))
            .context("unexpected Elecraft repeater response")?;
        anyhow::ensure!(payload.len() == 6, "invalid Elecraft repeater response");
        let shift = match &payload[..1] {
            "S" => crate::RepeaterShift::Simplex,
            "+" => crate::RepeaterShift::Plus,
            "-" => crate::RepeaterShift::Minus,
            _ => bail!("invalid Elecraft repeater shift"),
        };
        let offset_hz = payload[1..].parse::<u32>()? * 1_000;
        Ok(crate::RepeaterSettings {
            shift,
            offset_hz: Some(offset_hz),
            tone: crate::ToneSettings::default(),
        })
    }

    async fn set_repeater_settings(&self, settings: crate::RepeaterSettings) -> Result<()> {
        anyhow::ensure!(
            self.profile()
                .is_some_and(|profile| profile.supports_repeater),
            "Elecraft repeater control is not profiled"
        );
        anyhow::ensure!(
            settings.tone == crate::ToneSettings::default(),
            "Elecraft K4 CAT does not expose tone fields through RP"
        );
        let offset_hz = settings.offset_hz.unwrap_or_default();
        anyhow::ensure!(
            offset_hz.is_multiple_of(1_000),
            "Elecraft RP offset must use whole kHz"
        );
        anyhow::ensure!(offset_hz <= 99_999_000, "Elecraft RP offset is too large");
        let shift = match settings.shift {
            crate::RepeaterShift::Simplex => 'S',
            crate::RepeaterShift::Plus => '+',
            crate::RepeaterShift::Minus => '-',
        };
        self.set("RP", &format!("{shift}{:05}", offset_hz / 1_000))
    }

    fn supports_repeater_settings(&self) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_repeater)
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        let value = match id {
            MeterId::Signal => {
                let response = self.query("SM")?;
                Self::parse_meter_value(
                    &response,
                    "SM",
                    if matches!(self.model, Some(ElecraftModel::K2)) {
                        15
                    } else {
                        30
                    },
                )?
            }
            MeterId::Power => Self::parse_meter_value(&self.query("BG")?, "BG", 12)?,
            MeterId::Alc => {
                anyhow::ensure!(
                    matches!(self.model, Some(ElecraftModel::K3 | ElecraftModel::K3s)),
                    "Elecraft ALC meter is only profiled for K3/K3S"
                );
                Self::parse_meter_value(&self.query("BG")?, "BG", 7)?
            }
            MeterId::Swr => Self::parse_meter_value(&self.query("SW")?, "SW", 999)?,
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        match id {
            MeterId::Signal | MeterId::Power | MeterId::Swr => self.model.is_some(),
            MeterId::Alc => matches!(self.model, Some(ElecraftModel::K3 | ElecraftModel::K3s)),
            _ => false,
        }
    }
    fn supports_control(&self, id: ControlId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_control(id))
    }
    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: self
                .profile()
                .is_some_and(|profile| profile.can_get_frequency),
            can_set_frequency: self
                .profile()
                .is_some_and(|profile| profile.can_set_frequency),
            can_get_mode: self.profile().is_some_and(|profile| profile.can_get_mode),
            can_set_mode: self.profile().is_some_and(|profile| profile.can_set_mode),
            can_get_ptt: self.profile().is_some_and(|profile| profile.can_get_ptt),
            can_set_ptt: self.profile().is_some_and(|profile| profile.can_set_ptt),
            can_get_power: false,
            can_set_power: false,
            can_raw_protocol: true,
        }
    }

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        let mut command = request.to_vec();
        if !command.ends_with(b";") {
            command.push(b';');
        }
        self.transport
            .transact(&command, Some(&command[..command.len() - 1]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    struct MemoryTransport {
        input: Vec<u8>,
        output: Arc<Mutex<Vec<u8>>>,
    }

    impl Read for MemoryTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let count = buffer.len().min(self.input.len());
            buffer[..count].copy_from_slice(&self.input[..count]);
            self.input.drain(..count);
            Ok(count)
        }
    }
    impl Write for MemoryTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.output.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl RadioTransport for MemoryTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn k3_core_commands_use_documented_frames() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let transport = MemoryTransport {
            input: b"FA00014060000;MD2;TQ0;SM00015;".to_vec(),
            output: Arc::clone(&output),
        };
        let radio =
            ElecraftRadio::with_external_transport(Some(ElecraftModel::K3), 9_600, transport)
                .unwrap();
        assert_eq!(block_on(radio.get_frequency_hz()).unwrap(), 14_060_000);
        assert_eq!(block_on(radio.get_mode()).unwrap(), Mode::Usb);
        assert!(!block_on(radio.get_ptt()).unwrap());
        assert_eq!(
            block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(128)
        );
        block_on(radio.set_frequency_hz(14_074_000)).unwrap();
        block_on(radio.set_mode(Mode::Cw)).unwrap();
        block_on(radio.set_ptt(true)).unwrap();
        assert_eq!(
            &*output.lock().unwrap(),
            b"FA;MD;TQ;SM;FA00014074000;MD3;TX;"
        );
    }

    #[test]
    fn k2_profile_preserves_legacy_mode_and_meter_limits() {
        let transport = MemoryTransport {
            input: b"MD6;SM0015;".to_vec(),
            output: Arc::new(Mutex::new(Vec::new())),
        };
        let radio =
            ElecraftRadio::with_external_transport(Some(ElecraftModel::K2), 4_800, transport)
                .unwrap();
        assert_eq!(block_on(radio.get_mode()).unwrap(), Mode::Rtty);
        assert_eq!(
            block_on(radio.get_meter(MeterId::Signal)).unwrap(),
            Some(255)
        );
    }

    #[test]
    fn auto_info_frames_are_routed_while_querying() {
        let transport = MemoryTransport {
            input: b"MD2;FA00014060000;".to_vec(),
            output: Arc::new(Mutex::new(Vec::new())),
        };
        let radio =
            ElecraftRadio::with_external_transport(Some(ElecraftModel::K3), 9_600, transport)
                .unwrap();
        let subscription = radio.event_router().subscribe();
        assert_eq!(block_on(radio.get_frequency_hz()).unwrap(), 14_060_000);
        assert_eq!(
            subscription.drain(),
            vec![RadioEvent::ModeChanged { mode: Mode::Usb }]
        );
    }

    #[test]
    fn receiver_controls_use_model_native_ranges() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K4),
            9_600,
            MemoryTransport {
                input: b"AG030;RG-30;SQ020;".to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        assert_eq!(
            block_on(radio.get_control(ControlId::AfGain)).unwrap(),
            Some(ControlValue::U8(127))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::RfGain)).unwrap(),
            Some(ControlValue::U8(127))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::Squelch)).unwrap(),
            Some(ControlValue::U8(127))
        );
        block_on(radio.set_control(ControlId::RfGain, ControlValue::U8(255))).unwrap();
        assert!(String::from_utf8(output.lock().unwrap().clone())
            .unwrap()
            .ends_with("RG-00;"));
    }

    #[test]
    fn direct_cat_vfo_split_rit_xit_power_and_identification_are_profiled() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K3),
            9_600,
            MemoryTransport {
                input:
                    b"ID017;PC055;FR0;FR0;FT1;IF00014060000 +012310100;IF00014060000 +012310100;"
                        .to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        assert_eq!(radio.identify().unwrap(), b"ID017;".to_vec());
        assert_eq!(
            block_on(radio.get_control(ControlId::RfPower)).unwrap(),
            Some(ControlValue::U8(127))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::Vfo)).unwrap(),
            Some(ControlValue::Vfo(0))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::Split)).unwrap(),
            Some(ControlValue::Bool(true))
        );
        assert_eq!(block_on(radio.get_rit_offset_hz()).unwrap(), 123);
        assert_eq!(block_on(radio.get_xit_offset_hz()).unwrap(), 123);
        block_on(radio.set_control(ControlId::RfPower, ControlValue::U8(255))).unwrap();
        block_on(radio.set_control(ControlId::Vfo, ControlValue::Vfo(0))).unwrap();
        block_on(radio.set_control(ControlId::Split, ControlValue::Bool(true))).unwrap();
        block_on(radio.set_control(ControlId::Rit, ControlValue::Bool(true))).unwrap();
        block_on(radio.set_control(ControlId::Xit, ControlValue::Bool(false))).unwrap();
        block_on(radio.set_rit_offset_hz(-999)).unwrap();
        assert_eq!(
            &*output.lock().unwrap(),
            b"ID;PC;FR;FR;FT;IF;IF;PC110;FR0;FT1;RT1;XT0;RO-0999;"
        );
    }

    #[test]
    fn direct_cat_receiver_controls_use_profile_ranges() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K3),
            9_600,
            MemoryTransport {
                input: b"PA2;RA01;NB1;GT004;".to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        assert_eq!(
            block_on(radio.get_control(ControlId::Preamp)).unwrap(),
            Some(ControlValue::U8(255))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::Attenuator)).unwrap(),
            Some(ControlValue::U8(255))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::NoiseBlanker)).unwrap(),
            Some(ControlValue::Bool(true))
        );
        assert_eq!(
            block_on(radio.get_control(ControlId::Agc)).unwrap(),
            Some(ControlValue::U8(255))
        );
        block_on(radio.set_control(ControlId::Preamp, ControlValue::U8(255))).unwrap();
        block_on(radio.set_control(ControlId::Attenuator, ControlValue::U8(255))).unwrap();
        block_on(radio.set_control(ControlId::NoiseBlanker, ControlValue::Bool(false))).unwrap();
        block_on(radio.set_control(ControlId::Agc, ControlValue::U8(0))).unwrap();
        assert_eq!(&*output.lock().unwrap(), b"PA;RA;NB;GT;PA2;RA01;NB0;GT002;");
    }

    #[test]
    fn direct_cat_tx_meters_decode_documented_bargraph_and_swr_frames() {
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K3),
            9_600,
            MemoryTransport {
                input: b"BG12T;BG07T;SW123;".to_vec(),
                output: Arc::new(Mutex::new(Vec::new())),
            },
        )
        .unwrap();
        assert_eq!(
            block_on(radio.get_meter(MeterId::Power)).unwrap(),
            Some(255)
        );
        assert_eq!(block_on(radio.get_meter(MeterId::Alc)).unwrap(), Some(255));
        assert_eq!(block_on(radio.get_meter(MeterId::Swr)).unwrap(), Some(31));
    }

    #[test]
    fn direct_cat_filter_uses_model_owned_command_and_bandwidth_range() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K3),
            9_600,
            MemoryTransport {
                input: b"BW0250;".to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        assert_eq!(
            block_on(radio.get_control(ControlId::Filter)).unwrap(),
            Some(ControlValue::U8(6))
        );
        block_on(radio.set_control(ControlId::Filter, ControlValue::U8(255))).unwrap();
        assert_eq!(&*output.lock().unwrap(), b"BW;BW9999;");
    }

    #[test]
    fn k4_tuner_control_uses_at_mode_and_tu3_start() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K4),
            9_600,
            MemoryTransport {
                input: b"AT2;AT2;".to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        assert_eq!(
            block_on(radio.get_control(ControlId::Tuner)).unwrap(),
            Some(ControlValue::Bool(true))
        );
        assert_eq!(
            block_on(radio.get_tuner_status()).unwrap(),
            Some(crate::TunerStatus {
                enabled: true,
                tuning: false,
            })
        );
        block_on(radio.set_control(ControlId::Tuner, ControlValue::Bool(false))).unwrap();
        block_on(radio.start_tuner()).unwrap();
        assert_eq!(&*output.lock().unwrap(), b"AT;AT;AT1;TU3;");
    }

    #[test]
    fn k4_repeater_control_preserves_documented_shift_and_khz_offset() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K4),
            9_600,
            MemoryTransport {
                input: b"RP+00600;".to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        let settings = block_on(radio.get_repeater_settings()).unwrap();
        assert_eq!(settings.shift, crate::RepeaterShift::Plus);
        assert_eq!(settings.offset_hz, Some(600_000));
        block_on(radio.set_repeater_settings(settings)).unwrap();
        assert_eq!(&*output.lock().unwrap(), b"RP;RP+00600;");
    }

    #[test]
    fn k4_tuning_step_matches_vt_mode_qualified_frames() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::K4),
            9_600,
            MemoryTransport {
                input: b"VT$02;MD2;".to_vec(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        assert_eq!(
            block_on(radio.get_control(ControlId::TuningStep)).unwrap(),
            Some(ControlValue::U8(0))
        );
        block_on(radio.set_control(ControlId::TuningStep, ControlValue::U8(3))).unwrap();
        assert_eq!(&*output.lock().unwrap(), b"VT$X;MD;VT$32;");
    }

    #[test]
    fn kh1_uses_set_only_frequency_and_mode_commands() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let radio = ElecraftRadio::with_external_transport(
            Some(ElecraftModel::Kh1),
            9_600,
            MemoryTransport {
                input: Vec::new(),
                output: Arc::clone(&output),
            },
        )
        .unwrap();
        block_on(radio.set_frequency_hz(14_000_000)).unwrap();
        block_on(radio.set_mode(Mode::Usb)).unwrap();
        assert!(block_on(radio.get_frequency_hz()).is_err());
        assert!(block_on(radio.get_mode()).is_err());
        assert!(block_on(radio.set_ptt(true)).is_err());
        assert_eq!(&*output.lock().unwrap(), b"FA1400000;MD2;");
    }
}
