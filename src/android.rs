//! Android-facing radio implementations.
//!
//! Android USB permission and endpoint handling remains outside Rigwright.
//! QSONoid supplies a configured byte transport; Rigwright retains the radio
//! protocol and capability behavior.

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    elecraft::ElecraftRadio,
    icom::{CiVTransport, IcomCiVRadio},
    kenwood::KenwoodCatRadio,
    yaesu::{LegacyYaesuRadio, YaesuCatRadio},
    ControlId, ControlValue, DtmfSequence, MemoryChannel, MeterId, Mode, Radio, RadioCapabilities,
    RadioTransport, RepeaterSettings, TunerStatus,
};

/// Android radio entry point.
///
/// Supports all current Rigwright protocol families without changing the
/// `Radio` trait or transport contract. Android provides the configured byte
/// stream; each family retains its existing profiles and protocol engine.
pub struct RadioAndroid {
    inner: AndroidRadioFamily,
}

enum AndroidRadioFamily {
    Icom(IcomCiVRadio),
    Yaesu(YaesuCatRadio),
    YaesuLegacy(LegacyYaesuRadio),
    Kenwood(KenwoodCatRadio),
    Elecraft(ElecraftRadio),
}

impl RadioAndroid {
    /// Create an Android Icom CI-V radio over an already-configured transport.
    pub fn new_icom_civ<T>(
        model: Option<crate::models::IcomCivModel>,
        controller_address: u8,
        radio_address: u8,
        transport: T,
    ) -> Self
    where
        T: CiVTransport + 'static,
    {
        Self {
            inner: AndroidRadioFamily::Icom(IcomCiVRadio::with_transport(
                model,
                controller_address,
                radio_address,
                transport,
            )),
        }
    }

    /// Create a modern Yaesu CAT radio over an Android-provided transport.
    pub fn new_yaesu<T>(
        model: Option<crate::models::YaesuCatModel>,
        baud_rate: u32,
        transport: T,
    ) -> Self
    where
        T: RadioTransport + 'static,
    {
        Self {
            inner: AndroidRadioFamily::Yaesu(YaesuCatRadio::with_external_transport(
                model, baud_rate, transport,
            )),
        }
    }

    /// Create a classic five-byte Yaesu CAT radio over an Android-provided
    /// transport.
    pub fn new_yaesu_legacy<T>(
        model: Option<crate::models::YaesuLegacyModel>,
        baud_rate: u32,
        transport: T,
    ) -> Self
    where
        T: RadioTransport + 'static,
    {
        Self {
            inner: AndroidRadioFamily::YaesuLegacy(LegacyYaesuRadio::with_transport(
                model, baud_rate, transport,
            )),
        }
    }

    /// Create a Kenwood CAT radio over an Android-provided transport.
    pub fn new_kenwood<T>(
        model: Option<crate::models::KenwoodCatModel>,
        baud_rate: u32,
        transport: T,
    ) -> Self
    where
        T: RadioTransport + 'static,
    {
        Self {
            inner: AndroidRadioFamily::Kenwood(KenwoodCatRadio::with_external_transport(
                model, baud_rate, transport,
            )),
        }
    }

    /// Create an Elecraft transceiver over an Android-provided transport.
    pub fn new_elecraft<T>(
        model: Option<crate::ElecraftModel>,
        baud_rate: u32,
        transport: T,
    ) -> Result<Self>
    where
        T: RadioTransport + 'static,
    {
        Ok(Self {
            inner: AndroidRadioFamily::Elecraft(ElecraftRadio::with_external_transport(
                model, baud_rate, transport,
            )?),
        })
    }

    /// Return the underlying Icom driver when this instance is Icom CI-V.
    pub fn icom(&self) -> Option<&IcomCiVRadio> {
        match &self.inner {
            AndroidRadioFamily::Icom(radio) => Some(radio),
            _ => None,
        }
    }

    fn radio(&self) -> &dyn Radio {
        match &self.inner {
            AndroidRadioFamily::Icom(radio) => radio,
            AndroidRadioFamily::Yaesu(radio) => radio,
            AndroidRadioFamily::YaesuLegacy(radio) => radio,
            AndroidRadioFamily::Kenwood(radio) => radio,
            AndroidRadioFamily::Elecraft(radio) => radio,
        }
    }
}

#[async_trait]
impl Radio for RadioAndroid {
    fn event_router(&self) -> Option<crate::RadioEventRouter> {
        self.radio().event_router()
    }

    async fn get_frequency_hz(&self) -> Result<u64> {
        self.radio().get_frequency_hz().await
    }

