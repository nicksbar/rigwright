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

    fn parse_frequency(response: &[u8]) -> Result<u64> {
        let text =
            std::str::from_utf8(response).context("Elecraft frequency response is not ASCII")?;
        text.strip_prefix("FA")
            .and_then(|v| v.strip_suffix(';'))
            .context("unexpected Elecraft frequency response")?
            .parse()
            .context("invalid Elecraft frequency")
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

    fn decode_control(profile: ElecraftProfile, id: ControlId, response: &[u8]) -> Result<u8> {
        let text =
            std::str::from_utf8(response).context("Elecraft control response is not ASCII")?;
        let raw = match id {
            ControlId::AfGain => text.strip_prefix("AG"),
            ControlId::RfGain => text.strip_prefix("RG"),
            ControlId::Squelch => text.strip_prefix("SQ"),
            ControlId::RfPower => text.strip_prefix("PC"),
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
            _ => None,
        }
        .context("Elecraft control is not profiled")?;
        if native > maximum {
            bail!("Elecraft control value exceeds profile maximum");
        }
        Ok(if id == ControlId::RfPower {
            ((native * 255) / maximum) as u8
        } else if id == ControlId::RfGain && profile.rf_gain_is_attenuation {
            (((maximum - native) * 255) / maximum) as u8
        } else {
            ((native * 255) / maximum) as u8
        })
    }

    fn encode_control(profile: ElecraftProfile, id: ControlId, value: u8) -> Result<String> {
        let maximum = match id {
            ControlId::AfGain => profile.af_gain_max,
            ControlId::RfGain => profile.rf_gain_max,
            ControlId::Squelch => Some(profile.squelch_max),
            ControlId::RfPower => profile.power_max_watts,
            _ => None,
        }
        .context("Elecraft control is not profiled")?;
        let native = if id == ControlId::RfGain && profile.rf_gain_is_attenuation {
            maximum - ((u16::from(value) * maximum) / 255)
        } else {
            (u16::from(value) * maximum) / 255
        };
        Ok(match id {
            ControlId::RfPower => format!("{native:03}"),
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
        Self::parse_frequency(&self.query(self.selected_frequency())?)
    }

    async fn set_frequency_hz(&self, frequency_hz: u64) -> Result<()> {
        if let Some(profile) = self.profile() {
            if !profile.supports_frequency(frequency_hz) {
                bail!(
                    "{} frequency is outside the profiled range",
                    profile.model.model_name()
                );
            }
        }
        self.set("FA", &format!("{frequency_hz:011}"))
    }

    async fn get_mode(&self) -> Result<Mode> {
        let profile = self
            .profile()
            .context("Elecraft mode decoding requires a selected model")?;
        Self::parse_mode(profile, &self.query("MD")?)
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        let profile = self
            .profile()
            .context("Elecraft mode encoding requires a selected model")?;
        self.set("MD", &profile.encode_mode(mode)?.to_string())
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.set(if enabled { "TX" } else { "RX" }, "")
    }

    async fn get_ptt(&self) -> Result<bool> {
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
            _ => return Ok(None),
        };
        if matches!(id, ControlId::Vfo) {
            let value = Self::parse_numeric(&self.query(command)?, command)?;
            anyhow::ensure!(value <= 1, "invalid Elecraft receive VFO");
            return Ok(Some(ControlValue::Vfo(value as u8)));
        }
        if matches!(id, ControlId::Rit | ControlId::Xit) {
            let (_, rit, xit) = Self::parse_if_state(&self.query(command)?)?;
            return Ok(Some(ControlValue::Bool(if id == ControlId::Rit {
                rit
            } else {
                xit
            })));
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

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        if id != MeterId::Signal {
            return Ok(None);
        }
        let response = self.query("SM")?;
        let value = std::str::from_utf8(&response)
            .context("Elecraft S-meter response is not ASCII")?
            .trim_end_matches(';')
            .strip_prefix("SM")
            .context("unexpected Elecraft S-meter response")?
            .parse::<u16>()
            .context("invalid Elecraft S-meter")?;
        Ok(Some(
            crate::normalize_meter_level(
                value,
                if matches!(self.model, Some(ElecraftModel::K2)) {
                    15
                } else {
                    30
                },
            )
            .context("Elecraft S-meter out of range")?,
        ))
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        id == MeterId::Signal
    }
    fn supports_control(&self, id: ControlId) -> bool {
        self.profile()
            .is_some_and(|profile| profile.supports_control(id))
    }
    fn capabilities(&self) -> RadioCapabilities {
        RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: self.model.is_some(),
            can_set_mode: self.model.is_some(),
            can_get_ptt: true,
            can_set_ptt: true,
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
}
