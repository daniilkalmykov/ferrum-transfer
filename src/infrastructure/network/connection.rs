use std::{net::SocketAddr, sync::Arc, time::Duration};

use bytes::BytesMut;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::Instrument;

use super::{ConnectionContext, Frame, HandlerRegistry, response};
use crate::{
    ErrorType, ErrorsRegistry, ExecutorsRegistry, Storage,
    errors::{ConnectionError, build_error_or_unknown},
};

pub fn spawn(
    cancellation_token: CancellationToken,
    task_tracker: &TaskTracker,
    id: u64,
    storage: Arc<dyn Storage>,
    errors_registry: Arc<ErrorsRegistry>,
    transfer_semaphore: Arc<Semaphore>,
    stream: TcpStream,
    addr: SocketAddr,
    handler_registry: Arc<HandlerRegistry>,
    executors_registry: Arc<ExecutorsRegistry>,
    queue_timeout: usize,
    transfer_timeout: usize,
    connection_timeout: usize,
    handler_deadline: usize,
    connect_permission: OwnedSemaphorePermit,
    frame_deadline: usize,
) -> JoinHandle<()> {
    let span = tracing::info_span!("new connection", connection_id = id, peer_addr = %addr);

    task_tracker.spawn(
        async move {
            if let Err(error) = handle(
                stream,
                cancellation_token,
                storage,
                errors_registry,
                transfer_semaphore,
                handler_registry,
                executors_registry,
                queue_timeout,
                transfer_timeout,
                connection_timeout,
                handler_deadline,
                connect_permission,
                frame_deadline,
            )
            .await
            {
                tracing::warn!(
                    error = %error,
                    "handle connection failed"
                );
            }
        }
        .instrument(span),
    )
}

async fn handle(
    mut stream: TcpStream,
    cancellation_token: CancellationToken,
    storage: Arc<dyn Storage>,
    errors_registry: Arc<ErrorsRegistry>,
    transfer_semaphore: Arc<Semaphore>,
    handler_registry: Arc<HandlerRegistry>,
    executors_registry: Arc<ExecutorsRegistry>,
    queue_timeout: usize,
    transfer_timeout: usize,
    connection_timeout: usize,
    handler_deadline: usize,
    _connect_permission: OwnedSemaphorePermit,
    frame_deadline: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    const BUFFER_SIZE: usize = 4096;

    let mut buffer = BytesMut::with_capacity(BUFFER_SIZE);

    let mut connected_time = Instant::now();
    let mut deadline_time = connected_time + Duration::from_secs(frame_deadline as u64);

    loop {
        let now = Instant::now();

        if now >= deadline_time {
            handle_deadline(
                &mut stream,
                &errors_registry,
                frame_deadline,
                connection_timeout as u64,
            )
            .await?;

            return Ok(());
        }

        let idle_deadline = now + Duration::from_secs(connection_timeout as u64);
        let deadline = std::cmp::min(deadline_time, idle_deadline);

        let timeout = tokio::select! {
            _ = cancellation_token.cancelled() => {
                tracing::info!("Token is cancelled. Closing connection");
                return Ok(());
            },
            result = tokio::time::timeout_at(deadline, stream.read_buf(&mut buffer)) => {
                result
            },
        };

        let now = Instant::now();

        if now >= deadline_time {
            handle_deadline(
                &mut stream,
                &errors_registry,
                frame_deadline,
                connection_timeout as u64,
            )
            .await?;

            return Ok(());
        }

        let bytes_read = match timeout {
            Ok(result) => match result {
                Ok(bytes_read) => bytes_read,
                Err(error) => {
                    tracing::error!(error = %error,"Failed to read from client");

                    return Err(Box::new(error));
                }
            },
            Err(error) => {
                if deadline == idle_deadline {
                    tracing::error!(error = %error,
                    "Server couldn't read buffer in {} seconds", connection_timeout);

                    return Err(Box::new(error));
                } else {
                    handle_deadline(
                        &mut stream,
                        &errors_registry,
                        frame_deadline,
                        connection_timeout as u64,
                    )
                    .await?;

                    return Ok(());
                }
            }
        };

        if bytes_read == 0 {
            tracing::info!("Client disconnected");
            return Ok(());
        }

        loop {
            if cancellation_token.is_cancelled() {
                tracing::info!("Token is cancelled. Closing connection");
                return Ok(());
            }

            let result = Frame::decode(&mut buffer);

            match result {
                Ok(Some(frame)) => {
                    let context = ConnectionContext {
                        stream: &mut stream,
                        buffer: &mut buffer,
                        storage: &storage,
                        transfer_semaphore: Arc::clone(&transfer_semaphore),
                        errors_registry: &errors_registry,
                        queue_timeout,
                        transfer_timeout,
                        handler_deadline,
                    };

                    handle_frame(
                        context,
                        frame,
                        Arc::clone(&handler_registry),
                        Arc::clone(&executors_registry),
                        connection_timeout as u64,
                    )
                    .await?;

                    connected_time = Instant::now();
                    deadline_time = connected_time + Duration::from_secs(frame_deadline as u64);
                }
                Ok(None) => {
                    break;
                }
                Err(err) => {
                    handle_broken_frame(
                        &mut stream,
                        &errors_registry,
                        err,
                        connection_timeout as u64,
                    )
                    .await?;

                    return Ok(());
                }
            };
        }
    }
}

