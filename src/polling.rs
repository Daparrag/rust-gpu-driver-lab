use crate::register_fields::StatusSnapshot;
use crate::registers::{GpuRegisterBlock, RegisterBlockError};
use std::cmp::min;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Raw Polling configuration cannot be fabricated directly.
///
/// ```compile_fail
/// use std::time::{Duration};
/// use rust_gpu_driver_lab::polling::PollConfig;
/// let _ = PollConfig {
///   timeout: Duration::from_millis(10),
///   poll_interval: Duration::from_millis(10),
/// };
/// ```
pub struct PollConfig {
    timeout: Duration,
    poll_interval: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollConfigError {
    ZeroPollInterval,
}

impl PollConfig {
    pub fn new(timeout: Duration, poll_interval: Duration) -> Result<Self, PollConfigError> {
        if poll_interval.is_zero() {
            return Err(PollConfigError::ZeroPollInterval);
        }
        Ok(Self {
            timeout,
            poll_interval,
        })
    }
    pub const fn timeout(self) -> Duration {
        self.timeout
    }
    pub const fn poll_interval(self) -> Duration {
        self.poll_interval
    }
}

pub trait PollTimer {
    type Instant: Copy;
    fn now(&self) -> Self::Instant;

    fn elapsed_since(&self, start: Self::Instant) -> Duration;

    fn delay(&mut self, duration: Duration);
}

#[derive(Debug, Default)]
pub struct HostPollTimer;

impl PollTimer for HostPollTimer {
    type Instant = Instant;
    fn now(&self) -> Self::Instant {
        Instant::now()
    }

    fn elapsed_since(&self, start: Self::Instant) -> Duration {
        start.elapsed()
    }

