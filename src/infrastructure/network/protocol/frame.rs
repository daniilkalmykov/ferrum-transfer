use crate::errors::ProtocolError;

use super::Opcode;
use bytes::{Buf, BytesMut};

const MAX_FRAME_SIZE: usize = 1024;

pub enum Frame {
    Ping,
    Upload { file_name: String, file_size: u64 },
    Download { file_name: String },
}

impl Frame {
    pub fn decode(buffer: &mut BytesMut) -> Result<Option<Self>, ProtocolError> {
        if buffer.len() < 4 {
            return Ok(None);
        }

        let bytes: [u8; 4] = buffer[..4]
            .try_into()
            .map_err(|_| ProtocolError::InvalidFrame)?;

        let payload_length = u32::from_be_bytes(bytes) as usize;

        if payload_length > MAX_FRAME_SIZE {
            return Err(ProtocolError::TooLargeFrame);
        }

        if buffer.len() < 4 + 1 + payload_length {
            return Ok(None);
        }

        let opcode = Opcode::try_from(buffer[4])?;

        match opcode {
            Opcode::Ping => {
                if payload_length != 0 {
                    return Err(ProtocolError::NotEmptyPayload);
                }

                buffer.advance(5);
                return Ok(Some(Frame::Ping));
            }
            Opcode::Upload => {
                if payload_length < 2 {
                    return Err(ProtocolError::InvalidFrame);
                }

                let file_name_length = u16::from_be_bytes([buffer[5], buffer[6]]) as usize;

                if file_name_length == 0 {
                    return Err(ProtocolError::InvalidFrame);
                }

                if payload_length != 2 + file_name_length + 8 {
                    return Err(ProtocolError::InvalidFrame);
                }

                let file_name = String::from_utf8(buffer[7..7 + file_name_length].to_vec())
                    .map_err(|_| ProtocolError::InvalidUtf8)?;

                let file_size_start = 7 + file_name_length;

                let file_size_bytes: [u8; 8] = buffer[file_size_start..file_size_start + 8]
                    .try_into()
                    .map_err(|_| ProtocolError::InvalidFrame)?;

                let file_size = u64::from_be_bytes(file_size_bytes);

                buffer.advance(5 + payload_length);

                Ok(Some(Frame::Upload {
                    file_name,
                    file_size,
                }))
            }
            Opcode::Download => {
                if payload_length < 2 {
                    return Err(ProtocolError::InvalidFrame);
                }

                let file_name_length = u16::from_be_bytes([buffer[5], buffer[6]]) as usize;

                if file_name_length == 0 {
                    return Err(ProtocolError::InvalidFrame);
                }

                if buffer.len() < 2 + file_name_length {
                    return Err(ProtocolError::InvalidFrame);
                }

                if payload_length != 2 + file_name_length {
                    return Err(ProtocolError::InvalidFrame);
                }

                let file_name = String::from_utf8(buffer[7..7 + file_name_length].to_vec())
                    .map_err(|_| ProtocolError::InvalidUtf8)?;

                buffer.advance(5 + payload_length);

                Ok(Some(Frame::Download { file_name }))
            }
        }
    }
    pub fn get_opcode(&self) -> Opcode {
        match self {
            Frame::Ping => Opcode::Ping,
            Frame::Upload { .. } => Opcode::Upload,
            Frame::Download { .. } => Opcode::Download,
        }
    }
}