    async fn set_frequency_hz(&self, hz: u64) -> Result<()> {
        self.radio().set_frequency_hz(hz).await
    }

    async fn get_mode(&self) -> Result<Mode> {
        self.radio().get_mode().await
    }

    async fn set_mode(&self, mode: Mode) -> Result<()> {
        self.radio().set_mode(mode).await
    }

    async fn set_ptt(&self, enabled: bool) -> Result<()> {
        self.radio().set_ptt(enabled).await
    }

    async fn get_ptt(&self) -> Result<bool> {
        self.radio().get_ptt().await
    }

    async fn get_power(&self) -> Result<bool> {
        self.radio().get_power().await
    }

    async fn set_power(&self, enabled: bool) -> Result<()> {
        self.radio().set_power(enabled).await
    }

    async fn protocol_write_read(&self, request: &[u8]) -> Result<Vec<u8>> {
        self.radio().protocol_write_read(request).await
    }

    async fn get_control(&self, id: ControlId) -> Result<Option<ControlValue>> {
        self.radio().get_control(id).await
    }

    async fn set_control(&self, id: ControlId, value: ControlValue) -> Result<()> {
        self.radio().set_control(id, value).await
    }

    async fn get_repeater_settings(&self) -> Result<RepeaterSettings> {
        self.radio().get_repeater_settings().await
    }
    async fn set_repeater_settings(&self, settings: RepeaterSettings) -> Result<()> {
        self.radio().set_repeater_settings(settings).await
    }
    async fn get_rit_offset_hz(&self) -> Result<i32> {
        self.radio().get_rit_offset_hz().await
    }
    async fn set_rit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.radio().set_rit_offset_hz(offset_hz).await
    }
    async fn get_xit_offset_hz(&self) -> Result<i32> {
        self.radio().get_xit_offset_hz().await
    }
    async fn set_xit_offset_hz(&self, offset_hz: i32) -> Result<()> {
        self.radio().set_xit_offset_hz(offset_hz).await
    }
    async fn select_memory_channel(&self, channel: u16) -> Result<()> {
        self.radio().select_memory_channel(channel).await
    }
    async fn read_memory_channel(&self, channel: u16) -> Result<MemoryChannel> {
        self.radio().read_memory_channel(channel).await
    }
    async fn write_memory_channel(&self, channel: MemoryChannel) -> Result<()> {
        self.radio().write_memory_channel(channel).await
    }
    async fn send_dtmf(&self, sequence: DtmfSequence) -> Result<()> {
        self.radio().send_dtmf(sequence).await
    }
    fn supports_repeater_settings(&self) -> bool {
        self.radio().supports_repeater_settings()
    }
    fn supports_memory_channels(&self) -> bool {
        self.radio().supports_memory_channels()
    }
    fn supports_send_dtmf(&self) -> bool {
        self.radio().supports_send_dtmf()
    }

    async fn get_meter(&self, id: MeterId) -> Result<Option<u8>> {
        self.radio().get_meter(id).await
    }

    fn supports_meter(&self, id: MeterId) -> bool {
        self.radio().supports_meter(id)
    }

    fn supports_control(&self, id: ControlId) -> bool {
        self.radio().supports_control(id)
    }

    fn supports_control_read(&self, id: ControlId) -> bool {
        self.radio().supports_control_read(id)
    }

    fn supports_control_write(&self, id: ControlId) -> bool {
        self.radio().supports_control_write(id)
    }

    async fn start_tuner(&self) -> Result<()> {
        self.radio().start_tuner().await
    }

    async fn get_tuner_status(&self) -> Result<Option<TunerStatus>> {
        self.radio().get_tuner_status().await
    }

    fn capabilities(&self) -> RadioCapabilities {
        self.radio().capabilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{Error, ErrorKind, Read, Write};
    use std::time::Duration;

    struct MockCiVTransport {
        response: VecDeque<u8>,
        max_read: usize,
    }

    impl MockCiVTransport {
        fn new() -> Self {
            Self {
                response: VecDeque::new(),
                max_read: usize::MAX,
            }
        }
    }

    impl Read for MockCiVTransport {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.response.is_empty() {
                return Err(Error::new(ErrorKind::TimedOut, "mock response pending"));
            }
            let count = buffer.len().min(self.response.len()).min(self.max_read);
            for slot in &mut buffer[..count] {
                *slot = self.response.pop_front().expect("count is bounded");
            }
            Ok(count)
        }
    }

    impl Write for MockCiVTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            // Respond to a frequency query with the captured CI-V shape used
            // by the existing parser tests: 7,188,000 Hz.
            if buffer == [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD] {
                self.response.extend([
                    0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x80, 0x18, 0x07, 0x00, 0xFD,
                ]);
            } else if buffer.starts_with(&[0xFE, 0xFE, 0x94, 0xE0]) {
                self.response.extend([0xFE, 0xFE, 0xE0, 0x94, 0xFB, 0xFD]);
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl CiVTransport for MockCiVTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct NoopTransport;

    impl Read for NoopTransport {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(Error::new(ErrorKind::TimedOut, "no scripted response"))
        }
    }

    impl Write for NoopTransport {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl RadioTransport for NoopTransport {
        fn set_timeout(&mut self, _timeout: Duration) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn android_radio_uses_injected_transport_and_existing_civ_parser() {
        let radio = RadioAndroid::new_icom_civ(None, 0xE0, 0x94, MockCiVTransport::new());
        let frequency = futures::executor::block_on(radio.get_frequency_hz()).unwrap();
        assert_eq!(frequency, 7_188_000);
    }

    #[test]
    fn android_radio_handles_fragmented_reads_and_command_ack() {
        let mut transport = MockCiVTransport::new();
        transport.max_read = 2;
        let radio = RadioAndroid::new_icom_civ(None, 0xE0, 0x94, transport);

        assert_eq!(
            futures::executor::block_on(radio.get_frequency_hz()).unwrap(),
            7_188_000
        );
        futures::executor::block_on(radio.set_frequency_hz(7_074_000)).unwrap();
    }

    #[test]
    fn android_entry_point_constructs_every_rigwright_family() {
        let radios: Vec<Box<dyn Radio>> = vec![
            Box::new(RadioAndroid::new_icom_civ(
                Some(crate::models::IcomCivModel::Ic7300),
                0xE0,
                0x94,
                NoopTransport,
            )),
            Box::new(RadioAndroid::new_yaesu(
                Some(crate::models::YaesuCatModel::Ft710),
                9_600,
                NoopTransport,
            )),
            Box::new(RadioAndroid::new_yaesu_legacy(
                Some(crate::models::YaesuLegacyModel::Ft817Nd),
                9_600,
                NoopTransport,
            )),
            Box::new(RadioAndroid::new_kenwood(
                Some(crate::models::KenwoodCatModel::Ts590Sg),
                9_600,
                NoopTransport,
            )),
            Box::new(
                RadioAndroid::new_elecraft(Some(crate::ElecraftModel::K4), 9_600, NoopTransport)
                    .unwrap(),
            ),
        ];

        assert!(radios
            .iter()
            .all(|radio| radio.capabilities().can_get_frequency));
    }

    #[test]
    fn android_adapter_forwards_the_shared_radio_contract() {
        let radio = RadioAndroid::new_icom_civ(
            Some(crate::models::IcomCivModel::Ic7300),
            0xE0,
            0x94,
            MockCiVTransport::new(),
        );
        assert!(radio.icom().is_some());
        assert!(radio.event_router().is_some());
        assert!(radio.supports_control(ControlId::RfPower));
        assert!(radio.supports_control_read(ControlId::RfPower));
        assert!(radio.supports_control_write(ControlId::RfPower));
        assert!(radio.supports_meter(MeterId::Signal));
        assert!(radio.supports_repeater_settings());
        assert!(radio.supports_memory_channels());
        assert!(!radio.supports_send_dtmf());
        assert!(radio.capabilities().can_set_frequency);

        futures::executor::block_on(radio.set_frequency_hz(7_074_000)).unwrap();
        futures::executor::block_on(radio.set_ptt(true)).unwrap();
        futures::executor::block_on(radio.set_power(false)).unwrap();
        futures::executor::block_on(radio.set_control(ControlId::Xit, ControlValue::Bool(true)))
            .unwrap();
        futures::executor::block_on(radio.set_control(ControlId::RfPower, ControlValue::U8(50)))
            .unwrap();
        futures::executor::block_on(radio.set_rit_offset_hz(125)).unwrap();
        assert!(futures::executor::block_on(radio.set_xit_offset_hz(-125)).is_err());
        futures::executor::block_on(radio.select_memory_channel(3)).unwrap();
        futures::executor::block_on(radio.start_tuner()).unwrap();
        assert!(futures::executor::block_on(radio.get_power()).is_err());
        assert_eq!(
            futures::executor::block_on(radio.get_control(ControlId::RawCiV)).unwrap(),
            None
        );
        assert!(futures::executor::block_on(
            radio.set_control(ControlId::RawCiV, ControlValue::U8(0),)
        )
        .is_err());
    }
}
