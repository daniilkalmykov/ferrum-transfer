use std::fmt::Display;

use crate::errors::ProtocolError;

#[derive(Hash, PartialEq, Clone, Debug)]
pub enum Opcode {
    Ping = 0,
    Upload = 1,
    Download = 2,
}

impl TryFrom<u8> for Opcode {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Opcode::Ping),
            1 => Ok(Opcode::Upload),
            2 => Ok(Opcode::Download),
            _ => Err(ProtocolError::InvalidOpcode(value)),
        }
    }
}

impl Eq for Opcode {}

impl Display for Opcode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Opcode::Ping => write!(f, "Ping"),
            Opcode::Upload => write!(f, "Upload"),
            Opcode::Download => write!(f, "Download"),
        }
    }
}
