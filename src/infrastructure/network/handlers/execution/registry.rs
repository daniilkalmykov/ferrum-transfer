use std::{collections::HashMap, sync::Arc};

use crate::{Opcode, errors::ExecutorError};

use super::Executor;

pub struct ExecutorsRegistry {
    executors: HashMap<Opcode, Arc<dyn Executor>>,
}

impl ExecutorsRegistry {
    pub fn new() -> Self {
        Self {
            executors: HashMap::new(),
        }
    }

    pub fn register<T: Executor + 'static>(
        mut self,
        executor: T,
        opcode: Opcode,
    ) -> Result<Self, ExecutorError> {
        if let Some(_) = self.executors.insert(opcode.clone(), Arc::new(executor)) {
            return Err(ExecutorError::RegistryError(opcode));
        }

        Ok(self)
    }

    pub fn get(&self, opcode: &Opcode) -> Result<&Arc<dyn Executor>, ExecutorError> {
        match self.executors.get(opcode) {
            Some(executor) => Ok(executor),
            None => Err(ExecutorError::NotFound),
        }
    }
}
