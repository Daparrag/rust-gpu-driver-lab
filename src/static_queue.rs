use crate::submission::ValidatedSubmission;

#[derive(Debug, PartialEq, Eq)]
pub enum StaticQueueError {
    ZeroCapacity,
    QueueFull { capacity: usize },
    DuplicateCommandId { id: u64 },
}

/// Fixed-capacity FIFO queue.
///
/// The capacity is part of the queue's type.
///
/// ```compile_fail
/// use rust_gpu_driver_lab::static_queue::StaticCommandQueue;
///
/// fn accept_four(
///     _queue: StaticCommandQueue<4>,
/// ) {}
///
/// let queue =
///     StaticCommandQueue::<8>::new().unwrap();
///
/// accept_four(queue);
/// ```
#[derive(Debug)]
pub struct StaticCommandQueue<const N: usize> {
    slots: [Option<ValidatedSubmission>; N],
    head: usize,
    len: usize,
}

impl<const N: usize> StaticCommandQueue<N> {
    pub fn new() -> Result<Self, StaticQueueError> {
        if N == 0 {
            return Err(StaticQueueError::ZeroCapacity);
        }

        Ok(Self {
            slots: std::array::from_fn(|_| None),
            head: 0,
            len: 0,
        })
    }
    fn tail_index(&self) -> usize {
        let distance_to_end = N - self.head;

        if self.len >= distance_to_end {
            self.len - distance_to_end
        } else {
            self.head + self.len
        }
    }
    pub const fn capacity(&self) -> usize {
        N
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len == N
    }

    pub fn enqueue(&mut self, submission: ValidatedSubmission) -> Result<(), StaticQueueError> {
        let command_id = submission.command().id().get();

        let duplicate = self
            .slots
            .iter()
            .flatten()
            .any(|queued| queued.command().id().get() == command_id);

        if duplicate {
            return Err(StaticQueueError::DuplicateCommandId { id: command_id });
        }

        if self.is_full() {
            return Err(StaticQueueError::QueueFull { capacity: N });
        }

        let tail = self.tail_index();

        self.slots[tail] = Some(submission);
        self.len += 1;

        Ok(())
    }

    pub fn peek(&self) -> Option<&ValidatedSubmission> {
        if self.is_empty() {
            return None;
        }
        let submission = self.slots[self.head].as_ref()?;
        Some(submission)
    }

    pub fn dequeue(&mut self) -> Option<ValidatedSubmission> {
        if self.is_empty() {
            return None;
        }
        let submission = self.slots[self.head].take()?;
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(submission)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    use crate::GpuDevice;
    use crate::command::RawCommand;
    use crate::submission::{SubmissionRequest, validate_submission};

    fn make_submission(id: u64) -> ValidatedSubmission {
        let mut device = GpuDevice::default();

        let buffer_id = device.allocate_buffer(8).unwrap();

        device.write_buffer(buffer_id, &[0; 8]).unwrap();

        validate_submission(
            &device,
            SubmissionRequest {
                buffer_id,
                command: RawCommand {
                    id,
                    offset: 0,
                    length: 4,
                    priority: 1,
                },
            },
        )
        .unwrap()
    }
    #[test]
    fn zero_capacity_is_rejected() {
        assert!(matches!(
            StaticCommandQueue::<0>::new(),
            Err(StaticQueueError::ZeroCapacity)
        ));
    }
    #[test]
    fn queue_capacity_return_const() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        assert_eq!(queue.capacity(), 2);
    }
    #[test]
    fn queue_is_empty() {
        let queue = StaticCommandQueue::<2>::new().unwrap();
        assert!(queue.is_empty());
    }
    #[test]
    fn queue_fifo_ordering_preserved() {
        let mut queue = StaticCommandQueue::<2>::new().unwrap();
        queue.enqueue(make_submission(10)).unwrap();
        queue.enqueue(make_submission(20)).unwrap();

        let first = queue.dequeue().unwrap();
        let second = queue.dequeue().unwrap();
        //let is_empty = queue.is_empty();

        assert_eq!(first.command().id().get(), 10);
        assert_eq!(second.command().id().get(), 20);
        assert!(queue.is_empty());
    }
    #[test]
    fn duplicate_command_id_is_rejected() {
        let mut queue = StaticCommandQueue::<2>::new().unwrap();
        queue.enqueue(make_submission(10)).unwrap();
        let result = queue.enqueue(make_submission(10));
        assert!(matches!(
            result,
            Err(StaticQueueError::DuplicateCommandId { id: 10 })
        ));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.peek().unwrap().command().id().get(), 10);
    }

    #[test]
    fn full_queue_rejects_new_submission() {
        let mut queue = StaticCommandQueue::<1>::new().unwrap();
        queue.enqueue(make_submission(10)).unwrap();
        let result = queue.enqueue(make_submission(20));
        assert!(matches!(
            result,
            Err(StaticQueueError::QueueFull { capacity: 1 })
        ));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.peek().unwrap().command().id().get(), 10);
    }

    #[test]
    fn peek_does_not_remove_submission() {
        let mut queue = StaticCommandQueue::<2>::new().unwrap();
        queue.enqueue(make_submission(7)).unwrap();
        let first = queue.peek().unwrap();

        assert_eq!(first.command().id().get(), 7);

        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
    }
    #[test]
    fn dequeue_transfer_ownership() {
        let mut queue = StaticCommandQueue::<2>::new().unwrap();
        queue.enqueue(make_submission(7)).unwrap();
        let first = queue.dequeue().unwrap();
        assert_eq!(first.command().id().get(), 7);
        assert_eq!(queue.peek(), None);
    }
    #[test]
    fn dequeued_command_id_can_be_reused() {
        let mut queue = StaticCommandQueue::<2>::new().unwrap();
        queue.enqueue(make_submission(7)).unwrap();
        let first = queue.dequeue().unwrap();
        assert_eq!(first.command().id().get(), 7);
        queue.enqueue(make_submission(7)).unwrap();
        let peek_result = queue.peek().unwrap();
        assert_eq!(peek_result.command().id().get(), 7);
        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
    }
    #[test]
    fn fifo_ordering_survives_wraparound() {
        let mut queue = StaticCommandQueue::<3>::new().unwrap();

        queue.enqueue(make_submission(1)).unwrap();
        queue.enqueue(make_submission(2)).unwrap();
        queue.enqueue(make_submission(3)).unwrap();

        assert_eq!(queue.dequeue().unwrap().command().id().get(), 1);

        queue.enqueue(make_submission(4)).unwrap();

        assert_eq!(queue.dequeue().unwrap().command().id().get(), 2);

        assert_eq!(queue.dequeue().unwrap().command().id().get(), 3);

        assert_eq!(queue.dequeue().unwrap().command().id().get(), 4);

        assert!(queue.is_empty());
    }
}
