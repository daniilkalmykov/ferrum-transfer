use super::Handler;
use crate::{
    Error, ErrorType,
    errors::{ConnectionError, ProtocolError, ServerError, StorageError, build_error_or_unknown},
    infrastructure::network::{ConnectionContext, Frame, Opcode, Status, response},
};
use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::Instrument;

use std::time::Duration;

pub struct Upload;

#[async_trait]
impl Handler for Upload {
    fn opcode(&self) -> Opcode {
        Opcode::Upload
    }

    async fn handle(&self, frame: Frame, ctx: &mut ConnectionContext<'_>) -> Result<(), Error> {
        const CHUNK_SIZE: usize = 64 * 1024;

        let (file_name, file_size) = match frame {
            Frame::Upload {
                file_name,
                file_size,
            } => (file_name, file_size),
            _ => {
                return Err(build_error_or_unknown(
                    ctx.errors_registry,
                    ProtocolError::InvalidFrame,
                    ErrorType::Respond,
                ));
            }
        };

        let span = tracing::info_span!("UPLOAD", file_name = %file_name, file_size = file_size);

        async move {
            tracing::info!("UPLOAD received");
            tracing::info!("begin_upload called");

            let mut upload_session = match ctx.storage.begin_upload(&file_name, file_size).await {
                Ok(upload_session) => upload_session,
                Err(err) => {
                    tracing::warn!(error = %err, "Begin uploading failed");

                    return Err(build_error_or_unknown(
                        ctx.errors_registry,
                        err,
                        ErrorType::Respond,
                    ));
                }
            };

            let file = match upload_session.file() {
                Some(file) => file,
                None => return Err(build_error_or_unknown(ctx.errors_registry, StorageError::FileNotFound, ErrorType::Respond)),
            };

            let response = response::encode(Status::Ready, &[]);

            if let Err(err) = ctx.stream.write_all(&response).await {
                tracing::error!(error = %err, "Sending response failed. Aborting upload");

                let _ = ctx.storage.abort_upload(upload_session).await;

                return Err(build_error_or_unknown(
                    ctx.errors_registry,
                    ConnectionError::IOError(err),
                    ErrorType::Fatal,
                ));
            }

            tracing::info!("begin_upload succeeded");

            let mut remaining = file_size;

            tracing::info!("start uploading");

            let mut chunk_buffer = BytesMut::zeroed(CHUNK_SIZE);
            let mut hasher = Sha256::new();

            while remaining > 0 {
                let chunk_to_read = std::cmp::min(CHUNK_SIZE, remaining as usize);

                let mut chunk = if !ctx.buffer.is_empty() {
                    let to_take = std::cmp::min(ctx.buffer.len(), chunk_to_read);
                    &mut ctx.buffer.split_to(to_take)[..]
                } else {
                    let mut filled = 0;

                    while filled < chunk_to_read {
                        let timeout = tokio::time::timeout(
                            Duration::from_secs(ctx.transfer_timeout as u64),
                            ctx.stream.read(&mut chunk_buffer[filled..chunk_to_read]),
                        )
                        .await;

                        let bytes_read = match timeout {
                            Ok(result) => match result {
                                Ok(bytes_read) => bytes_read,
                                Err(err) => {
                                    tracing::error!(error = %err, "Failing read buffer, deleting file");

                                    let _ = ctx.storage.abort_upload(upload_session).await;

                                    return Err(build_error_or_unknown(
                                        ctx.errors_registry,
                                        ConnectionError::IOError(err),
                                        ErrorType::RespondAndFatal,
                                    ));
                                },
                            },
                            Err(err) => {
                                tracing::error!(error = %err,
                                    "Server couldn't read buffer in {} seconds",
                                    ctx.transfer_timeout
                                );

                                let _ = ctx.storage.abort_upload(upload_session).await;

                                return Err(build_error_or_unknown(
                                    ctx.errors_registry,
                                    ServerError::TransferTimeout,
                                    ErrorType::RespondAndFatal,
                                ));
                            },
                        };

                        if bytes_read == 0 {
                            tracing::info!("Client disconnected");

                            let _ = ctx.storage.abort_upload(upload_session).await;

                            return Err(build_error_or_unknown(ctx.errors_registry, ConnectionError::ClientDisconnected, ErrorType::Fatal));
                        }

                        filled += bytes_read;
                    }

                    &mut chunk_buffer[..chunk_to_read]
                };

                let chunk_len = chunk.len() as u64;
                hasher.update(&chunk);

                if let Err(err) = ctx.storage.write_bytes(&mut chunk, file).await {
                    tracing::error!(error = %err, "Failing write_bytes, deleting file");

                    let _ = ctx.storage.abort_upload(upload_session).await;

                    return Err(build_error_or_unknown(
                        ctx.errors_registry,
                        err,
                        ErrorType::RespondAndFatal,
                    ));
                }

                remaining -= chunk_len;
            }

            let mut hash = BytesMut::zeroed(32);

            let pointer = if ctx.buffer.is_empty() {
                0
            } else {
                let to_take = std::cmp::min(ctx.buffer.len(), hash.len());

                for i in 0..to_take {
                    hash[i] = ctx.buffer.get_u8();
                }

                to_take
            };

            if pointer != 32 {
                let timeout = tokio::time::timeout(
                    Duration::from_secs(ctx.transfer_timeout as u64),
                    ctx.stream.read_exact(&mut hash[pointer..]),
                )
                .await;

                match timeout {
                    Ok(result) => {
                        if let Err(err) = result {
                            tracing::error!(error = %err, "Failing read_exact hash, deleting file");

                            let _ = ctx.storage.abort_upload(upload_session).await;

                            return Err(build_error_or_unknown(
                                ctx.errors_registry,
                                ConnectionError::IOError(err),
                                ErrorType::RespondAndFatal,
                            ));
                        }
                    }
                    Err(err) => {
                        tracing::error!(error = %err,
                            "Server couldn't get read buffer in {} seconds",
                            ctx.transfer_timeout
                        );

                        let _ = ctx.storage.abort_upload(upload_session).await;

                        return Err(build_error_or_unknown(
                            ctx.errors_registry,
                            ServerError::TransferTimeout,
                            ErrorType::RespondAndFatal,
                        ));
                    }
                }
            }

            if hasher.finalize().as_slice() != hash {
                tracing::warn!("Mismatched hash. Aborting upload");

                let _ = ctx.storage.abort_upload(upload_session).await;

                return Err(build_error_or_unknown(
                    ctx.errors_registry,
                    StorageError::MismatchedHash,
                    ErrorType::Respond,
                ));
            }

            tracing::info!("Hash checked. Start finishing upload");

            ctx.storage
                .finish_upload(&file_name, upload_session)
                .await
                .map_err(|err| {
                    build_error_or_unknown(ctx.errors_registry, err, ErrorType::Respond)
                })?;

            tracing::info!("upload finished");

            let response = response::encode(Status::Ok, &[]);

            ctx.stream.write_all(&response).await.map_err(|e| {
                tracing::error!(error = %e, "Sending response failed");
                build_error_or_unknown(
                    ctx.errors_registry,
                    ConnectionError::IOError(e),
                    ErrorType::Fatal,
                )
            })?;

            Ok(())
        }
        .instrument(span)
        .await
    }
}
