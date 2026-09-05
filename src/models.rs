//! Model catalog and support maturity.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manufacturer {
    Icom,
    Yaesu,
    Kenwood,
    Elecraft,
}

impl Manufacturer {
    pub const ALL: [Self; 4] = [Self::Icom, Self::Yaesu, Self::Kenwood, Self::Elecraft];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Icom => "Icom",
            Self::Yaesu => "Yaesu",
            Self::Kenwood => "Kenwood",
            Self::Elecraft => "Elecraft",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    IcomCiV { default_address: u8 },
    YaesuCat,
    YaesuLegacyCat,
    KenwoodCat,
    ElecraftCat,
}

impl Protocol {
    pub const fn label(self) -> &'static str {
        match self {
            Self::IcomCiV { .. } => "Icom CI-V",
            Self::YaesuCat => "Yaesu CAT",
            Self::YaesuLegacyCat => "Classic Yaesu CAT",
            Self::KenwoodCat => "Kenwood PC control",
            Self::ElecraftCat => "Elecraft CAT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportLevel {
    /// Used regularly against physical hardware.
    HardwareValidated,
    /// Protocol and model profile exist, but hardware validation is pending.
    Framework,
}

impl SupportLevel {
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::HardwareValidated => "hardware validated",
            Self::Framework => "experimental",
        }
    }

    pub const fn detail_label(self) -> &'static str {
        match self {
            Self::HardwareValidated => "hardware validated",
            Self::Framework => "experimental - hardware validation pending",
        }
    }
}

/// Broad hardware/category metadata for catalog filtering.
///
/// Use [`RadioModelProfile::driver_capabilities`] and
/// [`RadioModelProfile::supports_control`] for exact implemented HAL behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub hf: bool,
    pub vhf_uhf: bool,
    pub frequency: bool,
    pub mode: bool,
    pub ptt: bool,
    pub levels: bool,
    pub spectrum: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RadioModelProfile {
    pub manufacturer: Manufacturer,
    pub model: &'static str,
    pub protocol: Protocol,
    pub support: SupportLevel,
    pub capabilities: ModelCapabilities,
}

/// Catalog identifiers for protocol-only drivers. These entries intentionally
/// expose only the controls guaranteed by the generic HAL; model-specific
/// commands remain unavailable until a concrete profile is selected.
pub const GENERIC_ICOM_MODEL: &str = "CI-V (generic)";
pub const GENERIC_YAESU_MODEL: &str = "CAT (generic)";
pub const GENERIC_YAESU_CLASSIC_MODEL: &str = "classic CAT (generic)";
pub const GENERIC_KENWOOD_MODEL: &str = "PC control (generic)";

