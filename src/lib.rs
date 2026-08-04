use std::collections::HashMap;

pub mod command;
pub mod device_state;
pub mod driver;
pub mod generic_driver;
pub mod queue;
pub mod queue_backend;
pub mod static_queue;
pub mod submission;
pub mod typed_driver;
pub mod typed_generic_driver;

/// Identifier returned to a client when a GPU buffer is allocated.
///
/// The fields are private so clients cannot construct arbitrary identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(u32);

/// Errors produced by our first simulated GPU-buffer manager.
#[derive(Debug, PartialEq, Eq)]
pub enum DriverError {
    ZeroSizedAllocation,
    UnknownBuffer(BufferId),
    BufferIdExhausted,
    WriteTooLarge {
        capacity: usize,
        requested: usize,
    },
    RangeOverflow {
        offset: usize,
        length: usize,
    },
    WriteOutOfBounds {
        offset: usize,
        length: usize,
        capacity: usize,
    },
    ReadOutOfBounds {
        offset: usize,
        length: usize,
        initialized: usize,
    },
}

/// A simulated DMA-capable GPU buffer.
///
/// `storage` owns the allocated bytes.
/// `used` identifies how many bytes currently contain valid data.
#[derive(Debug, PartialEq, Eq)]
pub struct GpuBuffer {
    id: BufferId,
    storage: Vec<u8>,
    used: usize,
}

impl GpuBuffer {
    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn capacity(&self) -> usize {
        self.storage.len()
    }

    pub fn data(&self) -> &[u8] {
        &self.storage[..self.used]
    }
}

/// Owns all buffers that are currently allocated to the simulated device.
#[derive(Debug, Default)]
pub struct GpuDevice {
    next_id: u32,
    buffers: HashMap<BufferId, GpuBuffer>,
}

impl GpuDevice {
    /// check for overflow
    fn checked_end(offset: usize, length: usize) -> Result<usize, DriverError> {
        offset
            .checked_add(length)
            .ok_or(DriverError::RangeOverflow { offset, length })
    }
    /// Write data beginning at a specific byte offset.
    ///
    /// A successful write may increase `buffer.used`.
    /// A failed write must leave the buffer unchanged.
    pub fn write_buffer_at(
        &mut self,
        id: BufferId,
        offset: usize,
        data: &[u8],
    ) -> Result<(), DriverError> {
        let buffer = self
            .buffers
            .get_mut(&id)
            .ok_or(DriverError::UnknownBuffer(id))?;
        let capacity = buffer.capacity();
        let length = data.len();
        let requested = Self::checked_end(offset, length)?;
        if requested > capacity {
            return Err(DriverError::WriteOutOfBounds {
                offset,
                length,
                capacity,
            });
        }
        buffer.storage[offset..requested].copy_from_slice(data);
        buffer.used = buffer.used.max(requested);
        Ok(())
    }

    /// Return a borrowed range from initialized buffer data.
    pub fn read_buffer_range(
        &self,
        id: BufferId,
        offset: usize,
        length: usize,
    ) -> Result<&[u8], DriverError> {
        //check offset
        let buffer = self
            .buffers
            .get(&id)
            .ok_or(DriverError::UnknownBuffer(id))?;

        let initialized = buffer.used;

        let end = Self::checked_end(offset, length)?;
        if initialized < end {
            return Err(DriverError::ReadOutOfBounds {
                offset,
                length,
                initialized,
            });
        }
        // return the right buffer range
        Ok(&buffer.storage[offset..end])
    }

    /// Mark the buffer as containing no valid client data.
    ///
    /// It is not necessary to overwrite the allocated storage.
    pub fn clear_buffer(&mut self, id: BufferId) -> Result<(), DriverError> {
        self.buffers
            .get_mut(&id)
            .map(|buffer| buffer.used = 0)
            .ok_or(DriverError::UnknownBuffer(id))
    }

    /// Allocate an empty buffer with the requested capacity.
    pub fn allocate_buffer(&mut self, capacity: usize) -> Result<BufferId, DriverError> {
        if capacity == 0 {
            return Err(DriverError::ZeroSizedAllocation);
        }

        // create a new GpuBuffer with the given capacity and a unique BufferId
        let next_id = self
            .next_id
            .checked_add(1)
            .ok_or(DriverError::BufferIdExhausted)?;

        let id = BufferId(self.next_id);
        self.next_id = next_id;

        let buffer = GpuBuffer {
            id,
            storage: vec![0; capacity],
            used: 0,
        };

        self.buffers.insert(id, buffer);
        Ok(id)
    }

    /// Copy client data into an existing buffer.
    ///
    /// A failed oversized write must leave the original buffer unchanged.
    pub fn write_buffer(&mut self, id: BufferId, data: &[u8]) -> Result<(), DriverError> {
        match self.buffers.get_mut(&id) {
            None => Err(DriverError::UnknownBuffer(id)),
            Some(buffer) => {
                let requested = data.len();
                let capacity = buffer.capacity();
                if requested > capacity {
                    return Err(DriverError::WriteTooLarge {
                        capacity,
                        requested,
                    });
                }
                buffer.storage[0..requested].copy_from_slice(data);
                buffer.used = requested;
                Ok(())
            }
        }
    }

    /// Borrow the valid contents of an existing buffer.
    pub fn read_buffer(&self, id: BufferId) -> Result<&[u8], DriverError> {
        self.buffers
            .get(&id)
            .map(GpuBuffer::data)
            .ok_or(DriverError::UnknownBuffer(id))
    }

