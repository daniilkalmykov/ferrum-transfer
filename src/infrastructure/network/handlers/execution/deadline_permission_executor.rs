use crate::{
    Error, ErrorType,
    errors::{ServerError, build_error_or_unknown},
    infrastructure::network::{
        ConnectionContext, Frame, get_permission,
        handlers::{Handler, execution::Executor},
    },
};
use async_trait::async_trait;
use std::{sync::Arc, time::Duration};

pub struct DeadlinePermissionExecutor;

#[async_trait]
impl Executor for DeadlinePermissionExecutor {
    async fn execute(
        &self,
        handler: &Arc<dyn Handler>,
        frame: Frame,
        ctx: &mut ConnectionContext<'_>,
    ) -> Result<(), Error> {
        let _permit = get_permission(
            Arc::clone(&ctx.transfer_semaphore),
            ctx.queue_timeout as u64,
            ctx.errors_registry,
        )
        .await?;

        let deadline = tokio::time::timeout(
            Duration::from_secs(ctx.handler_deadline as u64),
            handler.handle(frame, ctx),
        )
        .await;

        match deadline {
            Ok(result) => {
                return result;
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "Couldn't handle the request in {} seconds",
                    ctx.handler_deadline
                );

                return Err(build_error_or_unknown(
                    ctx.errors_registry,
                    ServerError::Deadline,
                    ErrorType::Fatal,
                ));
            }
        }
    }
}
