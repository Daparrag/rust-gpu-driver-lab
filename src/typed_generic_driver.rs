use crate::BufferId;
use crate::command::RawCommand;
use crate::device_state::DeviceState;
use crate::generic_driver::{GenericDriverError, GenericGpuDriver};
use crate::queue_backend::SubmissionQueue;
use crate::submission::ValidatedSubmission;
use crate::typed_driver::{DriverState, FirmwareLoaded, Offline, Operational, Ready};
use std::marker::PhantomData;

/// GPU driver whose lifecycle state and queue backend are both
/// represented in its type.
///
/// Offline drivers do not expose operational methods:
///
/// ```compile_fail
/// use rust_gpu_driver_lab::static_queue::StaticCommandQueue;
/// use rust_gpu_driver_lab::typed_driver::Offline;
/// use rust_gpu_driver_lab::typed_generic_driver::TypedGenericGpuDriver;
///
/// let queue =
///
/// StaticCommandQueue::<2>::new().unwrap();
///
/// let mut driver =
///
/// TypedGenericGpuDriver::<Offline, _>::new(queue);
///
/// driver.allocate_buffer(8);
/// ```
#[derive(Debug)]
pub struct TypedGenericGpuDriver<S, Q>
where
    S: DriverState,
    Q: SubmissionQueue,
{
    inner: GenericGpuDriver<Q>,
    marker: PhantomData<S>,
}
/// Failed lifecycle transition preserving both the driver and its
/// concrete queue backend.
#[derive(Debug)]
pub struct GenericTransitionError<S, Q>
where
    S: DriverState,
    Q: SubmissionQueue,
{
    driver: Box<TypedGenericGpuDriver<S, Q>>,
    error: GenericDriverError<Q::Error>,
}

impl<S, Q> GenericTransitionError<S, Q>
where
    S: DriverState,
    Q: SubmissionQueue,
{
    pub fn error(&self) -> &GenericDriverError<Q::Error> {
        &self.error
    }
    pub fn into_driver(self) -> TypedGenericGpuDriver<S, Q> {
        *self.driver
    }
    pub fn into_parts(self) -> (TypedGenericGpuDriver<S, Q>, GenericDriverError<Q::Error>) {
        (*self.driver, self.error)
    }
}

impl<S, Q> TypedGenericGpuDriver<S, Q>
where
    S: DriverState,
    Q: SubmissionQueue,
{
    /// Return state name to the caller
    pub fn state_name(&self) -> &'static str {
        S::NAME
    }
    /// Inspect the runtime for diagnostics
    pub fn runtime_state(&self) -> &DeviceState {
        self.inner.state()
    }
    fn change_state<T>(self) -> TypedGenericGpuDriver<T, Q>
    where
        T: DriverState,
    {
        TypedGenericGpuDriver {
            inner: self.inner,
            marker: PhantomData,
        }
    }
}

impl<Q> TypedGenericGpuDriver<Offline, Q>
where
    Q: SubmissionQueue,
{
    pub fn new(queue: Q) -> Self {
        Self {
            inner: GenericGpuDriver::new(queue),
            marker: PhantomData,
        }
    }

    pub fn load_firmware(
        mut self,
        image: &[u8],
    ) -> Result<TypedGenericGpuDriver<FirmwareLoaded, Q>, GenericTransitionError<Offline, Q>> {
        match self.inner.load_firmware(image) {
            Ok(()) => Ok(self.change_state()),
            Err(error) => Err(GenericTransitionError {
                driver: Box::new(self),
                error,
            }),
        }
    }
}

impl<Q> TypedGenericGpuDriver<FirmwareLoaded, Q>
where
    Q: SubmissionQueue,
{
    pub fn start(
        mut self,
    ) -> Result<TypedGenericGpuDriver<Ready, Q>, GenericTransitionError<FirmwareLoaded, Q>> {
        match self.inner.start() {
            Ok(()) => Ok(self.change_state()),
            Err(error) => Err(GenericTransitionError {
                driver: Box::new(self),
                error,
            }),
        }
    }
}

