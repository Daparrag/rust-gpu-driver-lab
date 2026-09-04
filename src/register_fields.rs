use crate::registers::{COMMAND, CONTROL, GpuRegisterBlock, RegisterBlockError, STATUS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PowerMode {
    Idle = 0,
    LowPower = 1,
    Normal = 2,
    Performance = 3,
}

impl From<u32> for PowerMode {
    fn from(bits: u32) -> Self {
        match bits & 0b11 {
            0 => Self::Idle,
            1 => Self::LowPower,
            2 => Self::Normal,
            3 => Self::Performance,
            _ => unreachable!(),
        }
    }
}

impl PowerMode {
    const fn bits(self) -> u32 {
        self as u32
    }
}

const CONTROL_ENABLE_MASK: u32 = 0b00001;
const CONTROL_IRQ_ENABLE_MASK: u32 = 0b01000;
const CONTROL_POWERMODE_MASK: u32 = 0b00110;
const CONTROL_POWERMODE_SHIFT: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlValue {
    raw: u32,
}

impl ControlValue {
    pub const fn from_raw(raw: u32) -> Self {
        Self { raw }
    }

    pub const fn raw(self) -> u32 {
        self.raw
    }

    pub const fn enabled(self) -> bool {
        self.raw & CONTROL_ENABLE_MASK != 0
    }
    pub const fn irq_enabled(self) -> bool {
        self.raw & CONTROL_IRQ_ENABLE_MASK != 0
    }

    pub fn power_mode(self) -> PowerMode {
        let bits = (self.raw & CONTROL_POWERMODE_MASK) >> CONTROL_POWERMODE_SHIFT;
        PowerMode::from(bits)
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        if enabled {
            self.raw |= CONTROL_ENABLE_MASK;
        } else {
            self.raw &= !CONTROL_ENABLE_MASK;
        }
        self
    }

    pub fn with_irq_enabled(mut self, enabled: bool) -> Self {
        if enabled {
            self.raw |= CONTROL_IRQ_ENABLE_MASK;
        } else {
            self.raw &= !CONTROL_IRQ_ENABLE_MASK;
        }
        self
    }

    pub fn with_power_mode(mut self, mode: PowerMode) -> Self {
        self.raw = (self.raw & !CONTROL_POWERMODE_MASK)
            | ((mode.bits() << CONTROL_POWERMODE_SHIFT) & CONTROL_POWERMODE_MASK);
        self
    }
}

//extend the register block without duplicating MMIO region
impl<'a> GpuRegisterBlock<'a> {
    pub fn read_control(&self) -> Result<ControlValue, RegisterBlockError> {
        let raw = self.read(CONTROL)?;
        Ok(ControlValue::from_raw(raw))
    }

    pub fn write_control(&mut self, value: ControlValue) -> Result<(), RegisterBlockError> {
        self.write(CONTROL, value.raw())
    }

    pub fn modify_control<F>(&mut self, update: F) -> Result<ControlValue, RegisterBlockError>
    where
        F: FnOnce(ControlValue) -> ControlValue,
    {
        let current = self.read_control()?;
        let updated = update(current);
        self.write_control(updated)?;
        Ok(updated)
    }
}

const STATUS_READY_MASK: u32 = 0b001;
const STATUS_BUSY_MASK: u32 = 0b010;
const STATUS_FAULT_MASK: u32 = 0b100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusSnapshot {
    raw: u32,
}

impl StatusSnapshot {
    pub const fn from_raw(raw: u32) -> Self {
        Self { raw }
    }

    pub const fn ready(self) -> bool {
        self.raw & STATUS_READY_MASK != 0
    }

    pub const fn busy(self) -> bool {
        self.raw & STATUS_BUSY_MASK != 0
    }

    pub const fn fault(self) -> bool {
        self.raw & STATUS_FAULT_MASK != 0
    }
}

impl<'a> GpuRegisterBlock<'a> {
    pub fn read_status(&self) -> Result<StatusSnapshot, RegisterBlockError> {
        let raw = self.read(STATUS)?;
        Ok(StatusSnapshot::from_raw(raw))
    }
}

pub enum CommandOpCode {
    Kick = 1,
    Flush = 2,
    Reset = 3,
}


impl CommandOpCode {
    const fn bits(self) -> u32 {
        self as u32
    }
}

const COMMAND_OPCODE_MASK: u32 = 0b1111;
const COMMAND_QUEUE_SHIFT: u32 = 8;
const COMMAND_QUEUE_MASK: u32 = 0xFF << COMMAND_QUEUE_SHIFT;

/// Raw command words cannot be fabricated directly.
///
/// ```compile_fail
/// use rust_gpu_driver_lab::register_fields::CommandWord;
///
/// let _ = CommandWord {
///  raw: 0xFFFF_FFFF,
/// };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandWord {
    raw: u32,
}

