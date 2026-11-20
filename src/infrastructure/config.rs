use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Config {
    server_host: String,
    server_port: u16,
    storage_path: PathBuf,
    log_path: PathBuf,
    max_concurrent_transfers: usize,
    queue_timeout: usize,
    transfer_timeout: usize,
    connection_timeout: usize,
    handler_deadline: usize,
    grace_period: usize,
    max_connections: usize,
    frame_deadline: usize,
}

impl Config {
    pub fn build_from_env() -> Result<Self, Box<dyn std::error::Error>> {
        const DEFAULT_SERVER_HOST: &str = "127.0.0.1";
        const DEFAULT_SERVER_PORT: u16 = 8080;
        const DEFAULT_STORAGE_PATH: &str = "./storage";
        const DEFAULT_LOG_PATH: &str = "./logs";
        const DEFAULT_MAX_CONCURRENT_TRANSFERS: usize = 4;
        const DEFAULT_QUEUE_TIMEOUT: usize = 5;
        const DEFAULT_TRANSFER_TIMEOUT: usize = 5;
        const DEFAULT_CONNECTION_TIMEOUT: usize = 5;
        const DEFAULT_HANDLER_DEADLINE: usize = 120;
        const DEFAULT_GRACE_PERIOD: usize = 30;
        const DEFAULT_MAX_CONNECTIONS: usize = 12;
        const DEFAULT_FRAME_DEADLINE: usize = 10;

        const SERVER_HOST_ENV_VAR: &str = "SERVER_HOST";
        const SERVER_PORT_ENV_VAR: &str = "SERVER_PORT";
        const STORAGE_PATH_ENV_VAR: &str = "STORAGE_PATH";
        const LOG_PATH_ENV_VAR: &str = "LOG_PATH";
        const MAX_CONCURRENT_TRANSFERS_ENV_VAR: &str = "MAX_CONCURRENT_TRANSFERS";
        const QUEUE_TIMEOUT_ENV_VAR: &str = "QUEUE_TIMEOUT";
        const TRANSFER_TIMEOUT_ENV_VAR: &str = "TRANSFER_TIMEOUT";
        const CONNECTION_TIMEOUT_ENV_VAR: &str = "CONNECTION_TIMEOUT";
        const HANDLER_DEADLINE_ENV_VAR: &str = "HANDLER_DEADLINE";
        const GRACE_PERIOD_ENV_VAR: &str = "GRACE_PERIOD";
        const MAX_CONNECTIONS_ENV_VAR: &str = "MAX_CONNECTIONS";
        const FRAME_DEADLINE_ENV_VAR: &str = "FRAME_DEADLINE";

        let server_host =
            std::env::var(SERVER_HOST_ENV_VAR).unwrap_or(DEFAULT_SERVER_HOST.to_string());

        let server_port = if let Ok(port) = std::env::var(SERVER_PORT_ENV_VAR) {
            port.parse()?
        } else {
            DEFAULT_SERVER_PORT
        };

        let storage_path = std::env::var(STORAGE_PATH_ENV_VAR)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_STORAGE_PATH));

        let log_path = std::env::var(LOG_PATH_ENV_VAR)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_LOG_PATH));

        let default_transfers_range = 1..10;

        let max_concurrent_transfers =
            if let Ok(max_concurrent_transfers) = std::env::var(MAX_CONCURRENT_TRANSFERS_ENV_VAR) {
                let transfers = max_concurrent_transfers.parse()?;

                if default_transfers_range.contains(&transfers) == false {
                    DEFAULT_MAX_CONCURRENT_TRANSFERS
                } else {
                    transfers
                }
            } else {
                DEFAULT_MAX_CONCURRENT_TRANSFERS
            };

        let default_timeout_range = 1..30;

        let queue_timeout = if let Ok(queue_timeout) = std::env::var(QUEUE_TIMEOUT_ENV_VAR) {
            let timeout = queue_timeout.parse()?;

            if default_timeout_range.contains(&timeout) == false {
                DEFAULT_QUEUE_TIMEOUT
            } else {
                timeout
            }
        } else {
            DEFAULT_QUEUE_TIMEOUT
        };

        let transfer_timeout = if let Ok(queue_timeout) = std::env::var(TRANSFER_TIMEOUT_ENV_VAR) {
            let timeout = queue_timeout.parse()?;

            if default_timeout_range.contains(&timeout) == false {
                DEFAULT_TRANSFER_TIMEOUT
            } else {
                timeout
            }
        } else {
            DEFAULT_TRANSFER_TIMEOUT
        };

        let connection_timeout =
            if let Ok(queue_timeout) = std::env::var(CONNECTION_TIMEOUT_ENV_VAR) {
                let timeout = queue_timeout.parse()?;

                if default_timeout_range.contains(&timeout) == false {
                    DEFAULT_CONNECTION_TIMEOUT
                } else {
                    timeout
                }
            } else {
                DEFAULT_CONNECTION_TIMEOUT
            };

        let handler_deadline = if let Ok(handler_deadline) = std::env::var(HANDLER_DEADLINE_ENV_VAR)
        {
            let deadline = handler_deadline.parse()?;

            if (60..=180).contains(&deadline) == false {
                DEFAULT_HANDLER_DEADLINE
            } else {
                deadline
            }
        } else {
            DEFAULT_HANDLER_DEADLINE
        };

        let grace_period = if let Ok(grace_period) = std::env::var(GRACE_PERIOD_ENV_VAR) {
            let period = grace_period.parse()?;

            if (20..=100).contains(&period) == false {
                DEFAULT_GRACE_PERIOD
            } else {
                period
            }
        } else {
            DEFAULT_GRACE_PERIOD
        };

        let max_connections = if let Ok(max_connections) = std::env::var(MAX_CONNECTIONS_ENV_VAR) {
            let connections = max_connections.parse()?;

            if (4..=20).contains(&connections) == false {
                DEFAULT_MAX_CONNECTIONS
            } else {
                connections
            }
        } else {
            DEFAULT_MAX_CONNECTIONS
        };

        let frame_deadline = if let Ok(frame_deadline) = std::env::var(FRAME_DEADLINE_ENV_VAR) {
            let deadline = frame_deadline.parse()?;

            if (1..=30).contains(&deadline) == false {
                DEFAULT_FRAME_DEADLINE
            } else {
                deadline
            }
        } else {
            DEFAULT_FRAME_DEADLINE
        };

        Ok(Self {
            server_host,
            server_port,
            storage_path,
            log_path,
            max_concurrent_transfers,
            queue_timeout,
            transfer_timeout,
            connection_timeout,
            handler_deadline,
            grace_period,
            max_connections,
            frame_deadline,
        })
    }

    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }

    pub fn server_host(&self) -> &str {
        &self.server_host
    }

    pub fn server_port(&self) -> u16 {
        self.server_port
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    pub fn max_concurrent_transfers(&self) -> usize {
        self.max_concurrent_transfers
    }

    pub fn queue_timeout(&self) -> usize {
        self.queue_timeout
    }

    pub fn transfer_timeout(&self) -> usize {
        self.transfer_timeout
    }

    pub fn connection_timeout(&self) -> usize {
        self.connection_timeout
    }

    pub fn handler_deadline(&self) -> usize {
        self.handler_deadline
    }

    pub fn grace_period(&self) -> usize {
        self.grace_period
    }

    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    pub fn frame_deadline(&self) -> usize {
        self.frame_deadline
    }
}