    fn delay(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

pub trait StatusReader {
    type Error;
    fn read_status(&mut self) -> Result<StatusSnapshot, Self::Error>;
}

impl StatusReader for GpuRegisterBlock<'_> {
    type Error = RegisterBlockError;

    fn read_status(&mut self) -> Result<StatusSnapshot, Self::Error> {
        GpuRegisterBlock::read_status(self)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PollError<E> {
    Source(E),
    Fault {
        status: StatusSnapshot,
        polls: u64,
    },
    Timeout {
        timeout: Duration,
        polls: u64,
        last_status: StatusSnapshot,
    },
}

pub fn wait_until_ready<R, T>(
    reader: &mut R,
    timer: &mut T,
    config: PollConfig,
) -> Result<StatusSnapshot, PollError<R::Error>>
where
    R: StatusReader,
    T: PollTimer,
{
    let start = timer.now();
    let mut polls = 0_u64;
    loop {
        polls = polls.saturating_add(1);
        let status = reader.read_status().map_err(PollError::Source)?;

        if status.fault() {
            return Err(PollError::Fault { status, polls });
        }

        if status.ready() {
            return Ok(status);
        }
        let elapsed = timer.elapsed_since(start);
        if elapsed >= config.timeout() {
            return Err(PollError::Timeout {
                timeout: config.timeout(),
                polls,
                last_status: status,
            });
        }
        let remaining = config.timeout().saturating_sub(elapsed);
        timer.delay(min(config.poll_interval(), remaining));
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[derive(Debug, Default)]
    struct FakePollTimer {
        now: Duration,
        delays: Vec<Duration>,
    }

    impl PollTimer for FakePollTimer {
        type Instant = Duration;
        fn now(&self) -> Self::Instant {
            self.now
        }

        fn elapsed_since(&self, start: Self::Instant) -> Duration {
            self.now.saturating_sub(start)
        }

        fn delay(&mut self, duration: Duration) {
            self.delays.push(duration);
            self.now += duration;
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeReadError {
        Failed,
    }

    #[derive(Debug)]
    struct ScriptedStatusReader {
        statuses: Vec<StatusSnapshot>,
        next: usize,
    }

    impl ScriptedStatusReader {
        fn new(statuses: Vec<StatusSnapshot>) -> Self {
            Self { statuses, next: 0 }
        }
    }

    impl StatusReader for ScriptedStatusReader {
        type Error = FakeReadError;
        fn read_status(&mut self) -> Result<StatusSnapshot, Self::Error> {
            let status = self
                .statuses
                .get(self.next)
                .copied()
                .or_else(|| self.statuses.last().copied())
                .ok_or(FakeReadError::Failed)?;
            self.next = self.next.saturating_add(1);
            Ok(status)
        }
    }

    #[derive(Debug)]
    struct FailingStatusReader;
    impl StatusReader for FailingStatusReader {
        type Error = FakeReadError;
        fn read_status(&mut self) -> Result<StatusSnapshot, Self::Error> {
            Err(FakeReadError::Failed)
        }
    }

    #[test]
    fn zero_poll_interval_is_rejected() {
        assert_eq!(
            PollConfig::new(Duration::from_millis(10), Duration::ZERO,),
            Err(PollConfigError::ZeroPollInterval)
        );
    }

    #[test]
    fn immediately_ready_device_does_not_delay() {
        let mut reader = ScriptedStatusReader::new(vec![StatusSnapshot::from_raw(0b0001)]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(10), Duration::from_millis(2)).unwrap();
        let status = wait_until_ready(&mut reader, &mut timer, config).unwrap();
        assert!(status.ready());
        assert!(timer.delays.is_empty());
    }
    #[test]
    fn busy_device_is_polled_until_ready() {
        let mut reader = ScriptedStatusReader::new(vec![
            StatusSnapshot::from_raw(0b0010),
            StatusSnapshot::from_raw(0b0010),
            StatusSnapshot::from_raw(0b0001),
        ]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(20), Duration::from_millis(3)).unwrap();
        let status = wait_until_ready(&mut reader, &mut timer, config).unwrap();
        assert!(status.ready());
        assert_eq!(timer.delays.len(), 2);
        assert_eq!(timer.now, Duration::from_millis(6));
    }

    #[test]
    fn fault_stops_polling_immediately() {
        let fault = StatusSnapshot::from_raw(0b0100);
        let mut reader = ScriptedStatusReader::new(vec![fault]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(20), Duration::from_millis(2)).unwrap();
        assert_eq!(
            wait_until_ready(&mut reader, &mut timer, config,),
            Err(PollError::Fault {
                status: fault,
                polls: 1,
            })
        );
        assert!(timer.delays.is_empty());
    }

    #[test]
    fn fault_has_priority_over_ready() {
        let ready_and_fault = StatusSnapshot::from_raw(0b0101);
        let mut reader = ScriptedStatusReader::new(vec![ready_and_fault]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(10), Duration::from_millis(1)).unwrap();
        assert!(matches!(
            wait_until_ready(&mut reader, &mut timer, config,),
            Err(PollError::Fault { .. })
        ));
    }

    #[test]
    fn timeout_reports_last_status() {
        let busy = StatusSnapshot::from_raw(0b0010);
        let mut reader = ScriptedStatusReader::new(vec![busy]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(5), Duration::from_millis(2)).unwrap();
        assert_eq!(
            wait_until_ready(&mut reader, &mut timer, config,),
            Err(PollError::Timeout {
                timeout: Duration::from_millis(5),
                polls: 4,
                last_status: busy,
            })
        );
    }

    #[test]
    fn zero_timeout_performs_one_status_check() {
        let busy = StatusSnapshot::from_raw(0b0010);
        let mut reader = ScriptedStatusReader::new(vec![busy]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::ZERO, Duration::from_millis(1)).unwrap();
        assert_eq!(
            wait_until_ready(&mut reader, &mut timer, config,),
            Err(PollError::Timeout {
                timeout: Duration::ZERO,
                polls: 1,
                last_status: busy,
            })
        );
        assert!(timer.delays.is_empty());
    }

    #[test]
    fn polling_does_not_delay_past_timeout_budget() {
        let busy = StatusSnapshot::from_raw(0b0010);
        let mut reader = ScriptedStatusReader::new(vec![busy]);
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(5), Duration::from_millis(4)).unwrap();
        let _ = wait_until_ready(&mut reader, &mut timer, config);
        assert_eq!(
            timer.delays,
            vec![Duration::from_millis(4), Duration::from_millis(1),]
        );
        assert_eq!(timer.now, Duration::from_millis(5));
    }

    #[test]
    fn source_error_is_preserved() {
        let mut reader = FailingStatusReader;
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(5), Duration::from_millis(1)).unwrap();
        assert_eq!(
            wait_until_ready(&mut reader, &mut timer, config,),
            Err(PollError::Source(FakeReadError::Failed))
        );
    }
    #[test]
    fn register_block_can_be_polled_through_status_reader() {
        let mut storage = [0, 0b0001, 0, 0, 0];
        let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        let mut timer = FakePollTimer::default();
        let config = PollConfig::new(Duration::from_millis(10), Duration::from_millis(1)).unwrap();
        let result = wait_until_ready(&mut block, &mut timer, config).unwrap();
        assert!(result.ready());
    }
}
