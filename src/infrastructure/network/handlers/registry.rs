use std::{collections::HashMap, sync::Arc};

use crate::{errors::HandlerError, infrastructure::network::Opcode};

use super::Handler;

pub struct HandlerRegistry {
    handlers: HashMap<Opcode, Arc<dyn Handler>>,
}

impl HandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn register<T: Handler + 'static>(mut self, handler: T) -> Result<Self, HandlerError> {
        let opcode = handler.opcode();

        if let Some(_) = self.handlers.insert(opcode.clone(), Arc::new(handler)) {
            return Err(HandlerError::RegistryError(opcode));
        }

        Ok(self)
    }

    pub fn get(&self, opcode: &Opcode) -> Result<&Arc<dyn Handler>, HandlerError> {
        match self.handlers.get(opcode) {
            Some(handler) => Ok(handler),
            None => Err(HandlerError::NotFound),
        }
    }
}
