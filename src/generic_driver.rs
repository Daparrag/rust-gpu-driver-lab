use crate::command::RawCommand;
use crate::device_state::{DeviceController, DeviceState, StateError};
use crate::queue_backend::SubmissionQueue;
use crate::submission::{
    SubmissionError, SubmissionRequest, ValidatedSubmission, validate_submission,
};
use crate::{BufferId, DriverError, GpuDevice};

#[derive(Debug, PartialEq, Eq)]
pub enum GenericDriverError<QE> {
    State(StateError),
    Buffer(DriverError),
    Submission(SubmissionError),
    Queue(QE),
}
impl<QE> From<StateError> for GenericDriverError<QE> {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}
impl<QE> From<DriverError> for GenericDriverError<QE> {
    fn from(error: DriverError) -> Self {
        Self::Buffer(error)
    }
}
impl<QE> From<SubmissionError> for GenericDriverError<QE> {
    fn from(error: SubmissionError) -> Self {
        Self::Submission(error)
    }
}

#[derive(Debug)]
pub struct GenericGpuDriver<Q>
where
    Q: SubmissionQueue,
{
    controller: DeviceController,
    device: GpuDevice,
    queue: Q,
}
impl<Q> GenericGpuDriver<Q>
where
    Q: SubmissionQueue,
{
    pub fn new(queue: Q) -> Self {
        Self {
            controller: DeviceController::default(),
            device: GpuDevice::default(),
            queue,
        }
    }
    pub fn state(&self) -> &DeviceState {
        self.controller.state()
    }
    pub fn load_firmware(&mut self, image: &[u8]) -> Result<(), GenericDriverError<Q::Error>> {
        self.controller.load_firmware(image)?;
        Ok(())
    }
    pub fn start(&mut self) -> Result<(), GenericDriverError<Q::Error>> {
        self.controller.start()?;
        Ok(())
    }
    pub fn allocate_buffer(
        &mut self,
        capacity: usize,
    ) -> Result<BufferId, GenericDriverError<Q::Error>> {
        self.controller.ensure_ready()?;
        let buff_id = self.device.allocate_buffer(capacity)?;
        Ok(buff_id)
    }
    pub fn write_buffer(
        &mut self,
        id: BufferId,
        data: &[u8],
    ) -> Result<(), GenericDriverError<Q::Error>> {
        self.controller.ensure_ready()?;
        self.device.write_buffer(id, data)?;
        Ok(())
    }
    pub fn submit(
        &mut self,
        buffer_id: BufferId,
        command: RawCommand,
    ) -> Result<(), GenericDriverError<Q::Error>> {
        self.controller.ensure_ready()?;
        let submission =
            validate_submission(&self.device, SubmissionRequest { buffer_id, command })?;
        self.queue
            .enqueue(submission)
            .map_err(GenericDriverError::Queue)?;
        Ok(())
    }
    pub fn next_submission(&mut self) -> Option<ValidatedSubmission> {
        self.queue.dequeue()
    }
    pub fn peek_submission(&self) -> Option<&ValidatedSubmission> {
        self.queue.peek()
    }
    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }
    pub fn is_queue_empty(&self) -> bool {
        self.queue.is_empty()
    }
    pub fn is_queue_full(&self) -> bool {
        self.queue.is_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandError, RawCommand};
    use crate::queue::{CommandQueue, QueueError};
    use crate::static_queue::{StaticCommandQueue, StaticQueueError};
    use crate::submission::SubmissionError;
    fn firmware() -> [u8; 6] {
        *b"RGPU\x01\x01"
    }
    fn valid_command(id: u64) -> RawCommand {
        RawCommand {
            id,
            offset: 0,
            length: 4,
            priority: 1,
        }
    }
    fn start_driver<Q>(queue: Q) -> GenericGpuDriver<Q>
    where
        Q: SubmissionQueue,
    {
        let mut driver = GenericGpuDriver::new(queue);
        driver.load_firmware(&firmware()).unwrap();
        driver.start().unwrap();
        driver
    }
    fn process_next_id<Q>(driver: &mut GenericGpuDriver<Q>) -> Option<u64>
    where
        Q: SubmissionQueue,
    {
        driver
            .next_submission()
            .map(|submission| submission.command().id().get())
    }
    #[test]
    fn dynamic_queue_backend_processes_submission() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(10)).unwrap();
        assert_eq!(process_next_id(&mut driver), Some(10));
    }
    #[test]
    fn static_queue_backend_processes_submission() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(20)).unwrap();
        assert_eq!(process_next_id(&mut driver), Some(20));
    }
    #[test]
    fn dynamic_queue_error_is_preserved() {
        let queue = CommandQueue::new(1).unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(1)).unwrap();
        assert_eq!(
            driver.submit(buffer_id, valid_command(2),),
            Err(GenericDriverError::Queue(QueueError::QueueFull {
                capacity: 1,
            }))
        );
    }
    #[test]
    fn static_queue_error_is_preserved() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(7)).unwrap();
        assert_eq!(
            driver.submit(buffer_id, valid_command(7),),
            Err(GenericDriverError::Queue(
                StaticQueueError::DuplicateCommandId { id: 7 }
            ))
        );
        assert_eq!(driver.queued_len(), 1);
    }
    #[test]
    fn dynamic_invalid_command_rejected() {
        let queue = CommandQueue::new(1).unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        assert_eq!(
            driver.submit(buffer_id, valid_command(0),),
            Err(GenericDriverError::Submission(SubmissionError::Command(
                CommandError::ZeroCommandId
            )))
        );
        assert_eq!(driver.queued_len(), 0);
    }
    #[test]
    fn static_invalid_command_rejected() {
        let queue = StaticCommandQueue::<1>::new().unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        assert_eq!(
            driver.submit(buffer_id, valid_command(0),),
            Err(GenericDriverError::Submission(SubmissionError::Command(
                CommandError::ZeroCommandId
            )))
        );
        assert_eq!(driver.queued_len(), 0);
    }
    #[test]
    fn dynamic_peek_submission() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(10)).unwrap();
        driver.submit(buffer_id, valid_command(20)).unwrap();
        let first = driver.peek_submission().unwrap();
        assert_eq!(first.command().id().get(), 10);
        assert_eq!(driver.queued_len(), 2);
    }
    #[test]
    fn static_peek_submission() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = start_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(10)).unwrap();
        driver.submit(buffer_id, valid_command(20)).unwrap();
        let first = driver.peek_submission().unwrap();
        assert_eq!(first.command().id().get(), 10);
        assert_eq!(driver.queued_len(), 2);
    }
    #[test]
    fn dynamic_readiness() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = GenericGpuDriver::new(queue);
        assert!(matches!(
            driver.allocate_buffer(8),
            Err(GenericDriverError::State(_))
        ));
    }
    #[test]
    fn static_readiness() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = GenericGpuDriver::new(queue);
        assert!(matches!(
            driver.allocate_buffer(8),
            Err(GenericDriverError::State(_))
        ));
    }
}
