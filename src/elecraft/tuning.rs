//! Elecraft VFO movement command variants.

use super::profile::{ElecraftProfile, ElecraftVfoMovementStrategy};
use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfoDirection {
    Up,
    Down,
}

/// Build the documented VFO movement command. Legacy K3-family radios take
/// a step-table index; K2 and K4 use their current front-panel step size.
pub(crate) fn command(
    profile: ElecraftProfile,
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
    match profile.vfo_movement_strategy {
        ElecraftVfoMovementStrategy::StepIndexed { maximum } => {
            let index = step_index.unwrap_or(1);
            anyhow::ensure!(
                index <= maximum,
                "Elecraft VFO step index exceeds the profiled range"
            );
            Ok(format!("{prefix}{index}"))
        }
        ElecraftVfoMovementStrategy::CurrentStep => {
            if step_index.is_some() {
                bail!("Elecraft VFO movement does not accept a CAT step index");
            }
            Ok(prefix.to_string())
        }
        ElecraftVfoMovementStrategy::Unsupported => {
            bail!("Elecraft VFO movement is not profiled")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elecraft::{k2, k3, kh1};

    #[test]
    fn command_variants_match_each_documented_family() {
        assert_eq!(
            command(k2::PROFILE, 0, VfoDirection::Up, None).unwrap(),
            "UP"
        );
        assert_eq!(
            command(k3::PROFILE, 1, VfoDirection::Down, Some(5)).unwrap(),
            "DNB5"
        );
        assert_eq!(
            command(crate::elecraft::k4::PROFILE, 1, VfoDirection::Up, None).unwrap(),
            "UPB"
        );
        assert!(command(kh1::PROFILE, 0, VfoDirection::Up, None).is_err());
    }

    #[test]
    fn command_rejects_invalid_vfo_and_step_requests() {
        assert!(command(k3::PROFILE, 2, VfoDirection::Up, Some(1)).is_err());
        assert!(command(k3::PROFILE, 0, VfoDirection::Up, Some(10)).is_err());
        assert!(command(crate::elecraft::k4::PROFILE, 0, VfoDirection::Up, Some(1)).is_err());
    }
}
