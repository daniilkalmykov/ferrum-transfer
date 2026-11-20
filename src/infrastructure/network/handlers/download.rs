use crate::{
    Error, ErrorType,
    errors::{ConnectionError, ProtocolError, ServerError, StorageError, build_error_or_unknown},
    infrastructure::network::{ConnectionContext, Frame, Opcode, Status, response},
};

use super::Handler;

use async_trait::async_trait;
use bytes::{BufMut, BytesMut};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::Instrument;

pub struct Download;

#[async_trait]
impl Handler for Download {
    fn opcode(&self) -> Opcode {
        Opcode::Download
    }

    async fn handle(&self, frame: Frame, ctx: &mut ConnectionContext<'_>) -> Result<(), Error> {
        const CHUNK_SIZE: usize = 64 * 1024;

        let file_name = match frame {
            Frame::Download { file_name } => file_name,
            _ => {
                return Err(build_error_or_unknown(
                    ctx.errors_registry,
                    ProtocolError::InvalidFrame,
                    ErrorType::Respond,
                ));
            }
        };
        let span = tracing::info_span!("DOWNLOAD", file_name = %file_name);

        async move {
            tracing::info!("DOWNLOAD received");
            tracing::info!("Starting download");

            let (mut file, file_size) = match ctx.storage.start_download(&file_name).await {
                Ok((file, file_size)) => (file, file_size),
                Err(err) => {
                    tracing::warn!(error = %err, "Begin download failed");

                    return Err(build_error_or_unknown(
                        ctx.errors_registry,
                        err,
                        ErrorType::Respond,
                    ));
                }
            };

            tracing::info!("Starting download succeded");

            let mut payload = BytesMut::new();

            payload.put_u64(file_size);

            let response = response::encode(Status::Ready, &payload);

            let timeout = tokio::time::timeout(Duration::from_secs(ctx.transfer_timeout as u64), ctx.stream.write_all(&response)).await;

            match timeout {
                Ok(result) => {
                    if let Err(err) = result {
                        tracing::error!(error = %err, "sending response failed");

                        return Err(build_error_or_unknown(
                            ctx.errors_registry,
                            ConnectionError::IOError(err),
                            ErrorType::Fatal,
                        ));
                    }
                },
                Err(error) => {
                    tracing::error!(error = %error, "Server couldn't write response in {} seconds", ctx.transfer_timeout);

                    return Err(build_error_or_unknown(
                        ctx.errors_registry,
                        ServerError::TransferTimeout,
                        ErrorType::Fatal,
                    ));
                },
            }

            tracing::info!("Starting sending file");

            let mut chunk_buffer = BytesMut::zeroed(CHUNK_SIZE);

            let mut remaining = file_size;

            let mut hasher = Sha256::new();

            while remaining > 0 {
                let chunk_to_write = std::cmp::min(CHUNK_SIZE, remaining as usize);

                let chunk = &mut chunk_buffer[..chunk_to_write];

                if let Err(err) = file.read_exact(chunk).await {
                    tracing::error!(error = %err, "reading file failed");

                    return Err(build_error_or_unknown(
                        ctx.errors_registry,
                        StorageError::IOError(err),
                        ErrorType::Fatal,
                    ));
                }

                let mut sent = 0;

                while sent < chunk_to_write {
                    let timeout = tokio::time::timeout(
                        Duration::from_secs(ctx.transfer_timeout as u64),
                        ctx.stream.write(&mut chunk[sent..chunk_to_write]),
                    )
                    .await;

                    let bytes_sent = match timeout {
                        Ok(result) => match result {
                            Ok(bytes_sent) => bytes_sent,
                            Err(err) => {
                                tracing::error!(error = %err, "sending file failed");

                                return Err(build_error_or_unknown(
                                    ctx.errors_registry,
                                    ConnectionError::IOError(err),
                                    ErrorType::Fatal,
                                ));
                            }
                        },
                        Err(err) => {
                            tracing::error!(error = %err, "Server couldn't write buffer in {} seconds", ctx.transfer_timeout);

                            return Err(build_error_or_unknown(
                                ctx.errors_registry,
                                ServerError::TransferTimeout,
                                ErrorType::Fatal,
                            ));
                        },
                    };

                    if bytes_sent == 0 {
                        tracing::info!("Writed zero bytes");

                        return Err(build_error_or_unknown(ctx.errors_registry, ConnectionError::WriteZero, ErrorType::Fatal));
                    }

                    sent += bytes_sent;
                }

                hasher.update(&chunk);

                remaining -= chunk.len() as u64;
            }

            let response = response::encode(Status::Ok, hasher.finalize().as_slice());

            let timeout = tokio::time::timeout(Duration::from_secs(ctx.transfer_timeout as u64), ctx.stream.write_all(&response)).await;

            match timeout {
                Ok(result) => {
                    if let Err(err) = result {
                        tracing::error!(error = %err, "sending hash failed");

                        return Err(build_error_or_unknown(
                            ctx.errors_registry,
                            ConnectionError::IOError(err),
                            ErrorType::Fatal,
                        ));
                    }
                },
                Err(error) => {
                    tracing::error!(error = %error, "Server couldn't write response in {} seconds", ctx.transfer_timeout);

                    return Err(build_error_or_unknown(
                        ctx.errors_registry,
                        ServerError::TransferTimeout,
                        ErrorType::Fatal,
                    ));
                },
            }

            Ok(())
        }
        .instrument(span)
        .await
    }
}
