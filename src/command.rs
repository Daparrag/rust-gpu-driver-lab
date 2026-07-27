#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandId(u64);

impl CommandId {
    pub fn new(raw: u64) -> Result<Self, CommandError> {
        if raw == 0 {
            return Err(CommandError::ZeroCommandId);
        }
        Ok(Self(raw))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priority(u8);

impl Priority {
    pub const MIN: u8 = 0;
    pub const MAX: u8 = 3;

    pub fn new(raw: u8) -> Result<Self, CommandError> {
        if !(Self::MIN..=Self::MAX).contains(&raw) {
            return Err(CommandError::InvalidPriority {
                found: raw,
                minimum: Self::MIN,
                maximum: Self::MAX,
            });
        }
        Ok(Self(raw))
    }

    pub fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Priority {
    type Error = CommandError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandRange {
    offset: usize,
    length: usize,
}

impl CommandRange {
    pub const ALIGNMENT: usize = 4;

    pub fn new(offset: usize, length: usize) -> Result<Self, CommandError> {
        if offset % Self::ALIGNMENT != 0 {
            return Err(CommandError::MisalignedOffset {
                offset,
                required_alignment: Self::ALIGNMENT,
            });
        }
        if length == 0 {
            return Err(CommandError::ZeroLength);
        }
        offset
            .checked_add(length)
            .ok_or(CommandError::RangeOverflow { offset, length })?;

        Ok(Self { offset, length })
    }

    pub fn offset(self) -> usize {
        self.offset
    }

    pub fn length(self) -> usize {
        self.length
    }

    pub fn end(self) -> usize {
        // Safe because construction must prove that offset + length
        // cannot overflow.
        self.offset + self.length
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RawCommand {
    pub id: u64,
    pub offset: usize,
    pub length: usize,
    pub priority: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedCommand {
    id: CommandId,
    range: CommandRange,
    priority: Priority,
}

impl ValidatedCommand {
    pub fn id(&self) -> CommandId {
        self.id
    }

    pub fn range(&self) -> CommandRange {
        self.range
    }

    pub fn priority(&self) -> Priority {
        self.priority
    }
}

impl TryFrom<RawCommand> for ValidatedCommand {
    type Error = CommandError;

    fn try_from(raw: RawCommand) -> Result<Self, Self::Error> {
        let id = CommandId::new(raw.id)?;
        let range = CommandRange::new(raw.offset, raw.length)?;
        let priority = Priority::try_from(raw.priority)?;
        Ok(Self {
            id,
            range,
            priority,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    ZeroCommandId,
    ZeroLength,
    MisalignedOffset {
        offset: usize,
        required_alignment: usize,
    },
    RangeOverflow {
        offset: usize,
        length: usize,
    },
    InvalidPriority {
        found: u8,
        minimum: u8,
        maximum: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_raw_command() -> RawCommand {
        RawCommand {
            id: 1,
            offset: 8,
            length: 16,
            priority: 2,
        }
    }
    #[test]
    fn valid_raw_command_is_converted() {
        let command = ValidatedCommand::try_from(valid_raw_command()).unwrap();

        assert_eq!(command.id().get(), 1);
        assert_eq!(command.range().offset(), 8);
        assert_eq!(command.range().length(), 16);
        assert_eq!(command.range().end(), 24);
        assert_eq!(command.priority().get(), 2);
    }
    #[test]
    fn validated_raw_command_with_incomplete_fields() {
        assert_eq!(
            ValidatedCommand::try_from(RawCommand::default()),
            Err(CommandError::ZeroCommandId)
        );
    }

    #[test]
    fn zero_command_id_is_rejected() {
        let mut raw = valid_raw_command();
        raw.id = 0;

        assert_eq!(
            ValidatedCommand::try_from(raw),
            Err(CommandError::ZeroCommandId)
        );
    }

    #[test]
    fn zero_length_is_rejected() {
        let mut raw = valid_raw_command();
        raw.length = 0;

        assert_eq!(
            ValidatedCommand::try_from(raw),
            Err(CommandError::ZeroLength)
        );
    }

    #[test]
    fn misaligned_offset_is_rejected() {
        let mut raw = valid_raw_command();
        raw.offset = 6;

        assert_eq!(
            ValidatedCommand::try_from(raw),
            Err(CommandError::MisalignedOffset {
                offset: 6,
                required_alignment: 4,
            })
        );
    }

    #[test]
    fn overflowing_range_is_rejected() {
        let mut raw = valid_raw_command();
        raw.offset = usize::MAX - 3;
        raw.length = 8;

        assert_eq!(
            ValidatedCommand::try_from(raw),
            Err(CommandError::RangeOverflow {
                offset: usize::MAX - 3,
                length: 8,
            })
        );
    }

    #[test]
    fn invalid_priority_is_rejected() {
        let mut raw = valid_raw_command();
        raw.priority = 4;

        assert_eq!(
            ValidatedCommand::try_from(raw),
            Err(CommandError::InvalidPriority {
                found: 4,
                minimum: 0,
                maximum: 3,
            })
        );
    }

    #[test]
    fn priority_boundaries_are_valid() {
        assert_eq!(Priority::new(0).unwrap().get(), 0);
        assert_eq!(Priority::new(3).unwrap().get(), 3);
    }
    #[test]
    fn maximum_u8_priority_is_rejected() {
        let result = Priority::new(u8::MAX);

        assert_eq!(
            result,
            Err(CommandError::InvalidPriority {
                found: 255,
                minimum: Priority::MIN,
                maximum: Priority::MAX,
            })
        );
    }
}
