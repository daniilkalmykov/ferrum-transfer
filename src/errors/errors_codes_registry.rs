use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    fmt::Display,
};

pub struct ErrorsRegistry {
    errors: HashMap<TypeId, u16>,
}

impl ErrorsRegistry {
    const BASE_RANGE: std::ops::RangeInclusive<u16> = 100..=999;
    const LOCAL_CODE_RANGE: std::ops::RangeInclusive<u16> = 0..=99;

    pub fn new() -> Self {
        Self {
            errors: HashMap::new(),
        }
    }

    pub fn register<T: RegisteredError + ErrorMeta>(&mut self, base: u16) -> bool {
        if let Some(max) = T::LOCAL_CODES.iter().max() {
            if !Self::LOCAL_CODE_RANGE.contains(max) {
                return false;
            }
        } else {
            return false;
        }

        if has_duplicates(T::LOCAL_CODES) {
            return false;
        }

        if !Self::BASE_RANGE.contains(&base) || base % 100 != 0 {
            return false;
        }

        if self.errors.contains_key(&TypeId::of::<T>()) {
            return false;
        }

        if self.errors.values().any(|code| code == &base) {
            return false;
        }

        self.errors.insert(TypeId::of::<T>(), base);

        true
    }

    pub fn get_code<T: RegisteredError + ErrorMeta>(&self, error: &T) -> Option<u16> {
        let local_code = &error.local_code();

        if !Self::LOCAL_CODE_RANGE.contains(local_code) || !T::LOCAL_CODES.contains(local_code) {
            return None;
        }

        self.errors
            .get(&TypeId::of::<T>())
            .map(|base| base + local_code)
    }
}

pub trait RegisteredError: 'static + Display + std::error::Error + Send + Sync {
    fn local_code(&self) -> u16;
    fn client_message(&self) -> String;
}

pub trait ErrorMeta {
    const LOCAL_CODES: &'static [u16];
}

fn has_duplicates(numbers: &[u16]) -> bool {
    let mut seen = HashSet::new();

    for &item in numbers {
        if !seen.insert(item) {
            return true;
        }
    }

    false
}
