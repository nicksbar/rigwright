//! Model-specific parsing for Elecraft `OM` option responses.

use super::profile::ElecraftModel;
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElecraftOptions {
    pub model: ElecraftModel,
    pub raw: String,
    flags: String,
}

impl ElecraftOptions {
    pub fn has_flag(&self, flag: char) -> bool {
        self.flags.contains(flag)
    }

    pub fn model_hint(&self) -> Option<ElecraftModel> {
        match self.model {
            ElecraftModel::K3 | ElecraftModel::K3s => {
                if self.has_flag('R') {
                    Some(ElecraftModel::K3s)
                } else {
                    Some(ElecraftModel::K3)
                }
            }
            ElecraftModel::Kx2 | ElecraftModel::Kx3 => {
                self.raw.chars().last().and_then(|id| match id {
                    '1' => Some(ElecraftModel::Kx2),
                    '2' => Some(ElecraftModel::Kx3),
                    _ => None,
                })
            }
            ElecraftModel::K4 => Some(ElecraftModel::K4),
            _ => None,
        }
    }
}

pub(crate) fn parse(model: ElecraftModel, response: &[u8]) -> Result<ElecraftOptions> {
    anyhow::ensure!(
        matches!(
            model,
            ElecraftModel::Kx2
                | ElecraftModel::Kx3
                | ElecraftModel::K3
                | ElecraftModel::K3s
                | ElecraftModel::K4
        ),
        "Elecraft OM option parsing is not documented for this model"
    );
    let text = std::str::from_utf8(response).context("Elecraft option response is not ASCII")?;
    let payload = text
        .strip_prefix("OM")
        .and_then(|value| value.strip_suffix(';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("unexpected Elecraft option response"))?;
    if !payload.is_ascii() {
        bail!("Elecraft option response contains non-ASCII data");
    }
    Ok(ElecraftOptions {
        model,
        raw: payload.to_string(),
        flags: payload.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_k3s_and_kx_model_hints_without_collapsing_option_flags() {
        let k3s = parse(ElecraftModel::K3, b"OM APXSDFfLVR--;").unwrap();
        assert!(k3s.has_flag('A') && k3s.has_flag('L') && k3s.has_flag('R'));
        assert_eq!(k3s.model_hint(), Some(ElecraftModel::K3s));

        let kx3 = parse(ElecraftModel::Kx2, b"OM APF---TBXI02;").unwrap();
        assert!(kx3.has_flag('A') && kx3.has_flag('F'));
        assert_eq!(kx3.model_hint(), Some(ElecraftModel::Kx3));
    }

    #[test]
    fn parses_the_fixed_position_k4_option_bitmap() {
        let k4 = parse(ElecraftModel::K4, b"OM APXSHML14---;").unwrap();
        assert_eq!(k4.raw, "APXSHML14---");
        assert!(k4.has_flag('A') && k4.has_flag('P') && k4.has_flag('4'));
        assert!(!k4.has_flag('D'));
        assert_eq!(k4.model_hint(), Some(ElecraftModel::K4));
    }
}
