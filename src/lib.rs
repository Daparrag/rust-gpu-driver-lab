use std::collections::HashMap;

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
    WriteTooLarge {
        capacity: usize,
        requested: usize,
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
    /// Allocate an empty buffer with the requested capacity.
    pub fn allocate_buffer(
        &mut self,
        capacity: usize,
    ) -> Result<BufferId, DriverError> {
	match capacity {
	 None => return Err(DriverError::ZeroSizedAllocation),
         _ => return Ok(1),
	}	
        todo!("validate capacity, create an ID, construct and store the buffer")
    }

    /// Copy client data into an existing buffer.
    ///
    /// A failed oversized write must leave the original buffer unchanged.
    pub fn write_buffer(
        &mut self,
        id: BufferId,
        data: &[u8],
    ) -> Result<(), DriverError> {
        todo!("find the buffer, validate the size and copy the data")
    }

    /// Borrow the valid contents of an existing buffer.
    pub fn read_buffer(
        &self,
        id: BufferId,
    ) -> Result<&[u8], DriverError> {
        todo!("return a borrowed slice without cloning the buffer")
    }

    /// Remove a buffer and transfer its ownership to the caller.
    pub fn release_buffer(
        &mut self,
        id: BufferId,
    ) -> Result<GpuBuffer, DriverError> {
        todo!("remove the buffer from the map")
    }

    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            device.read_buffer(id),
            Err(DriverError::UnknownBuffer(id))
        );
    }
}
