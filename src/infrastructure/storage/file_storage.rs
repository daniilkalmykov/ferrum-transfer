use super::UploadSession;

use crate::{Config, errors::StorageError};
use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use chrono::Local;
use tokio::{
    fs::{self, File, OpenOptions},
    io::AsyncWriteExt,
};
use uuid::Uuid;

const MAX_FILE_SIZE: u64 = 1024 * 1024 * 1024 * 20; // 20GB

#[async_trait]
pub trait Storage: Send + Sync {
    async fn begin_upload(
        &self,
        file_name: &str,
        file_size: u64,
    ) -> Result<UploadSession, StorageError>;

    async fn write_bytes(&self, bytes: &[u8], file: &mut File) -> Result<(), StorageError>;

    async fn finish_upload(
        &self,
        file_name: &str,
        upload_session: UploadSession,
    ) -> Result<(), StorageError>;

    async fn abort_upload(&self, session: UploadSession) -> Result<(), StorageError>;

    async fn start_download(&self, file_name: &str) -> Result<(File, u64), StorageError>;

    async fn clean_temp_files(&self) -> Result<(), StorageError>;
}

pub struct FileStorage {
    storage_path: PathBuf,
    temp_path: PathBuf,
}

impl FileStorage {
    pub async fn new(config: &Config) -> Result<Self, std::io::Error> {
        let storage_path = config.storage_path().to_path_buf();
        let temp_path = storage_path.join("temp");

        tokio::fs::create_dir_all(&storage_path).await?;
        tokio::fs::create_dir_all(&temp_path).await?;

        Ok(Self {
            storage_path,
            temp_path,
        })
    }
}

