use super::Handler;
use crate::{
    Error, ErrorType,
    errors::{ConnectionError, build_error_or_unknown},
    infrastructure::network::{ConnectionContext, Frame, Opcode, Status, response},
};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

pub struct Ping;

#[async_trait]
impl Handler for Ping {
    fn opcode(&self) -> Opcode {
        Opcode::Ping
    }

    async fn handle(&self, _frame: Frame, ctx: &mut ConnectionContext<'_>) -> Result<(), Error> {
        tracing::info!("PING received");

        let response = response::encode(Status::Ok, &[]);

        ctx.stream.write_all(&response).await.map_err(|e| {
            tracing::error!(error = %e, "handle_ping failed");

            build_error_or_unknown(
                ctx.errors_registry,
                ConnectionError::IOError(e),
                ErrorType::Fatal,
            )
        })?;

        Ok(())
    }
}
