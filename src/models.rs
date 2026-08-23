//! Model catalog and support maturity.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manufacturer {
    Icom,
    Yaesu,
    Kenwood,
}

impl Manufacturer {
    pub const ALL: [Self; 3] = [Self::Icom, Self::Yaesu, Self::Kenwood];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Icom => "Icom",
            Self::Yaesu => "Yaesu",
            Self::Kenwood => "Kenwood",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    IcomCiV { default_address: u8 },
    YaesuCat,
    YaesuLegacyCat,
    KenwoodCat,
}

impl Protocol {
    pub const fn label(self) -> &'static str {
        match self {
            Self::IcomCiV { .. } => "Icom CI-V",
            Self::YaesuCat => "Yaesu CAT",
            Self::YaesuLegacyCat => "Classic Yaesu CAT",
            Self::KenwoodCat => "Kenwood PC control",
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
    /// Root-HAL operations implemented by the selected driver profile.
    pub fn driver_capabilities(self) -> crate::RadioCapabilities {
        let can_get_ptt = match self.protocol {
            Protocol::KenwoodCat => KenwoodCatModel::from_model_name(self.model)
                .map(crate::kenwood::profile::profile_for_model)
                .is_some_and(|profile| profile.supports_if_status),
            _ => true,
        };
        crate::RadioCapabilities {
            can_get_frequency: true,
            can_set_frequency: true,
            can_get_mode: true,
            can_set_mode: true,
            can_get_ptt,
            can_set_ptt: true,
            can_get_power: matches!(self.protocol, Protocol::YaesuCat | Protocol::KenwoodCat),
            can_set_power: !matches!(self.protocol, Protocol::YaesuLegacyCat),
            can_raw_protocol: true,
        }
    }

    /// Preferred starting baud derived from the selected driver profile.
    /// Operators must still match the value configured on the radio.
    pub fn preferred_baud_rate(self) -> u32 {
        match self.protocol {
            Protocol::IcomCiV { .. } => 115_200,
            Protocol::YaesuCat => YaesuCatModel::from_model_name(self.model)
                .map(crate::yaesu::profile::profile_for_model)
                .and_then(|profile| profile.baud_rates.last().copied())
                .unwrap_or(38_400),
            Protocol::YaesuLegacyCat => YaesuLegacyModel::from_model_name(self.model)
                .map(crate::yaesu::legacy_profile::profile_for_model)
                .and_then(|profile| profile.baud_rates.first().copied())
                .unwrap_or(4_800),
            Protocol::KenwoodCat => KenwoodCatModel::from_model_name(self.model)
                .map(crate::kenwood::profile::profile_for_model)
                .and_then(|profile| profile.baud_rates.last().copied())
                .unwrap_or(9_600),
        }
    }

    /// Whether the selected model's implemented driver exposes a typed HAL
    /// control. This describes Rigwright behavior, not every manual feature.
    pub fn supports_control(self, id: crate::ControlId) -> bool {
        use crate::ControlId;

        match self.protocol {
            Protocol::IcomCiV { .. } => {
                let Some(model) = IcomCivModel::from_model_name(self.model) else {
                    return false;
                };
                let profile = crate::icom::profile::profile_for_model(model);
                profile.control(id).is_some()
                    || matches!(
                        id,
                        ControlId::DataMode
                            | ControlId::Filter
                            | ControlId::RawCiV
                            | ControlId::Vfo
                    )
                    || (id == ControlId::MainSub && profile.main_sub.is_some())
                    || (id == ControlId::ExternalPreamp && profile.external_preamp.is_some())
            }
            Protocol::YaesuCat => {
                let Some(model) = YaesuCatModel::from_model_name(self.model) else {
                    return false;
                };
                let profile = crate::yaesu::profile::profile_for_model(model);
                matches!(id, ControlId::RfPower) && profile.power_range_watts.is_some()
                    || id == ControlId::Split && profile.supports_split
                    || matches!(
                        id,
                        ControlId::Agc | ControlId::NoiseReduction | ControlId::NoiseReductionLevel
                    )
            }
            Protocol::YaesuLegacyCat => id == ControlId::Split,
            Protocol::KenwoodCat => {
                let Some(model) = KenwoodCatModel::from_model_name(self.model) else {
                    return false;
                };
                let profile = crate::kenwood::profile::profile_for_model(model);
                matches!(id, ControlId::RfPower) && profile.power_range_watts.is_some()
                    || id == ControlId::Split
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcomCivModel {
    Ic705,
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
    Ft710,
    Ft991A,
    Ftdx10,
    Ftdx101D,
    Ftdx101Mp,
}

/// Classic Yaesu radios using fixed five-byte binary CAT commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YaesuLegacyModel {
    Ft817Nd,
    Ft818,
    Ft857D,
    Ft897D,
}

/// Semicolon-terminated Kenwood PC-control radios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KenwoodCatModel {
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
            Self::Ic705 => "IC-705",
            Self::Ic7300 => "IC-7300",
            Self::Ic7610 => "IC-7610",
            Self::Ic9700 => "IC-9700",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
            "IC-705" => Some(Self::Ic705),
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
            Self::Ft710 => "FT-710",
            Self::Ft991A => "FT-991A",
            Self::Ftdx10 => "FTDX10",
            Self::Ftdx101D => "FTDX101D",
            Self::Ftdx101Mp => "FTDX101MP",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
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
            Self::Ft817Nd => "FT-817ND",
            Self::Ft818 => "FT-818",
            Self::Ft857D => "FT-857D",
            Self::Ft897D => "FT-897D",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
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
            Self::Ts590Sg => "TS-590SG",
            Self::Ts890S => "TS-890S",
            Self::Ts2000 => "TS-2000",
        }
    }

    pub fn from_model_name(model: &str) -> Option<Self> {
        match model.to_ascii_uppercase().as_str() {
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
        support: SupportLevel::Framework,
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
    fn only_ic7300_claims_hardware_validation() {
        let validated: Vec<_> = POPULAR_RADIOS
            .iter()
            .filter(|profile| profile.support == SupportLevel::HardwareValidated)
            .map(|profile| profile.model)
            .collect();
        assert_eq!(validated, ["IC-7300"]);
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
            ["Icom", "Yaesu", "Kenwood"]
        );
        assert_eq!(find_model("FTDX10").unwrap().preferred_baud_rate(), 38_400);
        assert_eq!(find_model("FT-857D").unwrap().preferred_baud_rate(), 4_800);
        assert_eq!(find_model("TS-2000").unwrap().preferred_baud_rate(), 57_600);
        assert_eq!(Protocol::KenwoodCat.label(), "Kenwood PC control");
    }

    #[test]
    fn typed_control_support_matches_driver_profiles() {
        let ic7300 = *find_model("IC-7300").unwrap();
        let ic7610 = *find_model("IC-7610").unwrap();
        let ic9700 = *find_model("IC-9700").unwrap();
        let ftdx10 = *find_model("FTDX10").unwrap();
        let ft991a = *find_model("FT-991A").unwrap();
        let ft857d = *find_model("FT-857D").unwrap();
        let ts890s = *find_model("TS-890S").unwrap();
        assert!(ic7300.supports_control(crate::ControlId::AfGain));
        assert!(ic7300.supports_control(crate::ControlId::Filter));
        assert!(!ic7610.supports_control(crate::ControlId::Agc));
        assert!(ic9700.supports_control(crate::ControlId::MainSub));
        assert!(ic9700.supports_control(crate::ControlId::ExternalPreamp));
        assert!(ftdx10.supports_control(crate::ControlId::Split));
        assert!(!ft991a.supports_control(crate::ControlId::Split));
        assert!(ft857d.supports_control(crate::ControlId::Split));
        assert!(ts890s.supports_control(crate::ControlId::RfPower));
        assert!(!ts890s.supports_control(crate::ControlId::AfGain));
        assert!(!ts890s.driver_capabilities().can_get_ptt);
        assert!(
            find_model("TS-590SG")
                .unwrap()
                .driver_capabilities()
                .can_get_ptt
        );
    }
}
