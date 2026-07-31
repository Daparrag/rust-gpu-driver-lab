use crate::command::RawCommand;
use crate::device_state::{DeviceController, DeviceState, StateError};
use crate::queue::{CommandQueue, QueueError};
use crate::submission::{
    SubmissionError, SubmissionRequest, ValidatedSubmission, validate_submission,
};
use crate::{BufferId, DriverError, GpuDevice};

#[derive(Debug, PartialEq, Eq)]
pub enum DriverApiError {
    State(StateError),
    Buffer(DriverError),
    Submission(SubmissionError),
    Queue(QueueError),
}

impl From<StateError> for DriverApiError {
    fn from(error: StateError) -> Self {
        DriverApiError::State(error)
    }
}

impl From<DriverError> for DriverApiError {
    fn from(error: DriverError) -> Self {
        DriverApiError::Buffer(error)
    }
}

impl From<SubmissionError> for DriverApiError {
    fn from(error: SubmissionError) -> Self {
        DriverApiError::Submission(error)
    }
}

impl From<QueueError> for DriverApiError {
    fn from(error: QueueError) -> Self {
        DriverApiError::Queue(error)
    }
}

/// Public interface coordinating device state, buffers and submissions.
#[derive(Debug)]
pub struct SimulatedGpuDriver {
    controller: DeviceController,
    device: GpuDevice,
    queue: CommandQueue,
}

impl SimulatedGpuDriver {
    pub fn new(queue_capacity: usize) -> Result<Self, DriverApiError> {
        let queue = CommandQueue::new(queue_capacity)?;
        Ok(Self {
            controller: DeviceController::default(),
            device: GpuDevice::default(),
            queue,
        })
    }

    pub fn state(&self) -> &DeviceState {
        self.controller.state()
    }

    pub fn load_firmware(&mut self, image: &[u8]) -> Result<(), DriverApiError> {
        self.controller.load_firmware(image)?;
        Ok(())
    }

    pub fn start(&mut self) -> Result<(), DriverApiError> {
        self.controller.start()?;
        Ok(())
    }

    /// Allocate a buffer only while the device is ready.
    pub fn allocate_buffer(&mut self, capacity: usize) -> Result<BufferId, DriverApiError> {
        // check device is ready or return
        self.controller.ensure_ready()?;
        // allocate buffer
        let id = self.device.allocate_buffer(capacity)?;
        Ok(id)
    }

    /// Write to a buffer only while the device is ready.
    pub fn write_buffer(&mut self, id: BufferId, data: &[u8]) -> Result<(), DriverApiError> {
        // check if device is ready
        self.controller.ensure_ready()?;
        //write buffer
        self.device.write_buffer(id, data)?;
        Ok(())
    }

    /// Validate and enqueue an untrusted command request.
    pub fn submit(
        &mut self,
        buffer_id: BufferId,
        command: RawCommand,
    ) -> Result<(), DriverApiError> {
        // check controller is ready
        self.controller.ensure_ready()?;
        //Validate Request
        let submission =
            validate_submission(&self.device, SubmissionRequest { buffer_id, command })?;
        self.queue.enqueue(submission)?;
        Ok(())
    }

