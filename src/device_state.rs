#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub major: u8,
    pub minor: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Offline,
    FirmwareLoaded,
    Ready,
    Faulted,
}

#[derive(Debug, PartialEq, Eq)]
pub enum DeviceState {
    Offline,
    FirmwareLoaded(FirmwareInfo),
    Ready(FirmwareInfo),
    Faulted,
}

impl DeviceState {
    pub fn kind(&self) -> StateKind {
        match self {
            DeviceState::Offline => StateKind::Offline,
            DeviceState::FirmwareLoaded(_) => StateKind::FirmwareLoaded,
            DeviceState::Ready(_) => StateKind::Ready,
            DeviceState::Faulted => StateKind::Faulted,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StateError {
    InvalidState {
        operation: &'static str,
        actual: StateKind,
    },
    FirmwareTooShort {
        actual: usize,
    },
    InvalidFirmwareMagic,
    UnsupportedFirmwareMajor {
        found: u8,
    },
}

#[derive(Debug)]
pub struct DeviceController {
    state: DeviceState,
}

impl Default for DeviceController {
    fn default() -> Self {
        Self {
            state: DeviceState::Offline,
        }
    }
}

impl DeviceController {
    /// check_image size
    fn check_image(image: &[u8]) -> Result<(), StateError> {
        if image.len() < 6 {
            return Err(StateError::FirmwareTooShort {
                actual: image.len(),
            });
        }
        //validate fields
        if image[0..4] != *b"RGPU" {
            return Err(StateError::InvalidFirmwareMagic);
        }

        if image[4] != 1 {
            return Err(StateError::UnsupportedFirmwareMajor { found: image[4] });
        }
        Ok(())
    }
    /// Return device controller state
    pub fn state(&self) -> &DeviceState {
        &self.state
    }

    /// Expected image format:
    ///
    /// bytes 0..4: b"RGPU"
    /// byte 4: major version
    /// byte 5: minor version
    ///
    /// Only firmware major version 1 is supported.
    pub fn load_firmware(&mut self, image: &[u8]) -> Result<(), StateError> {
        let actual = self.state.kind();

        if actual != StateKind::Offline {
            return Err(StateError::InvalidState {
                operation: "load_firmware",
                actual,
            });
        }

        Self::check_image(image)?;

        self.state = DeviceState::FirmwareLoaded(FirmwareInfo {
            major: image[4],
            minor: image[5],
        });

        Ok(())
        // check current state
        //Self::check_image(image)?;
        //let prevst = std::mem::replace(&mut self.state, DeviceState::Offline);
        //match prevst {
        //    DeviceState::Offline => {
        //        self.state = DeviceState::FirmwareLoaded({
        //            FirmwareInfo {
        //                major: image[4],
        //                minor: image[5],
        //           }
        //        });
        //        Ok(())
        //    }
        //    other => {
        //        let actual = other.kind();
        //        self.state = other;
        //       Err(StateError::InvalidState {
        //            operation: "load_firmware",
        //            actual,
        //        })
        //    }
        // }
    }

    /// Move the firmware information from FirmwareLoaded into Ready.
    ///
    /// Do not clone FirmwareInfo.
    pub fn start(&mut self) -> Result<(), StateError> {
        let prevst = std::mem::replace(&mut self.state, DeviceState::Offline);
        match prevst {
            DeviceState::FirmwareLoaded(info) => {
                self.state = DeviceState::Ready(info);
                Ok(())
            }
            other => {
                let actual = other.kind();
                self.state = other;

                Err(StateError::InvalidState {
                    operation: "start",
                    actual,
                })
            }
        }
    }

    /// Return borrowed firmware information only when the device is ready.
    pub fn ensure_ready(&self) -> Result<&FirmwareInfo, StateError> {
        match self.state() {
            DeviceState::Ready(info) => Ok(info),
            other => Err(StateError::InvalidState {
                operation: "ensure_ready",
                actual: other.kind(),
            }),
        }
    }

    pub fn reset(&mut self) {
        self.state = DeviceState::Offline;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_firmware() -> [u8; 6] {
        *b"RGPU\x01\x07"
    }

    #[test]
    fn valid_firmware_reaches_ready_state() {
        let mut controller = DeviceController::default();

        controller.load_firmware(&valid_firmware()).unwrap();
        controller.start().unwrap();

        assert_eq!(
            controller.state(),
            &DeviceState::Ready(FirmwareInfo { major: 1, minor: 7 })
        );
    }

    #[test]
    fn starting_offline_device_is_rejected() {
        let mut controller = DeviceController::default();

        let result = controller.start();

        assert_eq!(
            result,
            Err(StateError::InvalidState {
                operation: "start",
                actual: StateKind::Offline,
            })
        );

        assert_eq!(controller.state(), &DeviceState::Offline);
    }

    #[test]
    fn invalid_magic_preserves_offline_state() {
        let mut controller = DeviceController::default();
        let image = *b"FAIL\x01\x00";

        assert_eq!(
            controller.load_firmware(&image),
            Err(StateError::InvalidFirmwareMagic)
        );

        assert_eq!(controller.state(), &DeviceState::Offline);
    }

    #[test]
    fn unsupported_firmware_is_rejected() {
        let mut controller = DeviceController::default();
        let image = *b"RGPU\x02\x00";

        assert_eq!(
            controller.load_firmware(&image),
            Err(StateError::UnsupportedFirmwareMajor { found: 2 })
        );

        assert_eq!(controller.state(), &DeviceState::Offline);
    }

    #[test]
    fn loading_firmware_twice_is_rejected() {
        let mut controller = DeviceController::default();

        controller.load_firmware(&valid_firmware()).unwrap();
        let result = controller.load_firmware(&valid_firmware());

        assert_eq!(
            result,
            Err(StateError::InvalidState {
                operation: "load_firmware",
                actual: StateKind::FirmwareLoaded,
            })
        );

        assert_eq!(
            controller.state(),
            &DeviceState::FirmwareLoaded(FirmwareInfo { major: 1, minor: 7 })
        );
    }

    #[test]
    fn ensure_ready_returns_borrowed_firmware_information() {
        let mut controller = DeviceController::default();

        controller.load_firmware(&valid_firmware()).unwrap();
        controller.start().unwrap();

        let firmware = controller.ensure_ready().unwrap();

        assert_eq!(firmware.major, 1);
        assert_eq!(firmware.minor, 7);
    }

    #[test]
    fn reset_returns_device_to_offline() {
        let mut controller = DeviceController::default();

        controller.load_firmware(&valid_firmware()).unwrap();
        controller.start().unwrap();
        controller.reset();

        assert_eq!(controller.state(), &DeviceState::Offline);
    }
    #[test]
    fn ensure_ready_fails_invalid_state() {
        let mut controller = DeviceController::default();
        controller.load_firmware(&valid_firmware()).unwrap();

        let result = controller.ensure_ready();

        assert_eq!(
            result,
            Err(StateError::InvalidState {
                operation: "ensure_ready",
                actual: StateKind::FirmwareLoaded,
            })
        );
        assert_eq!(
            controller.state(),
            &DeviceState::FirmwareLoaded(FirmwareInfo { major: 1, minor: 7 })
        );
    }

    #[test]
    fn firmware_shorter_than_header_is_rejected() {
        let mut controller = DeviceController::default();
        let image = b"RGPU\x01";

        let result = controller.load_firmware(image);

        assert_eq!(result, Err(StateError::FirmwareTooShort { actual: 5 }));

        assert_eq!(controller.state(), &DeviceState::Offline);
    }

    #[test]
    fn firmware_with_payload_is_accepted() {
        let mut controller = DeviceController::default();
        let image = b"RGPU\x01\x07payload";

        controller.load_firmware(image).unwrap();

        assert_eq!(
            controller.state(),
            &DeviceState::FirmwareLoaded(FirmwareInfo { major: 1, minor: 7 })
        );
    }
}
