use std::collections::{HashSet, VecDeque};

use crate::command::CommandId;
use crate::submission::ValidatedSubmission;

#[derive(Debug, PartialEq, Eq)]
pub enum QueueError {
    ZeroCapacity,
    QueueFull { capacity: usize },
    DuplicateCommandId(CommandId),
}

#[derive(Debug)]
pub struct CommandQueue {
    capacity: usize,
    pending: VecDeque<ValidatedSubmission>,
    active_ids: HashSet<CommandId>,
}

impl CommandQueue {
    pub fn new(capacity: usize) -> Result<Self, QueueError> {
        if capacity == 0 {
            return Err(QueueError::ZeroCapacity);
        }

        Ok(Self {
            capacity,
            pending: VecDeque::with_capacity(capacity),
            active_ids: HashSet::new(),
        })
    }
    /// Move a validated submission into the pending queue.
    ///
    /// Failed enqueue operations must not modify the queue.
    pub fn enqueue(&mut self, submission: ValidatedSubmission) -> Result<(), QueueError> {
        let cmd_id = submission.command().id();
        // check if cmd is on the active ids
        if self.contains_command(cmd_id) {
            return Err(QueueError::DuplicateCommandId(cmd_id));
        }
        // check if queue is full
        if self.is_full() {
            return Err(QueueError::QueueFull {
                capacity: self.capacity,
            });
        }
        // add id to active ids
        self.active_ids.insert(cmd_id);
        // enqueue the submission
        self.pending.push_back(submission);
        Ok(())
    }

    /// Remove and return the oldest pending submission.
    pub fn dequeue(&mut self) -> Option<ValidatedSubmission> {
        // return None if queue is empty
        let submission = self.pending.pop_front()?;
        let cmd_id = submission.command().id();
        self.active_ids.remove(&cmd_id);
        Some(submission)
    }

    /// Borrow the oldest submission without removing it.
    pub fn peek(&self) -> Option<&ValidatedSubmission> {
        // Return None if queue is empty
        self.pending.front()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.pending.len() == self.capacity
    }

    pub fn contains_command(&self, id: CommandId) -> bool {
        self.active_ids.contains(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::GpuDevice;
    use crate::command::RawCommand;
    use crate::submission::{SubmissionRequest, validate_submission};

    fn make_submission(command_id: u64) -> ValidatedSubmission {
        let mut device = GpuDevice::default();

        let buffer_id = device.allocate_buffer(16).unwrap();

        device.write_buffer(buffer_id, &[0; 16]).unwrap();

        validate_submission(
            &device,
            SubmissionRequest {
                buffer_id,
                command: RawCommand {
                    id: command_id,
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
            CommandQueue::new(0),
            Err(QueueError::ZeroCapacity)
        ));
    }

    #[test]
    fn queue_processes_submissions_in_fifo_order() {
        let mut queue = CommandQueue::new(2).unwrap();

        queue.enqueue(make_submission(10)).unwrap();
        queue.enqueue(make_submission(20)).unwrap();

        let first = queue.dequeue().unwrap();
        let second = queue.dequeue().unwrap();

        assert_eq!(first.command().id().get(), 10);

        assert_eq!(second.command().id().get(), 20);

        assert!(queue.is_empty());
    }

    #[test]
    fn duplicate_command_id_is_rejected() {
        let mut queue = CommandQueue::new(3).unwrap();

        queue.enqueue(make_submission(10)).unwrap();

        let result = queue.enqueue(make_submission(10));

        assert_eq!(
            result,
            Err(QueueError::DuplicateCommandId(
                crate::command::CommandId::new(10).unwrap()
            ))
        );

        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn full_queue_rejects_new_submission() {
        let mut queue = CommandQueue::new(1).unwrap();

        queue.enqueue(make_submission(10)).unwrap();

        let result = queue.enqueue(make_submission(20));

        assert_eq!(result, Err(QueueError::QueueFull { capacity: 1 }));

        assert_eq!(queue.len(), 1);

        assert_eq!(queue.peek().unwrap().command().id().get(), 10);
    }

    #[test]
    fn dequeued_command_id_can_be_reused() {
        let mut queue = CommandQueue::new(1).unwrap();

        queue.enqueue(make_submission(10)).unwrap();

        queue.dequeue().unwrap();

        assert!(!queue.contains_command(crate::command::CommandId::new(10).unwrap()));

        assert!(queue.enqueue(make_submission(10)).is_ok());
    }

    #[test]
    fn peek_does_not_remove_submission() {
        let mut queue = CommandQueue::new(2).unwrap();

        queue.enqueue(make_submission(7)).unwrap();

        let first_view = queue.peek().unwrap();

        assert_eq!(first_view.command().id().get(), 7);

        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
    }

    #[test]
    fn dequeue_from_empty_queue_returns_none() {
        let mut queue = CommandQueue::new(1).unwrap();

        assert_eq!(queue.dequeue(), None);
    }
    #[test]
    fn duplicate_enqueue_does_not_make_queue_full() {
        let mut queue = CommandQueue::new(2).unwrap();
        assert!(queue.enqueue(make_submission(10)).is_ok());
        assert!(queue.enqueue(make_submission(10)).is_err());
        assert!(!queue.is_full());
    }
}