    /// Transfer ownership of the oldest submission to the caller.
    pub fn next_submission(&mut self) -> Option<ValidatedSubmission> {
        self.queue.dequeue()
    }

    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_queue_full(&self) -> bool {
        self.queue.is_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::command::{CommandError, RawCommand};
    use crate::device_state::{FirmwareInfo, StateKind};
    use crate::queue::QueueError;
    use crate::submission::SubmissionError;

    fn firmware() -> [u8; 6] {
        *b"RGPU\x01\x05"
    }

    fn valid_command(id: u64) -> RawCommand {
        RawCommand {
            id,
            offset: 0,
            length: 4,
            priority: 1,
        }
    }

    fn ready_driver(queue_capacity: usize) -> SimulatedGpuDriver {
        let mut driver = SimulatedGpuDriver::new(queue_capacity).unwrap();

        driver.load_firmware(&firmware()).unwrap();

        driver.start().unwrap();

        driver
    }

    #[test]
    fn zero_queue_capacity_is_rejected() {
        assert!(matches!(
            SimulatedGpuDriver::new(0),
            Err(DriverApiError::Queue(QueueError::ZeroCapacity))
        ));
    }

    #[test]
    fn new_driver_is_offline() {
        let driver = SimulatedGpuDriver::new(2).unwrap();

        assert_eq!(driver.state(), &DeviceState::Offline);
    }

    #[test]
    fn firmware_and_start_are_delegated() {
        let mut driver = SimulatedGpuDriver::new(2).unwrap();

        driver.load_firmware(&firmware()).unwrap();

        assert_eq!(
            driver.state(),
            &DeviceState::FirmwareLoaded(FirmwareInfo { major: 1, minor: 5 })
        );

        driver.start().unwrap();

        assert_eq!(
            driver.state(),
            &DeviceState::Ready(FirmwareInfo { major: 1, minor: 5 })
        );
    }

    #[test]
    fn allocation_before_ready_is_rejected() {
        let mut driver = SimulatedGpuDriver::new(2).unwrap();

        assert_eq!(
            driver.allocate_buffer(16),
            Err(DriverApiError::State(StateError::InvalidState {
                operation: "ensure_ready",
                actual: StateKind::Offline,
            }))
        );
    }

    #[test]
    fn ready_driver_processes_submission_end_to_end() {
        let mut driver = ready_driver(2);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        driver.write_buffer(buffer_id, &[1, 2, 3, 4]).unwrap();

        driver.submit(buffer_id, valid_command(10)).unwrap();

        assert_eq!(driver.queued_len(), 1);

        let submission = driver.next_submission().unwrap();

        assert_eq!(submission.command().id().get(), 10);

        assert_eq!(driver.queued_len(), 0);
    }

    #[test]
    fn invalid_command_does_not_modify_queue() {
        let mut driver = ready_driver(2);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        driver.write_buffer(buffer_id, &[0; 8]).unwrap();

        let result = driver.submit(
            buffer_id,
            RawCommand {
                id: 0,
                offset: 0,
                length: 4,
                priority: 1,
            },
        );

        assert_eq!(
            result,
            Err(DriverApiError::Submission(SubmissionError::Command(
                CommandError::ZeroCommandId
            )))
        );

        assert_eq!(driver.queued_len(), 0);
    }

    #[test]
    fn zero_initialized_buffer_rejects_submission() {
        let mut driver = ready_driver(2);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        let result = driver.submit(buffer_id, valid_command(1));

        assert_eq!(
            result,
            Err(DriverApiError::Submission(
                SubmissionError::RangeExceedsInitialized {
                    end: 4,
                    initialized: 0,
                }
            ))
        );
    }

    #[test]
    fn queue_full_error_preserves_first_submission() {
        let mut driver = ready_driver(1);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        driver.write_buffer(buffer_id, &[0; 8]).unwrap();

        driver.submit(buffer_id, valid_command(1)).unwrap();

        let result = driver.submit(buffer_id, valid_command(2));

        assert_eq!(
            result,
            Err(DriverApiError::Queue(QueueError::QueueFull { capacity: 1 }))
        );

        assert_eq!(driver.queued_len(), 1);

        let first = driver.next_submission().unwrap();

        assert_eq!(first.command().id().get(), 1);
    }
    #[test]
    fn writing_before_ready_is_rejected() {
        let mut driver = ready_driver(2);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        driver.controller.reset();

        let result = driver.write_buffer(buffer_id, &[1, 2, 3, 4]);

        assert_eq!(
            result,
            Err(DriverApiError::State(StateError::InvalidState {
                operation: "ensure_ready",
                actual: StateKind::Offline,
            }))
        );

        assert_eq!(driver.device.initialized_len(buffer_id), Ok(0));
    }
}
