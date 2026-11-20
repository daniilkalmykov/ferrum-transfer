use std::{sync::Arc, time::Duration};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{
    Error, ErrorType, ErrorsRegistry,
    errors::{ServerError, build_error_or_unknown},
};

pub async fn get_permission(
    semaphore: Arc<Semaphore>,
    queue_timeout: u64,
    errors_registry: &ErrorsRegistry,
) -> Result<OwnedSemaphorePermit, Error> {
    let timeout = tokio::time::timeout(
        Duration::from_secs(queue_timeout),
        semaphore.acquire_owned(),
    )
    .await;

    let permit = match timeout {
        Ok(result) => match result {
            Ok(permit) => permit,
            Err(_) => {
                return Err(build_error_or_unknown(
                    errors_registry,
                    ServerError::ServerNotWorking,
                    ErrorType::Respond,
                ));
            }
        },
        Err(err) => {
            tracing::error!(error = %err,
                "Server couldn't get permission in {} seconds",
                queue_timeout
            );

            return Err(build_error_or_unknown(
                errors_registry,
                ServerError::ServerBusy,
                ErrorType::Respond,
            ));
        }
    };

    Ok(permit)
}
