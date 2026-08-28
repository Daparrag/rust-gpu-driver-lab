use crate::mmio::{MmioError, MmioRegion};
use std::marker::PhantomData;
// seal to protect the register markets
mod sealed {
    pub trait Sealed {}
}
// define register access capabilities.
pub trait RegisterAccess: sealed::Sealed + std::fmt::Debug {}

pub trait ReadableAccess: RegisterAccess {}
pub trait WritableAccess: RegisterAccess {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOnly;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOnly;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadWrite;

// Assign capabilities to markets
impl sealed::Sealed for ReadOnly {}
impl sealed::Sealed for WriteOnly {}
impl sealed::Sealed for ReadWrite {}
// provide capability for Accessing Registers
impl RegisterAccess for ReadOnly {}
impl RegisterAccess for WriteOnly {}
impl RegisterAccess for ReadWrite {}
// permissions
impl ReadableAccess for ReadOnly {}
impl WritableAccess for WriteOnly {}
impl ReadableAccess for ReadWrite {}
impl WritableAccess for ReadWrite {}

// Register Description

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Register<A>
where
    A: RegisterAccess,
{
    index: usize,
    market: PhantomData<A>,
}

impl<A> Register<A>
where
    A: RegisterAccess,
{
    const fn new(index: usize) -> Self {
        Self {
            index,
            market: PhantomData,
        }
    }

    const fn index(self) -> usize {
        self.index
    }
}

pub const CONTROL: Register<ReadWrite> = Register::new(0);
pub const STATUS: Register<ReadOnly> = Register::new(1);
pub const COMMAND: Register<WriteOnly> = Register::new(2);

const REQUIRED_REGISTERS: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum RegisterBlockError {
    RegionTooSmall { required: usize, actual: usize },
    Mmio(MmioError),
}

impl From<MmioError> for RegisterBlockError {
    fn from(error: MmioError) -> Self {
        Self::Mmio(error)
    }
}

#[derive(Debug)]
pub struct GpuRegisterBlock<'a> {
    mmio: MmioRegion<'a>,
}

impl<'a> GpuRegisterBlock<'a> {
    pub fn from_mmio(mmio: MmioRegion<'a>) -> Result<Self, RegisterBlockError> {
        GpuRegisterBlock::check_register_len(mmio.len())?;
        Ok(Self { mmio })
    }
    pub fn from_slice(registers: &'a mut [u32]) -> Result<Self, RegisterBlockError> {
        GpuRegisterBlock::check_register_len(registers.len())?;
        let mmio = MmioRegion::from_slice(registers)?;
        Ok(Self { mmio })
    }

    fn check_register_len(len: usize) -> Result<(), RegisterBlockError> {
        if len < REQUIRED_REGISTERS {
            return Err(RegisterBlockError::RegionTooSmall {
                required: REQUIRED_REGISTERS,
                actual: len,
            });
        }
        Ok(())
    }
    /// read-only registers cannot be written.
    /// ```compile_fail
    /// use rust_gpu_driver_lab::registers::{GpuRegisterBlock,STATUS,};
    /// let mut storage = [0_u32; 3];
    /// let mut registers = GpuRegisterBlock::from_slice(&mut storage,).unwrap();
    /// registers.write(STATUS, 1).unwrap();
    /// ```
    pub fn read<A>(&self, register: Register<A>) -> Result<u32, RegisterBlockError>
    where
        A: ReadableAccess,
    {
        Ok(self.mmio.read(register.index())?)
    }

    /// write-only registers cannot be read
    /// ```compile_fail
    /// use rust_gpu_driver_lab::registers::{GpuRegisterBlock,COMMAND,};
    /// let mut storage = [0_u32; 3];
    /// let mut registers = GpuRegisterBlock::from_slice(&mut storage,).unwrap();
    /// registers.read(COMMAND).unwrap();
    /// ```
    pub fn write<A>(&mut self, register: Register<A>, value: u32) -> Result<(), RegisterBlockError>
    where
        A: WritableAccess,
    {
        Ok(self.mmio.write(register.index(), value)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn too_small_register_map_is_rejected() {
        let mut storage = [0_u32; 2];
        assert_eq!(
            GpuRegisterBlock::from_slice(&mut storage,).unwrap_err(),
            RegisterBlockError::RegionTooSmall {
                required: 3,
                actual: 2,
            }
        );
    }
    #[test]
    fn control_is_read_write() {
        let mut storage = [0_u32; 3];
        let mut registers = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        registers.write(CONTROL, 0x1234).unwrap();
        assert_eq!(registers.read(CONTROL), Ok(0x1234));
    }
    #[test]
    fn status_is_readable() {
        let mut storage = [0_u32, 0xA5A5, 0_u32];
        let registers = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        assert_eq!(registers.read(STATUS), Ok(0xA5A5));
    }
    #[test]
    fn command_is_writable() {
        let mut storage = [0_u32; 3];
        {
            let mut registers = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            registers.write(COMMAND, 42).unwrap();
        }
        assert_eq!(storage[2], 42);
    }
    #[test]
    fn register_operations_use_correct_offsets() {
        let mut storage = [0_u32; 3];
        {
            let mut registers = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            registers.write(CONTROL, 11).unwrap();
            registers.write(COMMAND, 33).unwrap();
        }
        assert_eq!(storage, [11, 0, 33]);
    }
    #[test]
    fn larger_region_is_accepted() {
        let mut storage = [0_u32; 8];
        let registers = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        assert_eq!(registers.read(STATUS), Ok(0));
    }
    #[test]
    fn mmio_errors_are_preserved() {
        let error = RegisterBlockError::from(MmioError::OutOfBounds {
            index: 5,
            register_count: 3,
        });
        assert_eq!(
            error,
            RegisterBlockError::Mmio(MmioError::OutOfBounds {
                index: 5,
                register_count: 3,
            })
        );
    }
}
