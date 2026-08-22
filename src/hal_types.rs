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
    RfPower,
    Preamp,
    Attenuator,
    NoiseBlanker,
    NoiseReduction,
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