async fn handle_deadline(
    stream: &mut TcpStream,
    errors_registry: &Arc<ErrorsRegistry>,
    frame_deadline: usize,
    connection_timeout: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::warn!("Couldn't decode frame in {frame_deadline} seconds");

    let _ = send_control_response(
        stream,
        &response::encode_error(&build_error_or_unknown(
            errors_registry,
            ConnectionError::SlowConnection,
            ErrorType::Respond,
        )),
        Duration::from_secs(connection_timeout),
    )
    .await;

    stream.shutdown().await?;
    Ok(())
}

async fn handle_frame(
    mut context: ConnectionContext<'_>,
    frame: Frame,
    handler_registry: Arc<HandlerRegistry>,
    executors_registry: Arc<ExecutorsRegistry>,
    connection_timeout: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let opcode = frame.get_opcode();

    let handler = match handler_registry.get(&opcode) {
        Ok(handler) => handler,
        Err(error) => {
            tracing::warn!(error = %error, "Handler error");

            let response = response::encode_error(&build_error_or_unknown(
                context.errors_registry,
                error,
                ErrorType::Respond,
            ));

            send_control_response(
                context.stream,
                &response,
                Duration::from_secs(connection_timeout),
            )
            .await?;

            return Ok(());
        }
    };

    let executor = match executors_registry.get(&opcode) {
        Ok(executor) => executor,
        Err(error) => {
            tracing::warn!(error = %error, "Executor error");

            let response = response::encode_error(&build_error_or_unknown(
                context.errors_registry,
                error,
                ErrorType::Respond,
            ));

            send_control_response(
                context.stream,
                &response,
                Duration::from_secs(connection_timeout),
            )
            .await?;
            return Ok(());
        }
    };

    let handler_result = executor.execute(handler, frame, &mut context).await;

    if let Err(e) = handler_result {
        match e.error_type() {
            ErrorType::Respond => {
                tracing::error!(error = %e.value(), "send_response_from_frame failed. Creating response");

                send_control_response(
                    context.stream,
                    &response::encode_error(&e),
                    Duration::from_secs(connection_timeout),
                )
                .await?;
            }
            ErrorType::Fatal => {
                tracing::error!(error = %e.value(), "send_response_from_frame failed. Fatal Error");
                tracing::info!("Closing stream");

                context.stream.shutdown().await?;

                tracing::info!("stream closed succesfully");

                return Err("Fatal error. Closing stream".into());
            }
            ErrorType::RespondAndFatal => {
                tracing::error!(error = %e.value(), "send_response_from_frame failed. Creating response and closing connection.");

                send_control_response(
                    context.stream,
                    &response::encode_error(&e),
                    Duration::from_secs(connection_timeout),
                )
                .await?;

                tracing::info!("Closing stream");

                context.stream.shutdown().await?;

                tracing::info!("Stream closed succesfully");

                return Err("Fatal error. Closing stream".into());
            }
        }
    };
    Ok(())
}

async fn handle_broken_frame(
    stream: &mut TcpStream,
    errors_registry: &Arc<ErrorsRegistry>,
    err: crate::errors::ProtocolError,
    connection_timeout: u64,
) -> Result<(), Box<dyn std::error::Error + 'static>> {
    tracing::warn!(error = %err, "Protocol error");

    let error = build_error_or_unknown(errors_registry, err, ErrorType::Respond);
    let response = response::encode_error(&error);

    send_control_response(stream, &response, Duration::from_secs(connection_timeout)).await?;

    return Ok(());
}

async fn send_control_response(
    stream: &mut TcpStream,
    response: &[u8],
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    match tokio::time::timeout(timeout, stream.write_all(response)).await {
        Ok(result) => {
            result?;
            return Ok(());
        }
        Err(err) => {
            let timeout_secs = timeout.as_secs();
            tracing::warn!("Couldn't send control frame in {timeout_secs} seconds");

            return Err(Box::new(err));
        }
    };
}
