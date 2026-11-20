use std::path::{Path, PathBuf};

use tokio::fs::File;

pub struct UploadSession {
    file: Option<File>,
    temp_path: PathBuf,
    state: UploadState,
}

impl UploadSession {
    pub fn new(file: File, temp_path: PathBuf) -> Self {
        Self {
            file: Some(file),
            temp_path,
            state: UploadState::Active,
        }
    }

    pub fn commit(&mut self) {
        self.state = UploadState::Commited;
    }

    pub fn file(&mut self) -> Option<&mut File> {
        self.file.as_mut()
    }

    pub fn abort(&mut self) {
        self.state = UploadState::Aborted;
    }

    pub fn drop_file(&mut self) {
        drop(self.file.take());
    }

    pub fn path(&self) -> &Path {
        &self.temp_path
    }
}

impl Drop for UploadSession {
    fn drop(&mut self) {
        if self.state == UploadState::Commited || self.state == UploadState::Aborted {
            return;
        }

        drop(self.file.take());

        let path = &self.temp_path;

        if path.exists() == false {
            return;
        }

        if let Err(err) = std::fs::remove_file(path) {
            tracing::error!(error = %err, "Deleting file failed");
        }
    }
}

#[derive(PartialEq)]
enum UploadState {
    Active,
    Commited,
    Aborted,
}
