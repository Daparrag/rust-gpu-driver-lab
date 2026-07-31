use std::marker::PhantomData;

use crate::BufferId;
use crate::command::RawCommand;
use crate::device_state::DeviceState;
use crate::driver::{DriverApiError, SimulatedGpuDriver};
use crate::submission::ValidatedSubmission;

mod sealed {
    pub trait Sealed {}
}

/// Marker implemented only by valid driver lifecycle states.
///
/// External crates cannot implement this trait because it is sealed.
///
/// ```compile_fail
/// use rust_gpu_driver_lab::typed_driver::DriverState;
///
/// #[derive(Debug)]
/// struct InventedState;
///
/// impl DriverState for InventedState {
///     const NAME: &'static str = "invented";
/// }
/// ```
pub trait DriverState: sealed::Sealed + std::fmt::Debug {
    const NAME: &'static str;
}

impl sealed::Sealed for Offline {}

impl DriverState for Offline {
    const NAME: &'static str = "offline";
}

impl sealed::Sealed for FirmwareLoaded {}

impl DriverState for FirmwareLoaded {
    const NAME: &'static str = "firmware-loaded";
}

impl sealed::Sealed for Ready {}

impl DriverState for Ready {
    const NAME: &'static str = "ready";
}

impl Operational for Ready {}

/// Capability for states that allow normal GPU operations.
pub trait Operational: DriverState {}

/// Compile-time marker for an offline driver.
#[derive(Debug)]
pub struct Offline;

/// Compile-time marker for a driver with loaded firmware.
#[derive(Debug)]
pub struct FirmwareLoaded;

/// Compile-time marker for a ready driver.
#[derive(Debug)]
pub struct Ready;

/// Driver wrapper whose lifecycle state is represented by `S`.
///
/// Offline drivers do not expose operational methods:
///
/// ```compile_fail
/// use rust_gpu_driver_lab::typed_driver::{
///     Offline,
///     TypedDriver,
/// };
///
/// let mut driver =
///     TypedDriver::<Offline>::new(2).unwrap();
///
/// driver.allocate_buffer(8);
/// ```
#[derive(Debug)]
pub struct TypedDriver<S: DriverState> {
    inner: SimulatedGpuDriver,
    marker: PhantomData<S>,
}

/// A failed transition returns both the error and the original driver.
///
/// This lets the caller inspect the error and retry without losing the
/// driver instance.
#[derive(Debug)]
pub struct TransitionError<S: DriverState> {
    driver: Box<TypedDriver<S>>,
    error: DriverApiError,
}

impl<S: DriverState> TransitionError<S> {
    pub fn error(&self) -> &DriverApiError {
        &self.error
    }

    pub fn into_driver(self) -> TypedDriver<S> {
        *self.driver
    }

    pub fn into_parts(self) -> (TypedDriver<S>, DriverApiError) {
        (*self.driver, self.error)
    }
}

impl<S: DriverState> TypedDriver<S> {
    /// Return state_name to the caller.
    pub fn state_name(&self) -> &'static str {
        S::NAME
    }

    /// Inspect the runtime state for diagnostics.
    pub fn runtime_state(&self) -> &DeviceState {
        self.inner.state()
    }

    /// Change only the compile-time marker.
    ///
    /// This must be called only after the corresponding runtime
    /// transition succeeds.
    fn change_state<T: DriverState>(self) -> TypedDriver<T> {
        TypedDriver {
            inner: self.inner,
            marker: PhantomData,
        }
    }
}

impl TypedDriver<Offline> {
    pub fn new(queue_capacity: usize) -> Result<Self, DriverApiError> {
        let driver = SimulatedGpuDriver::new(queue_capacity)?;
        Ok(Self {
            inner: driver,
            marker: PhantomData,
        })
    }

    /// Consume an offline driver and return a firmware-loaded driver.
    ///
    /// On failure, return the original offline driver.
    pub fn load_firmware(
        mut self,
        image: &[u8],
    ) -> Result<TypedDriver<FirmwareLoaded>, TransitionError<Offline>> {
        match self.inner.load_firmware(image) {
            Ok(()) => Ok(self.change_state()),
            Err(error) => Err(TransitionError {
                driver: Box::new(self),
                error,
            }),
        }
    }
}

impl TypedDriver<FirmwareLoaded> {
    /// Consume a firmware-loaded driver and return a ready driver.
    ///
    /// On failure, return the original firmware-loaded driver.
    pub fn start(mut self) -> Result<TypedDriver<Ready>, TransitionError<FirmwareLoaded>> {
        match self.inner.start() {
            Ok(()) => Ok(self.change_state()),
            Err(error) => Err(TransitionError {
                driver: Box::new(self),
                error,
            }),
        }
    }
}

