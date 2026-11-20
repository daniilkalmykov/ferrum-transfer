use crate::{ErrorMeta, ErrorsRegistry, RegisteredError};

#[derive(Clone)]
pub enum ErrorType {
    Respond,
    Fatal,
    RespondAndFatal,
}

pub struct Error {
    error_type: ErrorType,
    value: Box<dyn RegisteredError>,
    error_code: u16,
}

impl Error {
    pub fn build<T: RegisteredError + ErrorMeta>(
        errors_registry: &ErrorsRegistry,
        error: T,
        error_type: ErrorType,
    ) -> Option<Self> {
        let error_code = match errors_registry.get_code(&error) {
            Some(code) => code,
            None => {
                return None;
            }
        };

        Some(Self {
            error_type,
            value: Box::new(error),
            error_code,
        })
    }

    pub fn error_type(&self) -> &ErrorType {
        &self.error_type
    }

    pub fn value(&self) -> &dyn RegisteredError {
        &(*self.value)
    }

    pub fn error_code(&self) -> u16 {
        self.error_code
    }
}
