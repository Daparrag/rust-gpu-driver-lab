use std::fmt::Debug;

use crate::queue::{CommandQueue, QueueError};
use crate::static_queue::{StaticCommandQueue, StaticQueueError};
use crate::submission::ValidatedSubmission;

/// Queue behavior required by a generic GPU driver.
///
/// This trait is intentionally open: external crates may implement it
/// for their own queue types.
pub trait SubmissionQueue: Debug {
    type Error: Debug + PartialEq + Eq;
    fn enqueue(&mut self, submission: ValidatedSubmission) -> Result<(), Self::Error>;
    fn dequeue(&mut self) -> Option<ValidatedSubmission>;
    fn peek(&self) -> Option<&ValidatedSubmission>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn is_full(&self) -> bool;
}

/// Implements the SubmissionQueue Contract for CommandQueue
impl SubmissionQueue for CommandQueue {
    type Error = QueueError;
    fn enqueue(&mut self, submission: ValidatedSubmission) -> Result<(), Self::Error> {
        CommandQueue::enqueue(self, submission)
    }
    fn dequeue(&mut self) -> Option<ValidatedSubmission> {
        CommandQueue::dequeue(self)
    }
    fn peek(&self) -> Option<&ValidatedSubmission> {
        CommandQueue::peek(self)
    }
    fn len(&self) -> usize {
        CommandQueue::len(self)
    }
    fn is_empty(&self) -> bool {
        CommandQueue::is_empty(self)
    }
    fn is_full(&self) -> bool {
        CommandQueue::is_full(self)
    }
}

/// Implements the SubmissionQueue Contract for StaticCommandQueue
impl<const N: usize> SubmissionQueue for StaticCommandQueue<N> {
    type Error = StaticQueueError;
    fn enqueue(&mut self, submission: ValidatedSubmission) -> Result<(), Self::Error> {
        StaticCommandQueue::enqueue(self, submission)
    }
    fn dequeue(&mut self) -> Option<ValidatedSubmission> {
        StaticCommandQueue::dequeue(self)
    }
    fn peek(&self) -> Option<&ValidatedSubmission> {
        StaticCommandQueue::peek(self)
    }
    fn len(&self) -> usize {
        StaticCommandQueue::len(self)
    }
    fn is_empty(&self) -> bool {
        StaticCommandQueue::is_empty(self)
    }
    fn is_full(&self) -> bool {
        StaticCommandQueue::is_full(self)
    }
}
