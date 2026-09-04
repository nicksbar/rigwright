//! Protocol-neutral HAL value types.

/// A best-effort snapshot of a radio's core operating state, read in as few
/// protocol round trips as the backend allows. Each field is optional because
/// a degraded link may only recover part of the state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CoreState {
    pub frequency_hz: Option<u64>,
    pub mode: Option<Mode>,
    pub ptt: Option<bool>,
}

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

/// Driver-reported receiver filter bandwidth for one normalized operating
/// mode and filter selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterBandwidth {
    pub mode: Mode,
    pub filter: u8,
    pub bandwidth_hz: u32,
}

/// Documented setup required before a driver can perform a safe SWR sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwrSweepSetup {
    pub carrier_mode: Mode,
    pub rf_power: u8,
}

/// A physical presentation supplied by the driver for a normalized meter.
/// The UI formats this value but does not perform model-specific calibration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterPresentation {
    pub value: f32,
    pub unit: &'static str,
    pub precision: u8,
    pub upper_bound: Option<f32>,
}

/// Protocol-neutral native scope configuration. `None` leaves a setting
/// unchanged; drivers validate supported values against their model profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeConfiguration {
    pub span_hz: Option<u64>,
    pub fixed_edges_hz: Option<(u64, u64)>,
    pub fixed_edge_number: Option<u8>,
    pub hold: Option<bool>,
    pub reference_level_tenths_db: Option<i16>,
    pub sweep_speed: Option<u8>,
    pub center_mode: Option<bool>,
    pub vbw_wide: Option<bool>,
    pub center_type: Option<ScopeCenterType>,
    pub tx_display: Option<bool>,
    pub max_hold: Option<ScopeMaxHold>,
    pub marker_position: Option<ScopeMarkerPosition>,
    pub averaging: Option<u8>,
    pub waveform_type: Option<ScopeWaveformType>,
    pub waterfall_display: Option<bool>,
    pub waterfall_size: Option<u8>,
    pub waterfall_peak_level: Option<u8>,
    pub marker_auto_hide: Option<bool>,
    pub waveform_color_current: Option<ScopeColor>,
    pub waveform_color_line: Option<ScopeColor>,
    pub waveform_color_max_hold: Option<ScopeColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScopeColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeEdgeBank {
    pub low_hz: u64,
    pub high_hz: u64,
    pub edge_numbers: &'static [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScopeState {
    pub configuration: ScopeConfiguration,
    pub waveform_color_current: Option<ScopeColor>,
    pub waveform_color_line: Option<ScopeColor>,
    pub waveform_color_max_hold: Option<ScopeColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeCenterType {
    FilterCenter,
    CarrierPoint,
    CarrierPointAbsolute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMaxHold {
    Off,
    TenSeconds,
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMarkerPosition {
    FilterCenter,
    CarrierPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeWaveformType {
    Fill,
    FillAndLine,
}

/// Driver-owned geometry and legal values for a native spectrum scope.
/// Applications use this metadata to render controls; the driver remains the
/// authority that validates and applies `ScopeConfiguration`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeMetadata {
    pub waveform_bins: usize,
    pub waveform_divisions: u8,
    pub span_options_hz: &'static [u64],
    pub sweep_speed_values: &'static [u8],
    pub fixed_edge_numbers: &'static [u8],
    pub reference_level_range_tenths_db: Option<(i16, i16, i16)>,
    pub supports_hold: bool,
    pub supports_vbw: bool,
    pub center_type_options: &'static [ScopeCenterType],
    pub tx_display_options: &'static [bool],
    pub max_hold_options: &'static [ScopeMaxHold],
    pub marker_position_options: &'static [ScopeMarkerPosition],
    /// Averaging values use 0 for off and 2..=4 for sweep counts.
    pub averaging_options: &'static [u8],
    pub waveform_type_options: &'static [ScopeWaveformType],
    pub waterfall_display_options: &'static [bool],
    pub waterfall_size_options: &'static [u8],
    /// Waterfall peak-color threshold, expressed as the manual's grid 1..=8.
    pub waterfall_peak_level_options: &'static [u8],
    pub marker_auto_hide_options: &'static [bool],
    pub edge_banks: &'static [ScopeEdgeBank],
    pub supports_waveform_colors: bool,
}

/// Driver-owned polling guidance for a normalized meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterPollSpec {
    pub meter: MeterId,
    pub interval_ms: u64,
    pub tx_priority: bool,
}

/// Protocol-level meter facts that a client may use when it needs to
/// understand a driver's raw polling contract. The HAL value returned by
/// `get_meter` is still normalized to `0..=255`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterMetadata {
    pub meter: MeterId,
    pub raw_min: u16,
    pub raw_max: u16,
    pub raw_width: u8,
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
    MicGain,
    MonitorLevel,
    SpeechProcessor,
    SpeechProcessorLevel,
    IfShift,
    Vox,
    VoxGain,
    VoxDelay,
    BreakIn,
    Lock,
    NoiseBlankerLevel,
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
        Self::MicGain,
        Self::MonitorLevel,
        Self::SpeechProcessor,
        Self::SpeechProcessorLevel,
        Self::IfShift,
        Self::Vox,
        Self::VoxGain,
        Self::VoxDelay,
        Self::BreakIn,
        Self::Lock,
        Self::NoiseBlankerLevel,
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

/// Convert a HAL 0..=255 level to a documented vendor range using the same
/// half-up rounding policy as [`normalize_meter_level`].
pub fn denormalize_meter_level(level: u8, vendor_max: u16) -> Option<u16> {
    if vendor_max == 0 {
        return None;
    }
    Some(((u32::from(level) * u32::from(vendor_max) + 127) / 255) as u16)
}

#[cfg(test)]
mod meter_tests {
    use super::{denormalize_meter_level, normalize_meter_level};

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
        assert_eq!(denormalize_meter_level(0, 30), Some(0));
        assert_eq!(denormalize_meter_level(128, 30), Some(15));
        assert_eq!(denormalize_meter_level(255, 30), Some(30));
        assert_eq!(denormalize_meter_level(128, 0), None);
        for maximum in [1, 3, 15, 30, 70, 250, 999] {
            let tolerance = (255 / maximum) + 1;
            for level in u8::MIN..=u8::MAX {
                let native = denormalize_meter_level(level, maximum).unwrap();
                let round_trip = normalize_meter_level(native, maximum).unwrap();
                assert!(
                    u16::from(round_trip).abs_diff(u16::from(level)) <= tolerance,
                    "maximum={maximum}, level={level}, native={native}, round_trip={round_trip}, tolerance={tolerance}"
                );
            }
        }
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