impl RadioModelProfile {
    /// Serial speeds suitable for the selected model, ordered from the
    /// conservative choice to the fastest documented choice. These are
    /// connection options for the client; the radio's own CAT setting still
    /// has to match unless the transport performs an explicit probe.
    pub fn supported_baud_rates(self) -> &'static [u32] {
        const CIV_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];
        const YAESU_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400];
        const LEGACY_YAESU_BAUD_RATES: &[u32] = &[4_800, 9_600, 38_400];
        const KENWOOD_BAUD_RATES: &[u32] = &[4_800, 9_600, 19_200, 38_400, 57_600, 115_200];

        match self.protocol {
            Protocol::IcomCiV { .. } => IcomCivModel::from_model_name(self.model)
                .map(crate::icom::profile::profile_for_model)
                .map(|profile| profile.baud_rates)
                .unwrap_or(CIV_BAUD_RATES),
            Protocol::YaesuCat => YaesuCatModel::from_model_name(self.model)
                .map(crate::yaesu::profile::profile_for_model)
                .map(|profile| profile.baud_rates)
                .unwrap_or(YAESU_BAUD_RATES),
            Protocol::YaesuLegacyCat => YaesuLegacyModel::from_model_name(self.model)
                .map(crate::yaesu::legacy_profile::profile_for_model)
                .map(|profile| profile.baud_rates)
                .unwrap_or(LEGACY_YAESU_BAUD_RATES),
            Protocol::KenwoodCat => KenwoodCatModel::from_model_name(self.model)
                .map(crate::kenwood::profile::profile_for_model)
                .map(|profile| profile.baud_rates)
                .unwrap_or(KENWOOD_BAUD_RATES),
            Protocol::ElecraftCat => {
                crate::elecraft::profile::ElecraftModel::from_model_name(self.model)
                    .map(crate::elecraft::profile::profile_for_model)
                    .map(|profile| profile.baud_rates)
                    .unwrap_or(&[])
            }
        }
    }

    /// Fastest profile-advertised serial speed, useful for a default UI
    /// selection or a future transport probe policy.
    pub fn fastest_supported_baud_rate(self) -> Option<u32> {
        self.supported_baud_rates().iter().copied().max()
    }

    /// Root-HAL operations implemented by the selected driver profile.
    pub fn driver_capabilities(self) -> crate::RadioCapabilities {
        let (
            can_get_frequency,
            can_set_frequency,
            can_get_mode,
            can_set_mode,
            can_get_ptt,
            can_set_ptt,
        ) = match self.protocol {
            Protocol::ElecraftCat => {
                let Some(model) =
                    crate::elecraft::profile::ElecraftModel::from_model_name(self.model)
                else {
                    return crate::RadioCapabilities {
                        can_raw_protocol: true,
                        ..Default::default()
                    };
                };
                let profile = crate::elecraft::profile::profile_for_model(model);
                (
                    profile.can_get_frequency,
                    profile.can_set_frequency,
                    profile.can_get_mode,
                    profile.can_set_mode,
                    profile.can_get_ptt,
                    profile.can_set_ptt,
                )
            }
            _ => (
                true,
                true,
                true,
                true,
                match self.protocol {
                    Protocol::KenwoodCat => KenwoodCatModel::from_model_name(self.model)
                        .map(crate::kenwood::profile::profile_for_model)
                        .is_some_and(|profile| profile.supports_if_status),
                    _ => true,
                },
                true,
            ),
        };
        crate::RadioCapabilities {
            can_get_frequency,
            can_set_frequency,
            can_get_mode,
            can_set_mode,
            can_get_ptt,
            can_set_ptt,
            can_get_power: matches!(self.protocol, Protocol::YaesuCat | Protocol::KenwoodCat),
            can_set_power: !matches!(self.protocol, Protocol::YaesuLegacyCat)
                && !matches!(self.protocol, Protocol::ElecraftCat),
            can_raw_protocol: true,
        }
    }

    /// Preferred starting baud derived from the selected driver profile.
    /// Operators must still match the value configured on the radio.
    pub fn preferred_baud_rate(self) -> u32 {
        match self.protocol {
            Protocol::IcomCiV { .. } => IcomCivModel::from_model_name(self.model)
                .map(crate::icom::profile::profile_for_model)
                .map(|profile| profile.preferred_baud_rate)
                .unwrap_or(115_200),
            Protocol::YaesuCat => YaesuCatModel::from_model_name(self.model)
                .map(crate::yaesu::profile::profile_for_model)
                .map(|profile| profile.preferred_baud_rate)
                .unwrap_or(38_400),
            Protocol::YaesuLegacyCat => YaesuLegacyModel::from_model_name(self.model)
                .map(crate::yaesu::legacy_profile::profile_for_model)
                .and_then(|profile| profile.baud_rates.first().copied())
                .unwrap_or(4_800),
            Protocol::KenwoodCat => KenwoodCatModel::from_model_name(self.model)
                .map(crate::kenwood::profile::profile_for_model)
                .and_then(|profile| profile.baud_rates.last().copied())
                .unwrap_or(9_600),
            Protocol::ElecraftCat => {
                crate::elecraft::profile::ElecraftModel::from_model_name(self.model)
                    .map(crate::elecraft::profile::profile_for_model)
                    .and_then(|profile| profile.baud_rates.first().copied())
                    .unwrap_or(9_600)
            }
        }
    }

    /// Whether the selected model's implemented driver exposes a typed HAL
    /// control. This describes Rigwright behavior, not every manual feature.
    pub fn supports_control(self, id: crate::ControlId) -> bool {
        match self.protocol {
            Protocol::IcomCiV { .. } => {
                let Some(model) = IcomCivModel::from_model_name(self.model) else {
                    return false;
                };
                let profile = crate::icom::profile::profile_for_model(model);
                profile.supports_control(id)
            }
            Protocol::YaesuCat => {
                let Some(model) = YaesuCatModel::from_model_name(self.model) else {
                    return false;
                };
                crate::yaesu::profile::profile_for_model(model).supports_control(id)
            }
            Protocol::YaesuLegacyCat => YaesuLegacyModel::from_model_name(self.model)
                .map(crate::yaesu::legacy_profile::profile_for_model)
                .map(|profile| profile.supports_control(id))
                .unwrap_or(id == crate::ControlId::Split),
            Protocol::KenwoodCat => {
                let Some(model) = KenwoodCatModel::from_model_name(self.model) else {
                    return false;
                };
                let profile = crate::kenwood::profile::profile_for_model(model);
                profile.supports_control(id)
            }
            Protocol::ElecraftCat => {
                let Some(model) =
                    crate::elecraft::profile::ElecraftModel::from_model_name(self.model)
                else {
                    return false;
                };
                crate::elecraft::profile::profile_for_model(model).supports_control(id)
            }
        }
    }

    /// Model-owned discrete values for a typed numeric control.
    pub fn supported_control_values(self, id: crate::ControlId) -> Option<&'static [u8]> {
        use crate::ControlId;

        match self.protocol {
            Protocol::IcomCiV { .. } => {
                let model = IcomCivModel::from_model_name(self.model)?;
                let profile = crate::icom::profile::profile_for_model(model);
                match id {
                    ControlId::Attenuator => Some(profile.attenuator_values),
                    ControlId::Filter => Some(profile.control_capabilities.filter_values),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Maximum value for a model-owned numeric control, when the profile uses
    /// a contiguous range rather than a discrete value table.
    pub fn control_max(self, id: crate::ControlId) -> Option<u8> {
        use crate::ControlId;

        match self.protocol {
            Protocol::IcomCiV { .. } => {
                let model = IcomCivModel::from_model_name(self.model)?;
                let profile = crate::icom::profile::profile_for_model(model);
                match id {
                    ControlId::Preamp => Some(profile.preamp_max_level),
                    ControlId::Agc => Some(profile.agc_max),
                    ControlId::NoiseReductionLevel => Some(profile.noise_reduction_level_max),
                    _ => None,
                }
            }
            Protocol::YaesuCat => {
                let model = YaesuCatModel::from_model_name(self.model)?;
                crate::yaesu::profile::profile_for_model(model).control_max(id)
            }
            Protocol::KenwoodCat => {
                let profile = KenwoodCatModel::from_model_name(self.model)
                    .map(crate::kenwood::profile::profile_for_model)?;
                if let Some(spec) = profile.control(id) {
                    spec.max_value
                } else if id == ControlId::RfPower && profile.power_range_watts.is_some() {
                    Some(u8::MAX)
                } else {
                    None
                }
            }
            Protocol::ElecraftCat => {
                let profile = crate::elecraft::profile::ElecraftModel::from_model_name(self.model)
                    .map(crate::elecraft::profile::profile_for_model)?;
                match id {
                    ControlId::AfGain => profile.af_gain_max.and_then(|v| u8::try_from(v).ok()),
                    ControlId::RfGain => profile.rf_gain_max.and_then(|v| u8::try_from(v).ok()),
                    ControlId::Preamp => profile.preamp_max,
                    ControlId::Attenuator => profile.attenuator_max,
                    ControlId::NoiseBlanker => profile.noise_blanker_level_max,
                    ControlId::NoiseReductionLevel => profile.noise_reduction_level_max,
                    ControlId::Agc => profile.agc_max,
                    ControlId::Filter => profile.filter_max_hz.and_then(|v| u8::try_from(v).ok()),
                    _ => None,
                }
            }
            Protocol::YaesuLegacyCat => None,
        }
    }

    /// Convert a normalized meter reading to a documented physical value when
    /// the selected model has an authoritative calibration. `None` means the
    /// neutral HAL should remain in normalized units.
    pub fn calibrated_meter_value(self, id: crate::MeterId, raw: u8) -> Option<f32> {
        use crate::MeterId;

        if !matches!(self.protocol, Protocol::IcomCiV { .. }) || self.model != "IC-7300" {
            return None;
        }
        match id {
            MeterId::Swr => {
                let anchors = [(0_u8, 1.0_f32), (48, 1.5), (80, 2.0), (120, 3.0)];
                Some(
                    anchors
                        .windows(2)
                        .find(|window| raw <= window[1].0)
                        .map(|window| {
                            let (low_level, low_ratio) = window[0];
                            let (high_level, high_ratio) = window[1];
                            let fraction = f32::from(raw.saturating_sub(low_level))
                                / f32::from(high_level - low_level);
                            low_ratio + fraction * (high_ratio - low_ratio)
                        })
                        .unwrap_or(3.0),
                )
            }
            MeterId::Voltage => {
                let value = f32::from(raw);
                Some(if raw <= 13 {
                    (value * 10.0 / 13.0).clamp(0.0, 10.0)
                } else {
                    (10.0 + (value - 13.0) * 6.0 / (241.0 - 13.0)).min(16.0)
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcomCivModel {
    Generic,
    Ic705,
    Ic718,
    Ic7200,
    Ic7300,
    Ic7610,
    Ic9700,
}

/// Modern, semicolon-terminated Yaesu ASCII CAT models.
///
/// Older five-byte binary CAT radios deliberately do not appear here; their
/// framing and command semantics are handled by `YaesuLegacyCat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaesuCatModel {
    Generic,
    Ft710,
    Ft991A,
    Ftdx10,
    Ftdx101D,
    Ftdx101Mp,
}

/// Classic Yaesu radios using fixed five-byte binary CAT commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaesuLegacyModel {
    Generic,
    Ft817Nd,
    Ft818,
    Ft857D,
    Ft897D,
}

/// Semicolon-terminated Kenwood PC-control radios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KenwoodCatModel {
    Generic,
    Ts590Sg,
    Ts890S,
    Ts2000,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IcomScopeGeometry {
    pub divisions: usize,
    pub bins: usize,
    pub full_chunk_bins: usize,
    pub last_chunk_bins: usize,
    pub bin_max: u8,
    pub supports_main_sub_scope: bool,
}

impl IcomCivModel {
    pub fn model_name(self) -> &'static str {
        match self {
            Self::Generic => GENERIC_ICOM_MODEL,
            Self::Ic705 => "IC-705",
            Self::Ic718 => "IC-718",
            Self::Ic7200 => "IC-7200",
            Self::Ic7300 => "IC-7300",
            Self::Ic7610 => "IC-7610",
            Self::Ic9700 => "IC-9700",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "CI-V (GENERIC)" | "ICOM CI-V" => Some(Self::Generic),
            "IC-705" => Some(Self::Ic705),
            "IC-718" => Some(Self::Ic718),
            "IC-7200" => Some(Self::Ic7200),
            "IC-7300" => Some(Self::Ic7300),
            "IC-7610" => Some(Self::Ic7610),
            "IC-9700" => Some(Self::Ic9700),
            _ => None,
        }
    }
}

impl YaesuCatModel {
    pub fn model_name(self) -> &'static str {
        match self {
            Self::Generic => GENERIC_YAESU_MODEL,
            Self::Ft710 => "FT-710",
            Self::Ft991A => "FT-991A",
            Self::Ftdx10 => "FTDX10",
            Self::Ftdx101D => "FTDX101D",
            Self::Ftdx101Mp => "FTDX101MP",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "CAT (GENERIC)" | "YAESU (GENERIC)" => Some(Self::Generic),
            "FT-710" | "FT710" => Some(Self::Ft710),
            "FT-991A" | "FT991A" => Some(Self::Ft991A),
            "FTDX10" | "FT-DX10" => Some(Self::Ftdx10),
            "FTDX101D" | "FT-DX101D" => Some(Self::Ftdx101D),
            "FTDX101MP" | "FT-DX101MP" => Some(Self::Ftdx101Mp),
            _ => None,
        }
    }
}

impl YaesuLegacyModel {
    pub fn model_name(self) -> &'static str {
        match self {
            Self::Generic => GENERIC_YAESU_CLASSIC_MODEL,
            Self::Ft817Nd => "FT-817ND",
            Self::Ft818 => "FT-818",
            Self::Ft857D => "FT-857D",
            Self::Ft897D => "FT-897D",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "CLASSIC CAT (GENERIC)" | "YAESU CLASSIC (GENERIC)" => Some(Self::Generic),
            "FT-817ND" | "FT817ND" => Some(Self::Ft817Nd),
            "FT-818" | "FT818" | "FT-818ND" | "FT818ND" => Some(Self::Ft818),
            "FT-857D" | "FT857D" => Some(Self::Ft857D),
            "FT-897D" | "FT897D" | "FT-897" | "FT897" => Some(Self::Ft897D),
            _ => None,
        }
    }
}

impl KenwoodCatModel {
    pub fn model_name(self) -> &'static str {
        match self {
            Self::Generic => GENERIC_KENWOOD_MODEL,
            Self::Ts590Sg => "TS-590SG",
            Self::Ts890S => "TS-890S",
            Self::Ts2000 => "TS-2000",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "PC CONTROL (GENERIC)" | "KENWOOD (GENERIC)" => Some(Self::Generic),
            "TS-590SG" | "TS590SG" => Some(Self::Ts590Sg),
            "TS-890S" | "TS890S" => Some(Self::Ts890S),
            "TS-2000" | "TS2000" | "TS-2000X" | "TS2000X" | "TS-B2000" | "TSB2000" => {
                Some(Self::Ts2000)
            }
            _ => None,
        }
    }
}

const HF_BASE: ModelCapabilities = ModelCapabilities {
    hf: true,
    vhf_uhf: false,
    frequency: true,
    mode: true,
    ptt: true,
    levels: true,
    spectrum: false,
};

const HF_SCOPE: ModelCapabilities = ModelCapabilities {
    spectrum: true,
    ..HF_BASE
};

const ALL_MODE_BASE: ModelCapabilities = ModelCapabilities {
    vhf_uhf: true,
    ..HF_BASE
};

const ALL_MODE_SCOPE: ModelCapabilities = ModelCapabilities {
    vhf_uhf: true,
    ..HF_SCOPE
};

pub const POPULAR_RADIOS: &[RadioModelProfile] = &[
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: GENERIC_ICOM_MODEL,
        protocol: Protocol::IcomCiV {
            default_address: 0x94,
        },
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: "IC-7300",
        protocol: Protocol::IcomCiV {
            default_address: 0x94,
        },
        support: SupportLevel::HardwareValidated,
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: "IC-7200",
        protocol: Protocol::IcomCiV {
            default_address: 0x76,
        },
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: "IC-718",
        protocol: Protocol::IcomCiV {
            default_address: 0x5E,
        },
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: "IC-705",
        protocol: Protocol::IcomCiV {
            default_address: 0xA4,
        },
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: "IC-7610",
        protocol: Protocol::IcomCiV {
            default_address: 0x98,
        },
        support: SupportLevel::Framework,
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Icom,
        model: "IC-9700",
        protocol: Protocol::IcomCiV {
            default_address: 0xA2,
        },
        support: SupportLevel::Framework,
        capabilities: ModelCapabilities {
            hf: false,
            vhf_uhf: true,
            ..HF_SCOPE
        },
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: GENERIC_YAESU_CLASSIC_MODEL,
        protocol: Protocol::YaesuLegacyCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-817ND",
        protocol: Protocol::YaesuLegacyCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-818",
        protocol: Protocol::YaesuLegacyCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-857D",
        protocol: Protocol::YaesuLegacyCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-897D",
        protocol: Protocol::YaesuLegacyCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: GENERIC_YAESU_MODEL,
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-710",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FTDX10",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::HardwareValidated,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FTDX101D",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FTDX101MP",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-991A",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Kenwood,
        model: GENERIC_KENWOOD_MODEL,
        protocol: Protocol::KenwoodCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Kenwood,
        model: "TS-590SG",
        protocol: Protocol::KenwoodCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Kenwood,
        model: "TS-890S",
        protocol: Protocol::KenwoodCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Kenwood,
        model: "TS-2000",
        protocol: Protocol::KenwoodCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "K2",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "KX2",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "KX3",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "K3",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "K3S",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "K4",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Elecraft,
        model: "KH1",
        protocol: Protocol::ElecraftCat,
        support: SupportLevel::Framework,
        capabilities: HF_BASE,
    },
];

pub fn find_model(model: &str) -> Option<&'static RadioModelProfile> {
    POPULAR_RADIOS
        .iter()
        .find(|profile| profile.model.eq_ignore_ascii_case(model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_lookup_is_case_insensitive() {
        assert_eq!(find_model("ic-7300").unwrap().model, "IC-7300");
    }

    #[test]
    fn model_profiles_own_documented_meter_calibration() {
        let ic7300 = find_model("IC-7300").expect("IC-7300 profile");
        assert_eq!(
            ic7300.calibrated_meter_value(crate::MeterId::Swr, 80),
            Some(2.0)
        );
        assert_eq!(
            ic7300.calibrated_meter_value(crate::MeterId::Voltage, 13),
            Some(10.0)
        );
        let ft710 = find_model("FT-710").expect("FT-710 profile");
        assert_eq!(ft710.calibrated_meter_value(crate::MeterId::Swr, 80), None);
    }

    #[test]
    fn hardware_validated_models_are_explicit() {
        let validated: Vec<_> = POPULAR_RADIOS
            .iter()
            .filter(|profile| profile.support == SupportLevel::HardwareValidated)
            .map(|profile| profile.model)
            .collect();
        assert_eq!(validated, ["IC-7300", "FTDX10"]);
    }

    #[test]
    fn catalogs_both_ftdx101_variants() {
        let d = find_model("FTDX101D").expect("FTDX101D profile");
        let mp = find_model("ftdx101mp").expect("FTDX101MP profile");
        assert_eq!(d.protocol, Protocol::YaesuCat);
        assert_eq!(mp.protocol, Protocol::YaesuCat);
        assert_eq!(d.support, SupportLevel::Framework);
        assert_eq!(mp.support, SupportLevel::Framework);
    }

    #[test]
    fn catalogs_legacy_yaesu_binary_cat_family() {
        for model in ["FT-817ND", "FT-818", "FT-857D", "FT-897D"] {
            let profile = find_model(model).expect("legacy Yaesu profile");
            assert_eq!(profile.protocol, Protocol::YaesuLegacyCat);
            assert_eq!(profile.support, SupportLevel::Framework);
            assert!(!profile.capabilities.spectrum);
        }
    }

    #[test]
    fn icom_catalog_addresses_match_driver_profiles() {
        for catalog in POPULAR_RADIOS.iter().filter(|profile| {
            profile.manufacturer == Manufacturer::Icom && profile.model != GENERIC_ICOM_MODEL
        }) {
            let model = IcomCivModel::from_model_name(catalog.model).expect("known Icom model");
            let Protocol::IcomCiV { default_address } = catalog.protocol else {
                panic!("Icom model must use CI-V")
            };
            assert_eq!(
                default_address,
                crate::icom::profile::profile_for_model(model).default_address
            );
        }
    }

    #[test]
    fn modern_yaesu_catalog_models_have_driver_profiles() {
        for catalog in POPULAR_RADIOS.iter().filter(|profile| {
            profile.protocol == Protocol::YaesuCat && profile.model != GENERIC_YAESU_MODEL
        }) {
            let model =
                YaesuCatModel::from_model_name(catalog.model).expect("known modern Yaesu model");
            assert_eq!(
                catalog.model,
                crate::yaesu::profile::profile_for_model(model)
                    .model
                    .model_name()
            );
        }
    }

    #[test]
    fn classic_yaesu_catalog_models_have_driver_profiles() {
        for catalog in POPULAR_RADIOS.iter().filter(|profile| {
            profile.protocol == Protocol::YaesuLegacyCat
                && profile.model != GENERIC_YAESU_CLASSIC_MODEL
        }) {
            let model = YaesuLegacyModel::from_model_name(catalog.model)
                .expect("known classic Yaesu model");
            assert_eq!(
                catalog.model,
                crate::yaesu::legacy_profile::profile_for_model(model)
                    .model
                    .model_name()
            );
        }
    }

    #[test]
    fn kenwood_catalog_models_have_driver_profiles() {
        for catalog in POPULAR_RADIOS.iter().filter(|profile| {
            profile.protocol == Protocol::KenwoodCat && profile.model != GENERIC_KENWOOD_MODEL
        }) {
            let model =
                KenwoodCatModel::from_model_name(catalog.model).expect("known Kenwood model");
            assert_eq!(
                catalog.model,
                crate::kenwood::profile::profile_for_model(model)
                    .model
                    .model_name()
            );
        }
    }

    #[test]
    fn spectrum_capability_means_a_waveform_transport_is_implemented() {
        let spectrum_models: Vec<_> = POPULAR_RADIOS
            .iter()
            .filter(|profile| profile.capabilities.spectrum)
            .map(|profile| profile.model)
            .collect();
        assert_eq!(spectrum_models, ["IC-7300", "IC-705", "IC-7610", "IC-9700"]);
    }

    #[test]
    fn public_labels_and_defaults_come_from_catalog_metadata() {
        assert_eq!(
            Manufacturer::ALL.map(Manufacturer::label),
            ["Icom", "Yaesu", "Kenwood", "Elecraft"]
        );
        assert_eq!(find_model("FTDX10").unwrap().preferred_baud_rate(), 38_400);
        assert_eq!(find_model("FT-857D").unwrap().preferred_baud_rate(), 4_800);
        assert_eq!(find_model("TS-2000").unwrap().preferred_baud_rate(), 57_600);
        assert_eq!(Protocol::KenwoodCat.label(), "Kenwood PC control");
    }

    #[test]
    fn catalog_exposes_profile_baud_choices_and_fastest_option() {
        let ic7200 = *find_model("IC-7200").unwrap();
        let k3 = *find_model("K3").unwrap();
        let k4 = *find_model("K4").unwrap();
        assert_eq!(k3.supported_baud_rates(), &[4_800, 9_600, 19_200, 38_400]);
        assert_eq!(k3.fastest_supported_baud_rate(), Some(38_400));
        assert_eq!(k4.fastest_supported_baud_rate(), Some(115_200));
        assert!(k4.supported_baud_rates().starts_with(&[4_800, 9_600]));
        assert_eq!(
            ic7200.supported_baud_rates(),
            &[300, 1_200, 4_800, 9_600, 19_200]
        );
        assert_eq!(ic7200.preferred_baud_rate(), 19_200);
    }

    #[test]
    fn elecraft_catalog_preserves_limited_kh1_readability() {
        let kh1 = find_model("KH1").expect("KH1 profile");
        let capabilities = kh1.driver_capabilities();
        assert!(!capabilities.can_get_frequency);
        assert!(capabilities.can_set_frequency);
        assert!(!capabilities.can_get_mode);
        assert!(capabilities.can_set_mode);
        assert!(!capabilities.can_get_ptt);
        assert!(!capabilities.can_set_ptt);
    }

    #[test]
    fn typed_control_support_matches_driver_profiles() {
        let ic7300 = *find_model("IC-7300").unwrap();
        let ic7200 = *find_model("IC-7200").unwrap();
        let ic7610 = *find_model("IC-7610").unwrap();
        let ic9700 = *find_model("IC-9700").unwrap();
        let ftdx10 = *find_model("FTDX10").unwrap();
        let ft991a = *find_model("FT-991A").unwrap();
        let ft857d = *find_model("FT-857D").unwrap();
        let ts890s = *find_model("TS-890S").unwrap();
        assert!(ic7300.supports_control(crate::ControlId::AfGain));
        assert!(ic7300.supports_control(crate::ControlId::Filter));
        assert_eq!(ic7200.control_max(crate::ControlId::Agc), Some(2));
        assert!(ic7200.supports_control(crate::ControlId::TuningStep));
        assert!(!ic7610.supports_control(crate::ControlId::Agc));
        assert!(ic9700.supports_control(crate::ControlId::MainSub));
        assert!(ic9700.supports_control(crate::ControlId::ExternalPreamp));
        assert!(ftdx10.supports_control(crate::ControlId::Split));
        for control in [
            crate::ControlId::AfGain,
            crate::ControlId::RfGain,
            crate::ControlId::Squelch,
            crate::ControlId::Preamp,
            crate::ControlId::Attenuator,
            crate::ControlId::NoiseBlanker,
            crate::ControlId::Notch,
            crate::ControlId::ManualNotch,
            crate::ControlId::Filter,
            crate::ControlId::Rit,
            crate::ControlId::Xit,
            crate::ControlId::Tuner,
            crate::ControlId::Vfo,
        ] {
            assert!(
                ftdx10.supports_control(control),
                "missing Yaesu control {control:?}"
            );
        }
        assert!(ft991a.supports_control(crate::ControlId::Split));
        assert!(ft857d.supports_control(crate::ControlId::Split));
        assert!(ts890s.supports_control(crate::ControlId::RfPower));
        assert!(ts890s.supports_control(crate::ControlId::AfGain));
        assert!(!ts890s.driver_capabilities().can_get_ptt);
        assert!(
            find_model("TS-590SG")
                .unwrap()
                .driver_capabilities()
                .can_get_ptt
        );
    }

    #[test]
    fn control_bounds_are_exposed_by_the_selected_vendor_profile() {
        let ic7300 = *find_model("IC-7300").unwrap();
        let ftdx10 = *find_model("FTDX10").unwrap();
        let ts890s = *find_model("TS-890S").unwrap();
        let k4 = *find_model("K4").unwrap();

        assert_eq!(ic7300.control_max(crate::ControlId::Preamp), Some(1));
        assert_eq!(ic7300.control_max(crate::ControlId::Agc), Some(3));
        assert_eq!(ftdx10.control_max(crate::ControlId::Agc), Some(4));
        assert_eq!(
            ftdx10.control_max(crate::ControlId::NoiseReductionLevel),
            Some(15)
        );
        assert_eq!(ts890s.control_max(crate::ControlId::Agc), Some(3));
        assert_eq!(k4.control_max(crate::ControlId::Agc), Some(3));
    }
}
