//! Model catalog and support maturity.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manufacturer {
    Icom,
    Yaesu,
    Kenwood,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    IcomCiV { default_address: u8 },
    YaesuCat,
    KenwoodCat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportLevel {
    /// Used regularly against physical hardware.
    HardwareValidated,
    /// Protocol and model profile exist, but hardware validation is pending.
    Framework,
}

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

const ALL_MODE_SCOPE: ModelCapabilities = ModelCapabilities {
    vhf_uhf: true,
    ..HF_SCOPE
};

pub const POPULAR_RADIOS: &[RadioModelProfile] = &[
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
        model: "FT-710",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FTDX10",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FTDX101D",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FTDX101MP",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Yaesu,
        model: "FT-991A",
        protocol: Protocol::YaesuCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_SCOPE,
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
        capabilities: HF_SCOPE,
    },
    RadioModelProfile {
        manufacturer: Manufacturer::Kenwood,
        model: "TS-2000",
        protocol: Protocol::KenwoodCat,
        support: SupportLevel::Framework,
        capabilities: ALL_MODE_SCOPE,
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
}