impl<S> TypedDriver<S>
where
    S: Operational,
{
    pub fn allocate_buffer(&mut self, capacity: usize) -> Result<BufferId, DriverApiError> {
        self.inner.allocate_buffer(capacity)
    }

    pub fn write_buffer(&mut self, id: BufferId, data: &[u8]) -> Result<(), DriverApiError> {
        self.inner.write_buffer(id, data)
    }

    pub fn submit(
        &mut self,
        buffer_id: BufferId,
        command: RawCommand,
    ) -> Result<(), DriverApiError> {
        self.inner.submit(buffer_id, command)
    }

    pub fn next_submission(&mut self) -> Option<ValidatedSubmission> {
        self.inner.next_submission()
    }

    pub fn queued_len(&self) -> usize {
        self.inner.queued_len()
    }

    pub fn is_queue_full(&self) -> bool {
        self.inner.is_queue_full()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    use crate::command::{CommandError, RawCommand};
    use crate::device_state::{DeviceState, FirmwareInfo, StateError};
    use crate::driver::DriverApiError;
    use crate::queue::QueueError;
    use crate::submission::SubmissionError;

    fn firmware() -> [u8; 6] {
        *b"RGPU\x01\x09"
    }

    fn valid_command(id: u64) -> RawCommand {
        RawCommand {
            id,
            offset: 0,
            length: 4,
            priority: 1,
        }
    }

    fn ready_driver(queue_capacity: usize) -> TypedDriver<Ready> {
        TypedDriver::<Offline>::new(queue_capacity)
            .unwrap()
            .load_firmware(&firmware())
            .unwrap()
            .start()
            .unwrap()
    }
    fn operational_queue_length<S>(driver: &TypedDriver<S>) -> usize
    where
        S: Operational,
    {
        driver.queued_len()
    }

    #[test]
    fn new_typed_driver_is_offline() {
        let driver = TypedDriver::<Offline>::new(2).unwrap();

        assert_eq!(driver.runtime_state(), &DeviceState::Offline);
    }

    #[test]
    fn zero_queue_capacity_error_is_preserved() {
        assert!(matches!(
            TypedDriver::<Offline>::new(0),
            Err(DriverApiError::Queue(QueueError::ZeroCapacity))
        ));
    }

    #[test]
    fn successful_transitions_change_runtime_state() {
        let offline = TypedDriver::<Offline>::new(2).unwrap();

        let loaded = offline.load_firmware(&firmware()).unwrap();

        assert_eq!(
            loaded.runtime_state(),
            &DeviceState::FirmwareLoaded(FirmwareInfo { major: 1, minor: 9 })
        );

        let ready = loaded.start().unwrap();

        assert_eq!(
            ready.runtime_state(),
            &DeviceState::Ready(FirmwareInfo { major: 1, minor: 9 })
        );
    }

    #[test]
    fn failed_firmware_transition_returns_offline_driver() {
        let offline = TypedDriver::<Offline>::new(2).unwrap();

        let transition_error = offline.load_firmware(b"bad").unwrap_err();

        assert_eq!(
            transition_error.error(),
            &DriverApiError::State(StateError::FirmwareTooShort { actual: 3 })
        );

        let offline = transition_error.into_driver();

        assert_eq!(offline.runtime_state(), &DeviceState::Offline);

        // The recovered offline driver can be reused.
        let loaded = offline.load_firmware(&firmware()).unwrap();

        assert!(matches!(
            loaded.runtime_state(),
            DeviceState::FirmwareLoaded(_)
        ));
    }

    #[test]
    fn ready_driver_processes_submission() {
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
    fn invalid_command_does_not_modify_ready_driver_queue() {
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
    fn queue_full_error_preserves_existing_submission() {
        let mut driver = ready_driver(1);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        driver.write_buffer(buffer_id, &[0; 8]).unwrap();

        driver.submit(buffer_id, valid_command(1)).unwrap();

        assert_eq!(
            driver.submit(buffer_id, valid_command(2),),
            Err(DriverApiError::Queue(QueueError::QueueFull { capacity: 1 }))
        );

        assert_eq!(driver.queued_len(), 1);

        assert_eq!(driver.next_submission().unwrap().command().id().get(), 1);
    }

    #[test]
    fn state_names_follow_compile_time_markers() {
        let offline = TypedDriver::<Offline>::new(2).unwrap();

        assert_eq!(offline.state_name(), "offline");

        let loaded = offline.load_firmware(&firmware()).unwrap();

        assert_eq!(loaded.state_name(), "firmware-loaded");

        let ready = loaded.start().unwrap();

        assert_eq!(ready.state_name(), "ready");
    }

    #[test]
    fn ready_driver_satisfies_operational_capability() {
        let driver = ready_driver(2);

        assert_eq!(operational_queue_length(&driver), 0);
    }

    #[test]
    fn operational_capability_allows_buffer_work() {
        let mut driver = ready_driver(2);

        let buffer_id = driver.allocate_buffer(8).unwrap();

        driver.write_buffer(buffer_id, &[1, 2, 3, 4]).unwrap();

        driver.submit(buffer_id, valid_command(44)).unwrap();

        assert_eq!(driver.queued_len(), 1);
    }

    #[test]
    fn recovered_driver_keeps_offline_marker() {
        let offline = TypedDriver::<Offline>::new(2).unwrap();

        let failure = offline.load_firmware(b"bad").unwrap_err();

        let recovered = failure.into_driver();

        assert_eq!(recovered.state_name(), "offline");

        assert_eq!(recovered.runtime_state(), &DeviceState::Offline);
    }
}
