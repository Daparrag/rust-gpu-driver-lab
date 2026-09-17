use crate::registers::{GpuRegisterBlock, INT_ACK, INT_STATUS, RegisterBlockError};
use std::ops::{BitOr, BitOrAssign};

#[derive(Debug, PartialEq, Eq)]
pub enum InterruptError {
    Register(RegisterBlockError),
}

impl From<RegisterBlockError> for InterruptError {
    fn from(error: RegisterBlockError) -> Self {
        Self::Register(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptEvents {
    bits: u32,
}

const COMMAND_COMPLETE_MASK: u32 = 0b0001;
const QUEUE_FAULT_MASK: u32 = 0b0010;
const THERMAL_WARNING_MASK: u32 = 0b0100;
const FIRMWARE_FAULT_MASK: u32 = 0b1000;
const VALID_INTERRUPT_MASK: u32 =
    COMMAND_COMPLETE_MASK | QUEUE_FAULT_MASK | THERMAL_WARNING_MASK | FIRMWARE_FAULT_MASK;

impl InterruptEvents {
    pub const NONE: Self = Self { bits: 0 };
    pub const COMMAND_COMPLETE: Self = Self {
        bits: COMMAND_COMPLETE_MASK,
    };
    pub const QUEUE_FAULT: Self = Self {
        bits: QUEUE_FAULT_MASK,
    };
    pub const THERMAL_WARNING: Self = Self {
        bits: THERMAL_WARNING_MASK,
    };
    pub const FIRMWARE_FAULT: Self = Self {
        bits: FIRMWARE_FAULT_MASK,
    };
}

impl BitOr for InterruptEvents {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits | rhs.bits,
        }
    }
}

impl BitOrAssign for InterruptEvents {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

/// Querying an event set
impl InterruptEvents {
    pub const fn contains(self, event: Self) -> bool {
        (self.bits & event.bits) == event.bits
    }

    pub fn bits(self) -> u32 {
        self.bits
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

}

/// Raw interrupt masks cannot be fabricated directly.
///
/// ```compile_fail
/// use rust_gpu_driver_lab::interrupts::InterruptEvents;
///
/// let _ = InterruptEvents {
/// bits: 0xFFFF_FFFF,
/// };
/// ```
impl InterruptEvents {
    pub(crate) const fn from_status_bits(bits: u32) -> Self {
        Self {
            bits: bits & VALID_INTERRUPT_MASK,
        }
    }
}

/// Acknowledge events
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptSnapshot {
    raw: u32,
}

impl InterruptSnapshot {
    pub const fn from_raw(raw: u32) -> Self {
        Self { raw }
    }
    pub const fn raw(self) -> u32 {
        self.raw
    }
    pub const fn events(self) -> InterruptEvents {
        InterruptEvents::from_status_bits(self.raw)
    }
    pub const fn has(self, event: InterruptEvents) -> bool {
        self.events().contains(event)
    }
}

impl<'a> GpuRegisterBlock<'a> {
    pub fn read_interrupt_status(&self) -> Result<InterruptSnapshot, RegisterBlockError> {
        let raw = self.read(INT_STATUS)?;
        Ok(InterruptSnapshot::from_raw(raw))
    }

    pub fn ack_interrupts(&mut self, events: InterruptEvents) -> Result<(), RegisterBlockError> {
        self.write(INT_ACK, events.bits())
    }
}

#[derive(Debug)]
pub struct InterruptController;

impl Default for InterruptController {
    fn default() -> Self {
        InterruptController::new()
    }
}

impl InterruptController {
    pub const fn new() -> Self {
        Self
    }

    pub fn pending(
        &self,
        registers: &GpuRegisterBlock<'_>,
    ) -> Result<InterruptSnapshot, InterruptError> {
        Ok(registers.read_interrupt_status()?)
    }

    pub fn ack(
        &self,
        registers: &mut GpuRegisterBlock<'_>,
        events: InterruptEvents,
    ) -> Result<(), InterruptError> {
        registers.ack_interrupts(events)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn event_constants_have_correct_bits() {
        assert_eq!(InterruptEvents::COMMAND_COMPLETE.bits(), 0b0001);
        assert_eq!(InterruptEvents::QUEUE_FAULT.bits(), 0b0010);
        assert_eq!(InterruptEvents::THERMAL_WARNING.bits(), 0b0100);
        assert_eq!(InterruptEvents::FIRMWARE_FAULT.bits(), 0b1000);
    }
    #[test]
    fn combined_events_are_valid() {
        let events = InterruptEvents::COMMAND_COMPLETE | InterruptEvents::QUEUE_FAULT;
        assert_eq!(events.bits(), 0b0011);
    }
    #[test]
    fn contains_selected_events() {
        let events = InterruptEvents::COMMAND_COMPLETE | InterruptEvents::THERMAL_WARNING;
        assert!(events.contains(InterruptEvents::COMMAND_COMPLETE));
        assert!(!events.contains(InterruptEvents::QUEUE_FAULT));
    }
    #[test]
    fn unkown_interrupt_bits_are_filtered() {
        let snapshot = InterruptSnapshot::from_raw(0x8000_0001);
        assert_eq!(snapshot.events().bits(), 0b0001);
        assert_eq!(snapshot.raw(), 0x8000_0001);
    }
    #[test]
    fn read_pending_interrupts() {
        let mut storage = [0, 0, 0, 0b0101, 0];
        let block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        let snapshot = block.read_interrupt_status().unwrap();
        assert!(snapshot.has(InterruptEvents::COMMAND_COMPLETE));
        assert!(snapshot.has(InterruptEvents::THERMAL_WARNING));
    }
    #[test]
    fn acknowledge_writes_exact_event_mask() {
        let mut storage = [0_u32; 5];
        {
            let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            block
                .ack_interrupts(InterruptEvents::COMMAND_COMPLETE | InterruptEvents::QUEUE_FAULT)
                .unwrap();
        }
        assert_eq!(storage[4], 0b0011);
    }
    #[test]
    fn selective_acknowledgement_is_exact() {
        let mut storage = [0_u32; 5];
        {
            let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            block
                .ack_interrupts(InterruptEvents::THERMAL_WARNING)
                .unwrap();
        }
        assert_eq!(storage[4], 0b0100);
    }
    #[test]
    fn acknowledging_none_writes_zero() {
        let mut storage = [0_u32; 5];
        {
            let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            block.ack_interrupts(InterruptEvents::NONE).unwrap();
        }
        assert_eq!(storage[4], 0);
    }
    #[test]
    fn controller_reads_pending_events() {
        let mut storage = [0, 0, 0, 0b0010, 0];
        let block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        let controller = InterruptController::new();
        let pending = controller.pending(&block).unwrap();
        assert!(pending.has(InterruptEvents::QUEUE_FAULT));
    }
    #[test]
    fn controller_acknowledges_events() {
        let mut storage = [0_u32; 5];
        {
            let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            let controller = InterruptController::new();
            controller
                .ack(&mut block, InterruptEvents::FIRMWARE_FAULT)
                .unwrap();
        }
        assert_eq!(storage[4], 0b1000);
    }
    #[test]
    fn empty_interrupt_event_set_is_detected() {
        assert!(InterruptEvents::NONE.is_empty());
        assert!(!InterruptEvents::COMMAND_COMPLETE.is_empty());
    }
}
