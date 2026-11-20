#[macro_use]
pub mod macros;

mod errors_codes_registry;

pub use errors_codes_registry::ErrorMeta;
pub use errors_codes_registry::ErrorsRegistry;
pub use errors_codes_registry::RegisteredError;

mod error;

pub use error::Error;
pub use error::ErrorType;

use crate::Opcode;

define_error!(
    StorageError {
        IOError(std::io::Error) => {
            code: 0,
            client_msg: |_| "Storage IO Error".to_string(),
            display_msg: |error| format!("Storage IO Error: {}", error)
        },
        InvalidFilename => { code: 1, client_msg: "Invalid filename".to_string() },
        FileNotFound => { code: 2, client_msg: "File not found".to_string() },
        FileAlreadyExists => { code: 3, client_msg: "File already exists".to_string() },
        FileTooLarge  => { code: 4, client_msg: "File too large".to_string() },
        MismatchedHash => {code: 5, client_msg: "Mismatched hash".to_string() },
        DirNotFound => { code: 6, client_msg: "Directory not found" }
    }
);

define_error!(
    ProtocolError {
            InvalidOpcode(u8) => { code: 0, client_msg: |opcode| format!("Invalid opcode {}", opcode)},
            InvalidFrame      => { code: 1, client_msg: "Invalid frame" },
            TooLargeFrame     => { code: 2, client_msg: "Too large frame" },
            NotEmptyPayload   => { code: 3, client_msg: "Payload must be empty" },
            InvalidUtf8       => { code: 4, client_msg: "Invalid UTF-8" },
        }
);

define_error!(DefaultError {
    Unknown => {code: 0, client_msg: "Unknown Error"},
});

define_error!(ServerError {
    ServerBusy => { code: 0, client_msg: "Server is busy", },
    ServerNotWorking => { code: 1, client_msg: "Server is not accepting any requests" },
    TransferTimeout => { code: 2, client_msg: "Timeout for transfering file" },
    Deadline => { code: 3, client_msg: "Deadline for handling request" },
});

define_error!(HandlerError {
    RegistryError(Opcode) => {code: 0, client_msg: |opcode: &Opcode| format!("Handler registration failed. Opcode {}", opcode.to_string())},
    NotFound => {code: 1, client_msg: "Handler is not found"}
});

define_error!(ConnectionError {
    IOError(std::io::Error) => {
        code: 0,
        client_msg: |_| "Connection IO Error".to_string(),
        display_msg: |error| format!("Connection IO Error: {}", error)
    },
    ClientDisconnected => {code: 1, client_msg: "Client disconnected"},
    WriteZero => { code: 2, client_msg: "Writed zero bytes" },
    SlowConnection => { code: 3, client_msg: "Slow connection. Couldn't format frame" },
});

define_error!(ExecutorError {
    RegistryError(Opcode) => {code: 0, client_msg: |opcode: &Opcode| format!("Executor registration failed. Opcode {}", opcode.to_string())},
    NotFound => {code: 1, client_msg: "Executor is not found"}
});

pub fn build_error_or_unknown<T: RegisteredError + ErrorMeta>(
    errors_registry: &ErrorsRegistry,
    error: T,
    error_type: ErrorType,
) -> Error {
    match Error::build(errors_registry, error, error_type.clone()) {
        Some(error) => error,
        None => Error::build(errors_registry, DefaultError::Unknown, error_type).unwrap(),
    }
}
