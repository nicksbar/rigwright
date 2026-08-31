//! Protocol-neutral HAL value types.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Usb,
    Lsb,
    Cw,
    Data,
    Am,
    Fm,
    Wfm,
    Rtty,
    CwReverse,
    RttyReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseMode {
    Lsb,
    Usb,
    Am,
    Cw,
    Rtty,
    Fm,
    Wfm,
    CwR,
    RttyR,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatingMode {
    pub base: BaseMode,
    pub data_mode: bool,
    pub filter: Option<u8>,
}

impl OperatingMode {
    pub fn label(self) -> String {
        let base = match self.base {
            BaseMode::Lsb => "LSB",
            BaseMode::Usb => "USB",
            BaseMode::Am => "AM",
            BaseMode::Cw => "CW",
            BaseMode::Rtty => "RTTY",
            BaseMode::Fm => "FM",
            BaseMode::Wfm => "WFM",
            BaseMode::CwR => "CW-R",
            BaseMode::RttyR => "RTTY-R",
            BaseMode::Unknown(v) => return format!("MODE_{v:#04x}"),
        };
        if self.data_mode
            && matches!(
                self.base,
                BaseMode::Lsb | BaseMode::Usb | BaseMode::Am | BaseMode::Fm
            )
        {
            format!("{base}-D")
        } else {
            base.to_string()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlId {
    AfGain,
    RfGain,
    Squelch,
    /// Normalized output-power level (`ControlValue::U8`, 0-255). Vendor
    /// drivers convert this to their native unit; use a vendor-specific watts
    /// method when an exact power setting is required.
    RfPower,
    Preamp,
    Attenuator,
    NoiseBlanker,
    NoiseReduction,
    /// Noise-reduction depth (`ControlValue::U8`), where supported.
    NoiseReductionLevel,
    /// Icom IP Plus receiver optimization.
    IpPlus,
    /// Auto-notch enable/disable.
    Notch,
    /// Manual-notch enable/disable; position is a separate model-specific setting.
    ManualNotch,
    /// Manual-notch center/position, normalized to 0..=255 where supported.
    ManualNotchPosition,
    DataMode,
    Filter,
    /// Vendor-native tuning-step selector where the protocol exposes one.
    TuningStep,
    Agc,
    Rit,
    Xit,
    Split,
    Tuner,
    RawCiV,
    Vfo,
    MainSub,
    ExternalPreamp,
    /// Model-specific antenna connector selection.
    Antenna,
}

impl ControlId {
    /// Complete inventory used by consumers that need to discover a driver's
    /// typed control surface. Drivers still decide which entries they support.
    pub const ALL: &'static [Self] = &[
        Self::AfGain,
        Self::RfGain,
        Self::Squelch,
        Self::RfPower,
        Self::Preamp,
        Self::Attenuator,
        Self::NoiseBlanker,
        Self::NoiseReduction,
        Self::NoiseReductionLevel,
        Self::IpPlus,
        Self::Notch,
        Self::ManualNotch,
        Self::ManualNotchPosition,
        Self::DataMode,
        Self::Filter,
        Self::TuningStep,
        Self::Agc,
        Self::Rit,
        Self::Xit,
        Self::Split,
        Self::Tuner,
        Self::RawCiV,
        Self::Vfo,
        Self::MainSub,
        Self::ExternalPreamp,
        Self::Antenna,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeterId {
    /// Receive signal-strength meter, normalized to 0..=255.
    Signal,
    /// Relative RF output-power meter, normalized to 0..=255.
    Power,
    /// Transmit SWR meter, normalized by the driver to a 0..=255 meter level.
    Swr,
    /// Transmit ALC meter, normalized to 0..=255.
    Alc,
    /// Speech/data compressor meter, normalized to 0..=255.
    Compression,
    /// PA drain/current meter, normalized to 0..=255.
    Current,
    /// PA voltage meter, normalized to 0..=255.
    Voltage,
    /// PA temperature meter, normalized to 0..=255.
    Temperature,
}

impl MeterId {
    /// Complete inventory used by consumers that need to discover a driver's
    /// normalized meter surface.
    pub const ALL: &'static [Self] = &[
        Self::Signal,
        Self::Power,
        Self::Swr,
        Self::Alc,
        Self::Compression,
        Self::Current,
        Self::Voltage,
        Self::Temperature,
    ];
}

/// Normalize a vendor meter-dot value to the HAL's common 0..=255 scale.
///
/// The value represents meter deflection, not a physical SWR ratio. Drivers
/// must use a documented vendor maximum and reject values above it.
pub fn normalize_meter_level(value: u16, vendor_max: u16) -> Option<u8> {
    if vendor_max == 0 || value > vendor_max {
        return None;
    }
    Some((((u32::from(value) * 255) + u32::from(vendor_max) / 2) / u32::from(vendor_max)) as u8)
}

#[cfg(test)]
mod meter_tests {
    use super::normalize_meter_level;

    #[test]
    fn normalizes_vendor_meter_ranges() {
        assert_eq!(normalize_meter_level(0, 30), Some(0));
        assert_eq!(normalize_meter_level(15, 30), Some(128));
        assert_eq!(normalize_meter_level(30, 30), Some(255));
        assert_eq!(normalize_meter_level(1, 3), Some(85));
        assert_eq!(normalize_meter_level(2, 3), Some(170));
        assert_eq!(normalize_meter_level(255, 255), Some(255));
        assert_eq!(normalize_meter_level(31, 30), None);
        assert_eq!(normalize_meter_level(0, 0), None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TunerStatus {
    pub enabled: bool,
    pub tuning: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ControlValue {
    Bool(bool),
    U8(u8),
    I32(i32),
    U64(u64),
    Mode(Mode),
    Vfo(u8),
    Receiver(u8),
    Text(String),
    Raw(Vec<u8>),
}

#[cfg(test)]
mod control_value_tests {
    use super::DtmfSequence;

    #[test]
    fn dtmf_sequences_are_strictly_validated() {
        assert_eq!(
            DtmfSequence::new("1800DIAL#").unwrap_err().to_string(),
            "DTMF sequence contains an invalid digit"
        );
        assert!(DtmfSequence::new("1800DIAL").is_err());
        assert_eq!(DtmfSequence::new("*21#AB09").unwrap().as_str(), "*21#AB09");
        assert!(DtmfSequence::new("").is_err());
    }
}

/// Repeater tone operation.  `EncodeDecode` is the common repeater mode
/// exposed by radios which call it CTCSS ENC/DEC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToneMode {
    #[default]
    Off,
    Encode,
    EncodeDecode,
    /// Digital tone code squelch (DTCS), used by Icom VHF/UHF memory records.
    Dtcs,
}

/// A documented analog tone setting.  The index is retained because several
/// CAT protocols identify tones by an index rather than by frequency and do
/// not expose a frequency in their readback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToneSettings {
    pub mode: ToneMode,
    pub index: u8,
    /// Icom CI-V represents tone frequencies in tenths of a hertz (for
    /// example 885 means 88.5 Hz). Other protocols may leave this unset and
    /// use their documented tone index instead.
    pub frequency_tenths_hz: Option<u32>,
    /// Optional Icom DTCS code (000..=999).
    pub dtcs_code: Option<u16>,
    /// DTCS polarity: false normal, true reverse.
    pub dtcs_reverse: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeaterShift {
    #[default]
    Simplex,
    Plus,
    Minus,
}

/// Repeater-related state.  `offset_hz` is optional because some radios only
/// document a plus/minus shift selector through CAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepeaterSettings {
    pub shift: RepeaterShift,
    pub offset_hz: Option<u32>,
    pub tone: ToneSettings,
}

/// A validated DTMF sequence.  Keeping this typed prevents accidental CAT
/// command injection and gives every driver the same accepted digit set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DtmfSequence(String);

impl DtmfSequence {
    pub fn new(value: impl Into<String>) -> anyhow::Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 32 {
            anyhow::bail!("DTMF sequence must contain 1 to 32 digits")
        }
        if !value
            .bytes()
            .all(|digit| matches!(digit, b'0'..=b'9' | b'A'..=b'D' | b'*' | b'#'))
        {
            anyhow::bail!("DTMF sequence contains an invalid digit")
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A radio channel/memory entry.  Drivers may reject fields their model does
/// not document instead of silently dropping them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChannel {
    pub channel: u16,
    pub name: Option<String>,
    pub frequency_hz: u64,
    pub transmit_frequency_hz: Option<u64>,
    pub mode: Mode,
    pub repeater: RepeaterSettings,
}
