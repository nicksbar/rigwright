//! Shared profile data for the FTDX101D and FTDX101MP family.

use crate::hal_types::MeterId;

pub const METERS: &[MeterId] = &[
    MeterId::Signal,
    MeterId::Power,
    MeterId::Swr,
    MeterId::Alc,
    MeterId::Compression,
    MeterId::Current,
    MeterId::Voltage,
    MeterId::Temperature,
];
