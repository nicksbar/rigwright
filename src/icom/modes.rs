//! Model-aware Icom CI-V mode values.

use crate::hal::BaseMode;
use crate::models::IcomCivModel;

pub fn supports_mode(model: IcomCivModel, mode: BaseMode) -> bool {
    match model {
        IcomCivModel::Ic9700 => matches!(
            mode,
            BaseMode::Lsb
                | BaseMode::Usb
                | BaseMode::Am
                | BaseMode::Cw
                | BaseMode::CwR
                | BaseMode::Fm
                | BaseMode::Rtty
                | BaseMode::RttyR
        ),
        IcomCivModel::Ic705 => matches!(
            mode,
            BaseMode::Lsb
                | BaseMode::Usb
                | BaseMode::Am
                | BaseMode::Cw
                | BaseMode::CwR
                | BaseMode::Fm
                | BaseMode::Rtty
                | BaseMode::RttyR
                | BaseMode::Wfm
        ),
        IcomCivModel::Ic7300 | IcomCivModel::Ic7610 => matches!(
            mode,
            BaseMode::Lsb
                | BaseMode::Usb
                | BaseMode::Am
                | BaseMode::Cw
                | BaseMode::CwR
                | BaseMode::Fm
                | BaseMode::Rtty
                | BaseMode::RttyR
        ),
    }
}
