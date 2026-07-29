use crate::command::{CommandError, RawCommand, ValidatedCommand};
use crate::{BufferId, DriverError, GpuDevice};

/// Minimal capability needed by submission validation.
///
/// Implementations may obtain this information from a real device,
/// a simulated device, or a test double.
pub trait BufferInfo {
    fn initialized_len(&self, id: BufferId) -> Result<usize, DriverError>;
}

impl BufferInfo for GpuDevice {
    fn initialized_len(&self, id: BufferId) -> Result<usize, DriverError> {
        GpuDevice::initialized_len(self, id)
    }
}

/// Untrusted request received at the driver boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmissionRequest {
    pub buffer_id: BufferId,
    pub command: RawCommand,
}

/// Fully validated command associated with an existing buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSubmission {
    buffer_id: BufferId,
    command: ValidatedCommand,
}

impl ValidatedSubmission {
    pub fn buffer_id(&self) -> BufferId {
        self.buffer_id
    }

    pub fn command(&self) -> ValidatedCommand {
        self.command
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SubmissionError {
    Command(CommandError),
    Buffer(DriverError),
    RangeExceedsInitialized { end: usize, initialized: usize },
}

impl From<CommandError> for SubmissionError {
    fn from(error: CommandError) -> Self {
        Self::Command(error)
    }
}

impl From<DriverError> for SubmissionError {
    fn from(error: DriverError) -> Self {
        Self::Buffer(error)
    }
}

/// Validate a userspace submission.
///
/// Validation order:
///
/// 1. Validate the raw command.
/// 2. Resolve the buffer.
/// 3. Check the command range against initialized data.
/// 4. Construct the validated submission.
pub fn validate_submission<B>(
    buffers: &B,
    request: SubmissionRequest,
) -> Result<ValidatedSubmission, SubmissionError>
where
    B: BufferInfo,
{
    // try to build validated cmd from SubmissionRequest
    let cmd = ValidatedCommand::try_from(request.command)?;
    // lets validate buffer and requested size
    let initialized = buffers.initialized_len(request.buffer_id)?;
    // lets validate range and used
    let end = cmd.range().end();
    if end > initialized {
        return Err(SubmissionError::RangeExceedsInitialized { end, initialized });
    }
    Ok(ValidatedSubmission {
        buffer_id: request.buffer_id,
        command: cmd,
    })
}

#[cfg(test)]
struct FixedBufferInfo {
    initialized: usize,
}

#[cfg(test)]
impl BufferInfo for FixedBufferInfo {
    fn initialized_len(&self, _id: BufferId) -> Result<usize, DriverError> {
        Ok(self.initialized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandError, RawCommand};
    use crate::{DriverError, GpuDevice};

    fn valid_command() -> RawCommand {
        RawCommand {
            id: 1,
            offset: 4,
            length: 4,
            priority: 1,
        }
    }

    #[test]
    fn valid_submission_is_accepted() {
        let mut device = GpuDevice::default();
        let buffer_id = device.allocate_buffer(16).unwrap();

        device
            .write_buffer(buffer_id, &[1, 2, 3, 4, 5, 6, 7, 8])
            .unwrap();

        let request = SubmissionRequest {
            buffer_id,
            command: valid_command(),
        };

        let submission = validate_submission(&device, request).unwrap();

        assert_eq!(submission.buffer_id(), buffer_id);

        assert_eq!(submission.command().range().end(), 8);
    }

    #[test]
    fn command_ending_at_initialized_boundary_is_valid() {
        let mut device = GpuDevice::default();
        let buffer_id = device.allocate_buffer(16).unwrap();

        device.write_buffer(buffer_id, &[0; 8]).unwrap();

        let request = SubmissionRequest {
            buffer_id,
            command: valid_command(),
        };

        assert!(validate_submission(&device, request).is_ok());
    }

    #[test]
    fn command_beyond_initialized_data_is_rejected() {
        let mut device = GpuDevice::default();
        let buffer_id = device.allocate_buffer(16).unwrap();

        device.write_buffer(buffer_id, &[1, 2, 3, 4, 5, 6]).unwrap();

        let request = SubmissionRequest {
            buffer_id,
            command: valid_command(),
        };

        assert_eq!(
            validate_submission(&device, request),
            Err(SubmissionError::RangeExceedsInitialized {
                end: 8,
                initialized: 6,
            })
        );
    }

    #[test]
    fn unknown_buffer_is_rejected() {
        let mut device = GpuDevice::default();
        let buffer_id = device.allocate_buffer(8).unwrap();

        device.release_buffer(buffer_id).unwrap();

        let request = SubmissionRequest {
            buffer_id,
            command: valid_command(),
        };

        assert_eq!(
            validate_submission(&device, request),
            Err(SubmissionError::Buffer(DriverError::UnknownBuffer(
                buffer_id
            )))
        );
    }

    #[test]
    fn structurally_invalid_command_is_rejected_first() {
        let mut device = GpuDevice::default();
        let buffer_id = device.allocate_buffer(8).unwrap();

        device.release_buffer(buffer_id).unwrap();

        let request = SubmissionRequest {
            buffer_id,
            command: RawCommand {
                id: 0,
                offset: 4,
                length: 4,
                priority: 1,
            },
        };

        assert_eq!(
            validate_submission(&device, request),
            Err(SubmissionError::Command(CommandError::ZeroCommandId))
        );
    }
    #[test]
    fn validation_works_with_test_double() {
        let mut device = GpuDevice::default();
        let buffer_id = device.allocate_buffer(1).unwrap();

        let buffers = FixedBufferInfo { initialized: 32 };

        let request = SubmissionRequest {
            buffer_id,
            command: RawCommand {
                id: 8,
                offset: 16,
                length: 8,
                priority: 3,
            },
        };

        let result = validate_submission(&buffers, request);

        assert!(result.is_ok());
    }
}
