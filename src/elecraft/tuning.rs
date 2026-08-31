//! Elecraft VFO movement command variants.

use super::profile::ElecraftModel;
use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoDirection {
    Up,
    Down,
}

/// Build the documented VFO movement command. Legacy K3-family radios take
/// a step-table index; K2 and K4 use their current front-panel step size.
pub(crate) fn command(
    model: ElecraftModel,
    vfo: u8,
    direction: VfoDirection,
    step_index: Option<u8>,
) -> Result<String> {
    anyhow::ensure!(vfo <= 1, "Elecraft VFO must be A (0) or B (1)");
    let prefix = match direction {
        VfoDirection::Up if vfo == 0 => "UP",
        VfoDirection::Up => "UPB",
        VfoDirection::Down if vfo == 0 => "DN",
        VfoDirection::Down => "DNB",
    };
    match model {
        ElecraftModel::Kx2 | ElecraftModel::Kx3 | ElecraftModel::K3 | ElecraftModel::K3s => {
            let index = step_index.unwrap_or(1);
            anyhow::ensure!(index <= 9, "Elecraft VFO step index must be 0..=9");
            Ok(format!("{prefix}{index}"))
        }
        ElecraftModel::K2 | ElecraftModel::K4 => {
            if step_index.is_some() {
                bail!(
                    "{} does not accept a CAT VFO step index",
                    model.model_name()
                );
            }
            Ok(prefix.to_string())
        }
        ElecraftModel::Kh1 => bail!("Elecraft KH1 does not document UP/DN VFO movement"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_variants_match_each_documented_family() {
        assert_eq!(
            command(ElecraftModel::K2, 0, VfoDirection::Up, None).unwrap(),
            "UP"
        );
        assert_eq!(
            command(ElecraftModel::K3, 1, VfoDirection::Down, Some(5)).unwrap(),
            "DNB5"
        );
        assert_eq!(
            command(ElecraftModel::K4, 1, VfoDirection::Up, None).unwrap(),
            "UPB"
        );
        assert!(command(ElecraftModel::Kh1, 0, VfoDirection::Up, None).is_err());
    }
}
