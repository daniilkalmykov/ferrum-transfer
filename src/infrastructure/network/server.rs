use std::sync::{Arc, atomic::AtomicU64};
use std::time::Duration;

use tokio::signal;
use tokio::task::JoinHandle;
use tokio::{net::TcpListener, sync::Semaphore};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::errors::StorageError;
use crate::{Config, ExecutorsRegistry, HandlerRegistry, Storage};
use crate::{errors::ErrorsRegistry, infrastructure::network::connection};

pub struct Server {
    storage: Arc<dyn Storage>,
    errors_registry: Arc<ErrorsRegistry>,
    handlers_registry: Arc<HandlerRegistry>,
    executors_registry: Arc<ExecutorsRegistry>,
}

impl Server {
    pub async fn new<T: Storage + 'static>(
        storage: T,
        errors_registry: ErrorsRegistry,
        handlers_registry: HandlerRegistry,
        exeutors_registry: ExecutorsRegistry,
    ) -> Result<Self, StorageError> {
        let server = Self {
            storage: Arc::new(storage),
            errors_registry: Arc::new(errors_registry),
            handlers_registry: Arc::new(handlers_registry),
            executors_registry: Arc::new(exeutors_registry),
        };

        server.storage.clean_temp_files().await?;

        Ok(server)
    }

    pub async fn run(&self, config: &Config) -> Result<(), std::io::Error> {
        let mut task_tracker = TaskTracker::new();
        let cancellation_token = CancellationToken::new();

        let mut handles = Vec::new();

        tokio::select! {
            result = signal::ctrl_c() => {
                tracing::info!("Server stopped working. Closing connections");

                cancellation_token.cancel();
                task_tracker.close();

                let timeout = tokio::time::timeout(Duration::from_secs(config.grace_period() as u64), task_tracker.wait()).await;

                match timeout {
                    Ok(_) => {
                        tracing::info!("All tasks completed during grace period");
                    },
                    Err(err) => {
                        let unmet_tasks = task_tracker.len();
                        tracing::error!(error = %err, "Timeout! There are {unmet_tasks} unmet tasks.");

                        for handle in handles {
                            handle.abort();
                        }

                         task_tracker.wait().await;
                    },
                }

                result
            },
            result = self.start_listening(config, &mut task_tracker, cancellation_token.clone(), &mut handles) => {
                result
            },
        }
    }

    async fn start_listening(
        &self,
        config: &Config,
        task_tracker: &mut TaskTracker,
        cancellation_token: CancellationToken,
        handles: &mut Vec<JoinHandle<()>>,
    ) -> Result<(), std::io::Error> {
        let transfer_semaphore = Arc::new(Semaphore::new(config.max_concurrent_transfers()));
        let connection_semaphore = Arc::new(Semaphore::new(config.max_connections()));

        let connection_id = AtomicU64::new(1);

        tracing::info!(
            host = config.server_host(),
            port = config.server_port(),
            "Server is starting"
        );

        let listener =
            match TcpListener::bind(format!("{}:{}", config.server_host(), config.server_port()))
                .await
            {
                Ok(it) => it,
                Err(err) => {
                    tracing::error!(error = %err,
                        server_host = config.server_host(),
                        server_port = config.server_port(),
                        "Failed to bind");

                    return Err(err);
                }
            };

        tracing::info!(
            host = config.server_host(),
            port = config.server_port(),
            "Server listening"
        );

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let id = connection_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    tracing::info!(connection_id = id, peer_addr = %addr, "Connection accepted");

                    let storage = Arc::clone(&self.storage);
                    let errors_registry = Arc::clone(&self.errors_registry);
                    let transfer_semaphore = Arc::clone(&transfer_semaphore);
                    let handlers_registry = Arc::clone(&self.handlers_registry);
                    let executors_registry = Arc::clone(&self.executors_registry);
                    let queue_timeout = config.queue_timeout();
                    let transfer_timeout = config.transfer_timeout();
                    let connection_timeout = config.connection_timeout();
                    let handler_deadline = config.handler_deadline();
                    let connection_semaphore = Arc::clone(&connection_semaphore);
                    let frame_deadline = config.frame_deadline();

                    if let Ok(permit) = connection_semaphore.try_acquire_owned() {
                        let join_handle = connection::spawn(
                            cancellation_token.clone(),
                            task_tracker,
                            id,
                            storage,
                            errors_registry,
                            transfer_semaphore,
                            stream,
                            addr,
                            handlers_registry,
                            executors_registry,
                            queue_timeout,
                            transfer_timeout,
                            connection_timeout,
                            handler_deadline,
                            permit,
                            frame_deadline,
                        );

                        handles.retain(|handle| handle.is_finished() == false);
                        handles.push(join_handle);
                    } else {
                        tracing::debug!("Connection aborted. No more free connections");
                    }
                }
                Err(err) => {
                    tracing::error!(error = %err, "Failed to accept connection");
                }
            }
        }
    }
}
