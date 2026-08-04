use crate::BufferId;
use crate::command::RawCommand;
use crate::generic_driver::GenericDriverError;
use crate::queue_backend::SubmissionQueue;
use crate::typed_driver::{DriverState, Operational};
use crate::typed_generic_driver::TypedGenericGpuDriver;

/// Exclusive, lifetime-bound access to the submission API.
///
/// A session cannot outlive the driver it borrows.
/// Offline drivers cannot create submission sessions.
///
/// ```compile_fail
/// use rust_gpu_driver_lab::queue::CommandQueue;
/// use rust_gpu_driver_lab::typed_driver::Offline;
/// use rust_gpu_driver_lab::
///
/// typed_generic_driver::TypedGenericGpuDriver;
///
/// let queue = CommandQueue::new(2).unwrap();
///
/// let mut driver =
///
/// TypedGenericGpuDriver::<Offline, _>::new(queue);
///
/// driver.begin_submission_session();
/// ```
#[derive(Debug)]
pub struct SubmissionSession<'a, S, Q>
where
    S: DriverState + Operational,
    Q: SubmissionQueue,
{
    driver: &'a mut TypedGenericGpuDriver<S, Q>,
}

/// The driver cannot be used directly while a session holds its
/// exclusive borrow.
///
/// ```compile_fail
/// use rust_gpu_driver_lab::queue::CommandQueue;
/// use rust_gpu_driver_lab::typed_driver::Offline;
/// use rust_gpu_driver_lab::
///
/// typed_generic_driver::TypedGenericGpuDriver;
///
/// let queue = CommandQueue::new(2).unwrap();
///
/// let offline =TypedGenericGpuDriver::<Offline, _>::new(queue);
///
/// let loaded = offline.load_firmware(b"RGPU\x01\x01").unwrap();
///
/// let mut driver = loaded.start().unwrap();
///
/// let session = driver.begin_submission_session();
///
/// let _ = driver.queued_len();
///
/// // Keep the session borrow alive after the direct access.
/// let _ = session.queued_len();
/// ```
impl<'a, S, Q> SubmissionSession<'a, S, Q>
where
    S: DriverState + Operational,
    Q: SubmissionQueue,
{
    pub(crate) fn new(driver: &'a mut TypedGenericGpuDriver<S, Q>) -> Self {
        Self { driver }
    }
    pub fn submit(
        &mut self,
        buffer_id: BufferId,
        command: RawCommand,
    ) -> Result<(), GenericDriverError<Q::Error>> {
        self.driver.submit(buffer_id, command)
    }
    pub fn queued_len(&self) -> usize {
        self.driver.queued_len()
    }
    pub fn is_queue_empty(&self) -> bool {
        self.driver.is_queue_empty()
    }
    pub fn is_queue_full(&self) -> bool {
        self.driver.is_queue_full()
    }
    pub fn state_name(&self) -> &'static str {
        self.driver.state_name()
    }
    /// Consume the session and release its exclusive borrow.
    pub fn finish(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandError, RawCommand};
    use crate::generic_driver::GenericDriverError;
    use crate::queue::{CommandQueue, QueueError};
    use crate::static_queue::{StaticCommandQueue, StaticQueueError};
    use crate::submission::SubmissionError;
    use crate::typed_driver::{Offline, Ready};
    fn firmware() -> [u8; 6] {
        *b"RGPU\x01\x04"
    }
    fn valid_command(id: u64) -> RawCommand {
        RawCommand {
            id,
            offset: 0,
            length: 4,
            priority: 1,
        }
    }
    fn ready_driver<Q>(queue: Q) -> TypedGenericGpuDriver<Ready, Q>
    where
        Q: SubmissionQueue,
    {
        TypedGenericGpuDriver::<Offline, Q>::new(queue)
            .load_firmware(&firmware())
            .unwrap()
            .start()
            .unwrap()
    }
    #[test]
    fn dynamic_session_submits_commands() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        {
            let mut session = driver.begin_submission_session();
            session.submit(buffer_id, valid_command(10)).unwrap();
            assert_eq!(session.queued_len(), 1);
            assert_eq!(session.state_name(), "ready");
        }
        assert_eq!(driver.queued_len(), 1);
    }
    #[test]
    fn static_session_submits_commands() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        let mut session = driver.begin_submission_session();
        session.submit(buffer_id, valid_command(20)).unwrap();
        assert_eq!(session.queued_len(), 1);
    }
    #[test]
    fn finish_releases_driver_borrow() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        let mut session = driver.begin_submission_session();
        session.submit(buffer_id, valid_command(1)).unwrap();
        session.finish();
        let submission = driver.next_submission().unwrap();
        assert_eq!(submission.command().id().get(), 1);
    }
    #[test]
    fn dropping_session_releases_driver_borrow() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        {
            let mut session = driver.begin_submission_session();
            session.submit(buffer_id, valid_command(2)).unwrap();
        }
        assert_eq!(driver.next_submission().unwrap().command().id().get(), 2);
    }
    #[test]
    fn failed_submission_does_not_end_session() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        let mut session = driver.begin_submission_session();
        assert_eq!(
            session.submit(buffer_id, valid_command(0),),
            Err(GenericDriverError::Submission(SubmissionError::Command(
                CommandError::ZeroCommandId
            )))
        );
        assert_eq!(session.queued_len(), 0);
        session.submit(buffer_id, valid_command(3)).unwrap();
        assert_eq!(session.queued_len(), 1);
    }
    #[test]
    fn dynamic_queue_error_remains_concrete() {
        let queue = CommandQueue::new(1).unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        let mut session = driver.begin_submission_session();
        session.submit(buffer_id, valid_command(1)).unwrap();
        assert_eq!(
            session.submit(buffer_id, valid_command(2),),
            Err(GenericDriverError::Queue(QueueError::QueueFull {
                capacity: 1,
            }))
        );
    }
    #[test]
    fn static_queue_error_remains_concrete() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        let mut session = driver.begin_submission_session();
        session.submit(buffer_id, valid_command(7)).unwrap();
        assert_eq!(
            session.submit(buffer_id, valid_command(7),),
            Err(GenericDriverError::Queue(
                StaticQueueError::DuplicateCommandId { id: 7 }
            ))
        );
    }
}