    /// Remove a buffer and transfer its ownership to the caller.
    pub fn release_buffer(&mut self, id: BufferId) -> Result<GpuBuffer, DriverError> {
        self.buffers
            .remove(&id)
            .ok_or(DriverError::UnknownBuffer(id))
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }
    /// Return the number of bytes that have been written to the buffer.
    pub fn initialized_len(&self, id: BufferId) -> Result<usize, DriverError> {
        self.buffers
            .get(&id)
            .map(|buffer| buffer.used)
            .ok_or(DriverError::UnknownBuffer(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_buffer_ids_are_rejected() {
        let mut device = GpuDevice {
            next_id: u32::MAX,
            ..Default::default()
        };

        assert_eq!(
            device.allocate_buffer(1),
            Err(DriverError::BufferIdExhausted)
        );

        assert_eq!(device.buffer_count(), 0);
    }
    #[test]
    fn allocate_write_and_read_buffer() {
        let mut device = GpuDevice::default();

        let id = device.allocate_buffer(8).unwrap();

        assert_eq!(device.buffer_count(), 1);

        device.write_buffer(id, &[10, 20, 30]).unwrap();

        assert_eq!(device.read_buffer(id).unwrap(), &[10, 20, 30]);
    }

    #[test]
    fn zero_sized_allocation_is_rejected() {
        let mut device = GpuDevice::default();

        let result = device.allocate_buffer(0);

        assert_eq!(result, Err(DriverError::ZeroSizedAllocation));
        assert_eq!(device.buffer_count(), 0);
    }

    #[test]
    fn oversized_write_does_not_modify_existing_data() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(3).unwrap();

        device.write_buffer(id, &[1, 2]).unwrap();

        let result = device.write_buffer(id, &[7, 8, 9, 10]);

        assert_eq!(
            result,
            Err(DriverError::WriteTooLarge {
                capacity: 3,
                requested: 4,
            })
        );

        assert_eq!(device.read_buffer(id).unwrap(), &[1, 2]);
    }

    #[test]
    fn releasing_transfers_buffer_ownership() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(4).unwrap();

        device.write_buffer(id, &[42, 43]).unwrap();

        let released = device.release_buffer(id).unwrap();

        assert_eq!(released.id(), id);
        assert_eq!(released.capacity(), 4);
        assert_eq!(released.data(), &[42, 43]);
        assert_eq!(device.buffer_count(), 0);
        assert_eq!(device.read_buffer(id), Err(DriverError::UnknownBuffer(id)));
    }

    #[test]
    fn allocations_receive_different_ids() {
        let mut device = GpuDevice::default();

        let id1 = device.allocate_buffer(4).unwrap();
        let id2 = device.allocate_buffer(4).unwrap();

        assert_ne!(id1, id2);
        assert_eq!(device.buffer_count(), 2);
    }

    #[test]
    fn writes_and_reads_at_an_offset() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(8).unwrap();

        device.write_buffer_at(id, 3, &[10, 20]).unwrap();

        assert_eq!(device.read_buffer_range(id, 3, 2).unwrap(), &[10, 20]);

        assert_eq!(device.read_buffer(id).unwrap(), &[0, 0, 0, 10, 20]);
    }

    #[test]
    fn double_write_valid_capacity() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(2).unwrap();

        device.write_buffer(id, &[1, 2]).unwrap();
        device.write_buffer_at(id, 0, &[3, 4]).unwrap();

        assert_eq!(device.read_buffer(id).unwrap(), &[3, 4]);
    }

    #[test]
    fn double_write_invalid_capacity() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(5).unwrap();
        device.write_buffer_at(id, 3, &[10, 20]).unwrap();
        let result = device.write_buffer_at(id, 3, &[1, 2, 3]);
        assert_eq!(
            result,
            Err(DriverError::WriteOutOfBounds {
                offset: 3,
                length: 3,
                capacity: 5,
            })
        );
        assert_eq!(device.read_buffer(id).unwrap(), &[0, 0, 0, 10, 20]);
    }

    #[test]
    fn writing_before_current_end_does_not_shrink_used_length() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(8).unwrap();

        device.write_buffer_at(id, 4, &[40, 50]).unwrap();
        device.write_buffer_at(id, 1, &[10]).unwrap();

        assert_eq!(device.read_buffer(id).unwrap(), &[0, 10, 0, 0, 40, 50]);
    }

    #[test]
    fn overflowing_range_is_rejected() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(8).unwrap();

        let result = device.write_buffer_at(id, usize::MAX, &[1, 2]);

        assert_eq!(
            result,
            Err(DriverError::RangeOverflow {
                offset: usize::MAX,
                length: 2,
            })
        );
    }

    #[test]
    fn out_of_bounds_write_preserves_existing_data() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(4).unwrap();

        device.write_buffer(id, &[1, 2, 3]).unwrap();

        let result = device.write_buffer_at(id, 3, &[8, 9]);

        assert_eq!(
            result,
            Err(DriverError::WriteOutOfBounds {
                offset: 3,
                length: 2,
                capacity: 4,
            })
        );

        assert_eq!(device.read_buffer(id).unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn reading_uninitialized_range_is_rejected() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(8).unwrap();

        device.write_buffer(id, &[1, 2, 3]).unwrap();

        let result = device.read_buffer_range(id, 2, 2);

        assert_eq!(
            result,
            Err(DriverError::ReadOutOfBounds {
                offset: 2,
                length: 2,
                initialized: 3,
            })
        );
    }

    #[test]
    fn clearing_preserves_capacity_but_removes_valid_data() {
        let mut device = GpuDevice::default();
        let id = device.allocate_buffer(8).unwrap();

        device.write_buffer(id, &[1, 2, 3]).unwrap();
        device.clear_buffer(id).unwrap();

        assert_eq!(device.read_buffer(id).unwrap(), &[]);
        assert_eq!(device.buffer_count(), 1);

        let released = device.release_buffer(id).unwrap();
        assert_eq!(released.capacity(), 8);
    }
}
