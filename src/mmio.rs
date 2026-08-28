use std::marker::PhantomData;
use std::mem::align_of;
use std::ptr::NonNull;

#[derive(Debug, PartialEq, Eq)]
pub enum MmioError {
    EmptyRegion,
    NullBase,
    MisalignedBase {
        address: usize,
        required_alignment: usize,
    },
    OutOfBounds {
        index: usize,
        register_count: usize,
    },
}

/// Lifetime-bound region of 32-bit MMIO registers.
///
/// The region provides checked volatile access to its registers.
///
/// The region cannot outlive safely borrowed backing storage:
///
/// ```compile_fail
/// use rust_gpu_driver_lab::mmio::MmioRegion;
///
/// let region;
///
/// {
///     let mut registers = [0_u32; 2];
///
///     region =
///         MmioRegion::from_slice(&mut registers)
///         .unwrap();
/// }
///
/// let _ = region.read(0);
/// ```
#[derive(Debug)]
pub struct MmioRegion<'a> {
    base: NonNull<u32>,
    register_count: usize,
    marker: PhantomData<&'a mut [u32]>,
}

impl<'a> MmioRegion<'a> {
    /// Construct an MMIO region from exclusively borrowed storage.
    pub fn from_slice(registers: &'a mut [u32]) -> Result<Self, MmioError> {
        if registers.is_empty() {
            return Err(MmioError::EmptyRegion);
        }
        Ok(Self {
            base: NonNull::<u32>::new(&mut registers[0] as *mut _).ok_or(MmioError::NullBase)?,
            register_count: registers.len(),
            marker: PhantomData,
        })
    }
    /// Construct an MMIO region from raw parts.
    ///
    /// # Safety
    ///
    /// If `base` is non-null, correctly aligned and
    /// `register_count > 0`, the caller must guarantee that:
    ///
    /// - `base` points to at least `register_count` consecutive
    ///
    /// readable and writable `u32` registers;
    /// - the complete region remains valid for lifetime `'a`;
    /// - no incompatible references or accesses violate the
    ///
    /// exclusive access represented by this region;
    /// - volatile `u32` access is valid for the target hardware.
    ///
    /// Null, empty and misaligned inputs are rejected before the
    /// pointer is accessed.
    pub unsafe fn from_raw_parts(base: *mut u32, register_count: usize) -> Result<Self, MmioError> {
        if base.is_null() {
            return Err(MmioError::NullBase);
        }

        if register_count == 0 {
            return Err(MmioError::EmptyRegion);
        }
        if !(base as usize).is_multiple_of(align_of::<u32>()) {
            return Err(MmioError::MisalignedBase {
                address: base as usize,
                required_alignment: align_of::<u32>(),
            });
        }

        Ok(Self {
            base: NonNull::new(base).ok_or(MmioError::NullBase)?,
            register_count,
            marker: PhantomData,
        })
    }
    pub fn len(&self) -> usize {
        self.register_count
    }
    pub fn is_empty(&self) -> bool {
        self.register_count == 0
    }
    pub fn read(&self, index: usize) -> Result<u32, MmioError> {
        self.check_index(index)?;
        // SAFETY:
        // - The constructor guarantees a valid register region.
        // - `check_index` guarantees that `index` is inside it.
        // - `add(index)` therefore remains inside the region.
        // - The caller of the raw constructor guarantees that volatile
        // u32 reads are valid for this hardware mapping.
        let value = unsafe { self.base.add(index).read_volatile() };
        Ok(value)
    }
    pub fn write(&mut self, index: usize, value: u32) -> Result<(), MmioError> {
        self.check_index(index)?;
        // SAFETY:
        // The same region and bounds guarantees used by `read` apply.
        unsafe { self.base.add(index).write_volatile(value) };
        Ok(())
    }

    fn check_index(&self, index: usize) -> Result<(), MmioError> {
        if index >= self.len() {
            return Err(MmioError::OutOfBounds {
                index,
                register_count: self.len(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slice_is_rejected() {
        let mut registers = [];
        assert!(matches!(
            MmioRegion::from_slice(&mut registers),
            Err(MmioError::EmptyRegion)
        ));
    }
    #[test]
    fn volatile_read_and_write_round_trip() {
        let mut registers = [0_u32; 3];
        {
            let mut region = MmioRegion::from_slice(&mut registers).unwrap();
            region.write(1, 0xA5A5_5A5A).unwrap();
            assert_eq!(region.read(1), Ok(0xA5A5_5A5A));
        }
        assert_eq!(registers[1], 0xA5A5_5A5A);
    }

    #[test]
    fn out_of_bounds_write_preserves_registers() {
        let mut registers = [11_u32, 22];
        {
            let mut region = MmioRegion::from_slice(&mut registers).unwrap();
            assert_eq!(
                region.write(2, 99),
                Err(MmioError::OutOfBounds {
                    index: 2,
                    register_count: 2,
                })
            );
        }
        assert_eq!(registers, [11, 22]);
    }
    #[test]
    fn null_raw_base_is_rejected() {
        let result = unsafe { MmioRegion::from_raw_parts(std::ptr::null_mut(), 1) };
        assert!(matches!(result, Err(MmioError::NullBase)));
    }
    #[test]
    fn misaligned_raw_base_is_rejected() {
        let mut bytes = [0_u8; 8];
        let base = unsafe { bytes.as_mut_ptr().add(1) } as *mut u32;
        let address = base as usize;
        let result = unsafe { MmioRegion::from_raw_parts(base, 1) };
        assert_eq!(
            result.unwrap_err(),
            MmioError::MisalignedBase {
                address,
                required_alignment: align_of::<u32>(),
            }
        );
    }

    #[test]
    #[allow(clippy::drop_non_drop)]
    fn valid_raw_region_supports_access() {
        let mut registers = [0_u32; 2];
        let mut region =
            unsafe { MmioRegion::from_raw_parts(registers.as_mut_ptr(), registers.len()) }.unwrap();
        region.write(0, 42).unwrap();
        assert_eq!(region.read(0), Ok(42));
        drop(region);
        assert_eq!(registers[0], 42);
    }
    #[test]
    fn empty_raw_region_is_rejected() {
        let mut register = 0_u32;

        let result = unsafe { MmioRegion::from_raw_parts(&mut register, 0) };

        assert!(matches!(result, Err(MmioError::EmptyRegion)));
    }
    #[test]
    fn out_of_bound_read() {
        let mut registers = [0_u32; 2];
        let mut region =
            unsafe { MmioRegion::from_raw_parts(registers.as_mut_ptr(), registers.len()) }.unwrap();
        region.write(1, 0xA5A5_5A5A).unwrap();
        assert_eq!(
            region.read(2),
            Err(MmioError::OutOfBounds {
                index: 2,
                register_count: 2,
            })
        );
    }
    #[test]
    fn out_of_bound_write() {
        let mut registers = [0_u32; 2];
        let mut region =
            unsafe { MmioRegion::from_raw_parts(registers.as_mut_ptr(), registers.len()) }.unwrap();
        assert_eq!(
            region.write(2, 0xA5A5_5A5A),
            Err(MmioError::OutOfBounds {
                index: 2,
                register_count: 2,
            })
        );
    }
    #[test]
    fn registers_are_independent() {
        let mut registers = [0_u32; 3];
        let mut region = MmioRegion::from_slice(&mut registers).unwrap();
        region.write(0, 10).unwrap();
        region.write(2, 30).unwrap();
        assert_eq!(region.read(0), Ok(10));
        assert_eq!(region.read(1), Ok(0));
        assert_eq!(region.read(2), Ok(30));
    }
}
