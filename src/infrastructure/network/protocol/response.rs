use super::Status;
use crate::Error;

use bytes::{BufMut, BytesMut};

pub fn encode(status: Status, payload: &[u8]) -> BytesMut {
    let mut buffer = BytesMut::with_capacity(5 + payload.len());

    buffer.put_u16(payload.len() as u16);
    buffer.put_u8(status.into());
    buffer.put_slice(payload);

    buffer
}

pub fn encode_error(error: &Error) -> BytesMut {
    let client_message = error.value().client_message();

    let mut payload = BytesMut::new();

    payload.put_u16(error.error_code());
    payload.put_slice(client_message.as_bytes());

    encode(Status::Error, &payload)
}