impl<T, Q> TypedGenericGpuDriver<T, Q>
where
    T: Operational,
    Q: SubmissionQueue,
{
    pub fn allocate_buffer(
        &mut self,
        capacity: usize,
    ) -> Result<BufferId, GenericDriverError<Q::Error>> {
        self.inner.allocate_buffer(capacity)
    }

    pub fn write_buffer(
        &mut self,
        id: BufferId,
        data: &[u8],
    ) -> Result<(), GenericDriverError<Q::Error>> {
        self.inner.write_buffer(id, data)
    }

    pub fn submit(
        &mut self,
        buffer_id: BufferId,
        command: RawCommand,
    ) -> Result<(), GenericDriverError<Q::Error>> {
        self.inner.submit(buffer_id, command)
    }

    pub fn next_submission(&mut self) -> Option<ValidatedSubmission> {
        self.inner.next_submission()
    }
    pub fn peek_submission(&self) -> Option<&ValidatedSubmission> {
        self.inner.peek_submission()
    }

    pub fn queued_len(&self) -> usize {
        self.inner.queued_len()
    }
    pub fn is_queue_empty(&self) -> bool {
        self.inner.is_queue_empty()
    }
    pub fn is_queue_full(&self) -> bool {
        self.inner.is_queue_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::RawCommand;
    use crate::queue::{CommandQueue, QueueError};
    use crate::static_queue::{StaticCommandQueue, StaticQueueError};
    fn firmware() -> [u8; 6] {
        *b"RGPU\x01\x03"
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
    fn operational_length<S, Q>(driver: &TypedGenericGpuDriver<S, Q>) -> usize
    where
        S: Operational,
        Q: SubmissionQueue,
    {
        driver.queued_len()
    }
    #[test]
    fn dynamic_backend_survives_state_transitions() {
        let queue = CommandQueue::new(2).unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(10)).unwrap();
        assert_eq!(driver.next_submission().unwrap().command().id().get(), 10);
    }
    #[test]
    fn static_backend_survives_state_transitions() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(20)).unwrap();
        assert_eq!(driver.next_submission().unwrap().command().id().get(), 20);
    }
    fn assert_dynamic_ready(_driver: &TypedGenericGpuDriver<Ready, CommandQueue>) {}
    #[test]
    fn dynamic_backend_type_is_preserved() {
        let queue = CommandQueue::new(2).unwrap();
        let driver = ready_driver(queue);
        assert_dynamic_ready(&driver);
    }
    fn assert_static_ready(_driver: &TypedGenericGpuDriver<Ready, StaticCommandQueue<4>>) {}
    #[test]
    fn static_backend_type_is_preserved() {
        let queue = StaticCommandQueue::<4>::new().unwrap();
        let driver = ready_driver(queue);
        assert_static_ready(&driver);
    }
    #[test]
    fn failed_transition_returns_original_backend() {
        let queue = StaticCommandQueue::<3>::new().unwrap();
        let offline = TypedGenericGpuDriver::<Offline, _>::new(queue);
        let failure = offline.load_firmware(b"bad").unwrap_err();
        let offline = failure.into_driver();
        assert_eq!(offline.state_name(), "offline");
        assert_eq!(offline.runtime_state(), &DeviceState::Offline);
        let ready = offline.load_firmware(&firmware()).unwrap().start().unwrap();
        let _: TypedGenericGpuDriver<Ready, StaticCommandQueue<3>> = ready;
    }

    #[test]
    fn dynamic_queue_error_remains_concrete() {
        let queue = CommandQueue::new(1).unwrap();
        let mut driver = ready_driver(queue);
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
    fn static_queue_error_remains_concrete() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let mut driver = ready_driver(queue);
        let buffer_id = driver.allocate_buffer(8).unwrap();
        driver.write_buffer(buffer_id, &[0; 8]).unwrap();
        driver.submit(buffer_id, valid_command(7)).unwrap();
        assert_eq!(
            driver.submit(buffer_id, valid_command(7),),
            Err(GenericDriverError::Queue(
                StaticQueueError::DuplicateCommandId { id: 7 }
            ))
        );
    }
    #[test]
    fn ready_driver_satisfies_both_bounds() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        let driver = ready_driver(queue);
        assert_eq!(operational_length(&driver), 0);
    }
}