impl CommandWord {
    pub const fn new(opcode: CommandOpCode, queue_id: u8) -> Self {
        let raw =
            (opcode.bits() & COMMAND_OPCODE_MASK) | ((queue_id as u32) << COMMAND_QUEUE_SHIFT);
        Self { raw }
    }

    pub const fn queue_id(self) -> u8 {
        ((self.raw & COMMAND_QUEUE_MASK) >> COMMAND_QUEUE_SHIFT) as u8
    }

    pub const fn opcode_bits(self) -> u32 {
        self.raw & COMMAND_OPCODE_MASK
    }
    pub(crate) const fn raw(self) -> u32 {
        self.raw
    }
}

impl<'a> GpuRegisterBlock<'a> {
    pub fn issue_command(&mut self, command: CommandWord) -> Result<(), RegisterBlockError> {
        self.write(COMMAND, command.raw())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn control_fields_are_decoded() {
        let control = ControlValue::from_raw(0b1011);
        assert!(control.enabled());
        assert!(control.irq_enabled());
        assert_eq!(control.power_mode(), PowerMode::LowPower);
    }
    #[test]
    fn power_mode_can_be_changed() {
        let control = ControlValue::from_raw(0).with_power_mode(PowerMode::Performance);
        assert_eq!(control.power_mode(), PowerMode::Performance);
    }

    #[test]
    fn control_updates_preserve_reserved_bits() {
        let original = 0x8000_0000;
        let control = ControlValue::from_raw(original)
            .with_enabled(true)
            .with_irq_enabled(true);
        assert_eq!(control.raw(), 0x8000_0009);
    }

    #[test]
    fn disabling_irq_preserves_other_fields() {
        let control = ControlValue::from_raw(0x8000_000F).with_irq_enabled(false);
        assert_eq!(control.raw(), 0x8000_0007);
    }

    #[test]
    fn modify_control_preserves_existing_bits() {
        let mut storage = [0x8000_0000, 0, 0];
        {
            let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            let updated = block
                .modify_control(|control| {
                    control
                        .with_enabled(true)
                        .with_power_mode(PowerMode::Normal)
                })
                .unwrap();
            assert!(updated.enabled());
            assert_eq!(updated.power_mode(), PowerMode::Normal);
        }
        assert_eq!(storage[0], 0x8000_0005);
    }
    #[test]
    fn status_snapshot_decodes_flags() {
        let status = StatusSnapshot::from_raw(0b0101);
        assert!(status.ready());
        assert!(!status.busy());
        assert!(status.fault());
    }
    #[test]
    fn register_block_reads_typed_status() {
        let mut storage = [0, 0b0011, 0];
        let block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
        let status = block.read_status().unwrap();
        assert!(status.ready());
        assert!(status.busy());
        assert!(!status.fault());
    }
    #[test]
    fn command_word_encodes_fields() {
        let command = CommandWord::new(CommandOpCode::Kick, 7);
        assert_eq!(command.opcode_bits(), 1);
        assert_eq!(command.queue_id(), 7);
        assert_eq!(command.raw(), 0x0000_0701);
    }
    #[test]
    fn issue_command_writes_command_register() {
        let mut storage = [0_u32; 3];
        {
            let mut block = GpuRegisterBlock::from_slice(&mut storage).unwrap();
            block
                .issue_command(CommandWord::new(CommandOpCode::Flush, 4))
                .unwrap();
        }
        assert_eq!(storage, [0, 0, 0x0000_0402,]);
    }
}
