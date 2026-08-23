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
    /// Icom IP Plus receiver optimization.
    IpPlus,
    /// Auto-notch enable/disable.
    Notch,
    /// Manual-notch enable/disable; position is a separate model-specific setting.
    ManualNotch,
    DataMode,
    Filter,
    Agc,
    Rit,
    Xit,
    Split,
    Tuner,
    RawCiV,
    Vfo,
    MainSub,
    ExternalPreamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeterId {
    /// Transmit SWR meter, normalized by the driver to a 0..=255 meter level.
    Swr,
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