#[async_trait]
impl Storage for FileStorage {
    async fn begin_upload(
        &self,
        file_name: &str,
        file_size: u64,
    ) -> Result<UploadSession, StorageError> {
        if file_size > MAX_FILE_SIZE {
            return Err(StorageError::FileTooLarge);
        }

        if !is_valid_file_name(file_name) {
            return Err(StorageError::InvalidFilename);
        }

        let final_path = self.storage_path.join(file_name);

        let now = Local::now().format("%d%m%Y-%H%M%S").to_string();
        let id = Uuid::new_v4().to_string();

        let part_path = self
            .temp_path
            .join(format!("{file_name}({now})({id}).part"));

        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part_path)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Error while creating file");
                StorageError::IOError(e)
            })?;

        let session = UploadSession::new(file, part_path);

        let try_exists = match fs::try_exists(&final_path).await {
            Ok(try_exists) => try_exists,
            Err(error) => {
                tracing::error!(error = %error, "Error while checking if file exists");

                let _ = self.abort_upload(session).await;

                return Err(StorageError::IOError(error));
            }
        };

        if try_exists {
            tracing::warn!("File already exists, removing part file");

            let _ = self.abort_upload(session).await;

            return Err(StorageError::FileAlreadyExists);
        }

        Ok(session)
    }

    async fn write_bytes(&self, bytes: &[u8], file: &mut File) -> Result<(), StorageError> {
        file.write_all(bytes).await.map_err(|e| {
            tracing::error!(error = %e, "Error while writing bytes");
            StorageError::IOError(e)
        })
    }

    async fn finish_upload(
        &self,
        file_name: &str,
        mut upload_session: UploadSession,
    ) -> Result<(), StorageError> {
        let file = upload_session.file();

        let file = match file {
            Some(file) => file,
            None => {
                return Err(StorageError::FileNotFound);
            }
        };

        if let Err(err) = file.flush().await {
            tracing::error!(error = %err, "Error while flushing file");

            let _ = self.abort_upload(upload_session).await.map_err(|e| {
                tracing::error!(error = %e, "Error while aborting upload");
            });

            return Err(StorageError::IOError(err));
        }

        upload_session.drop_file();

        let final_path = self.storage_path.join(file_name);
        let part_path = upload_session.path();

        if let Err(err) = fs::hard_link(part_path, &final_path).await {
            tracing::error!(error = %err, "Error while creating hard link");

            let _ = self.abort_upload(upload_session).await.map_err(|e| {
                tracing::error!(error = %e, "Error while aborting upload");
            });

            match err.kind() {
                std::io::ErrorKind::AlreadyExists => return Err(StorageError::FileAlreadyExists),
                _ => return Err(StorageError::IOError(err)),
            }
        }

        if let Err(err) = fs::remove_file(part_path).await {
            tracing::error!(error = %err, "Error while removing part file");
        } else {
            upload_session.commit();
        }

        Ok(())
    }

    async fn abort_upload(&self, mut session: UploadSession) -> Result<(), StorageError> {
        session.drop_file();

        let part_path = session.path();

        if let Err(err) = fs::remove_file(part_path).await {
            tracing::error!(error = %err, "Error while removing file");

            return Err(StorageError::IOError(err));
        }

        session.abort();
        Ok(())
    }

    async fn start_download(&self, file_name: &str) -> Result<(File, u64), StorageError> {
        if !is_valid_file_name(file_name) {
            return Err(StorageError::InvalidFilename);
        }

        let path = self.storage_path.join(file_name);

        let file = match fs::File::open(path).await {
            Ok(file) => file,
            Err(err) => {
                tracing::error!(error = %err, "Error while opening file");

                match err.kind() {
                    std::io::ErrorKind::NotFound => {
                        return Err(StorageError::FileNotFound);
                    }
                    _ => {
                        return Err(StorageError::IOError(err));
                    }
                }
            }
        };

        let data = match file.metadata().await {
            Ok(data) => data,
            Err(err) => {
                tracing::error!(error = %err, "Error while getting file metadata");
                return Err(StorageError::IOError(err));
            }
        };

        if data.is_file() == false {
            return Err(StorageError::InvalidFilename);
        }

        Ok((file, data.len()))
    }

    async fn clean_temp_files(&self) -> Result<(), StorageError> {
        let temp_path = &self.temp_path;

        let mut entries = match fs::read_dir(temp_path).await {
            Ok(entries) => entries,
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => {
                    tracing::error!("Directory is not found.");
                    return Err(StorageError::DirNotFound);
                }
                _ => {
                    tracing::error!(error = %err, directon = %&temp_path.display(), "Failed reading temp directon");

                    return Err(StorageError::IOError(err));
                }
            },
        };

        let mut files_deleted: u32 = 0;
        let mut size_deleted: u64 = 0;

        loop {
            let entry = match entries.next_entry().await {
                Ok(entry) => entry,
                Err(err) => {
                    tracing::error!(error = %err, "Failed to get next entry");

                    return Err(StorageError::IOError(err));
                }
            };

            if let Some(entry) = entry {
                let path = &entry.path();

                let file_type = match entry.file_type().await {
                    Ok(file_type) => file_type,
                    Err(err) => {
                        tracing::error!(error = %err, path = %path.display(), "Couldn't get a file type");
                        continue;
                    }
                };

                if file_type.is_file() == false {
                    continue;
                }

                let file_name = match path.file_name().and_then(|name| name.to_str()) {
                    Some(file_name) => file_name,
                    None => continue,
                };

                let metadata = match fs::metadata(path).await {
                    Ok(metadata) => metadata,
                    Err(err) => {
                        tracing::error!(file_name = file_name, error = %err, path = %path.display(), "Couldn't get file metadata");

                        continue;
                    }
                };

                let file_size = metadata.len();

                if let Some(ext) = Path::new(path).extension().and_then(|ext| ext.to_str()) {
                    if ext == "part" {
                        if let Err(err) = fs::remove_file(path).await {
                            tracing::error!(error = %err, "Error while removing file");

                            continue;
                        }

                        files_deleted += 1;
                        size_deleted += file_size;
                    }
                }
            } else {
                break;
            }
        }

        tracing::info!(
            files_deleted = files_deleted,
            size_deleted = size_deleted,
            "Files deleted - {files_deleted}. Size of the deleted files - {size_deleted}"
        );
        Ok(())
    }
}

fn is_valid_file_name(file_name: &str) -> bool {
    let mut components = Path::new(file_name).components();

    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}
