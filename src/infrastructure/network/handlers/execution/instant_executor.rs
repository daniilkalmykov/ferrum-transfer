use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    Error,
    infrastructure::network::{ConnectionContext, Frame, handlers::Handler},
};

use super::Executor;

pub struct InstantExecutor;

#[async_trait]
impl Executor for InstantExecutor {
    async fn execute(
        &self,
        handler: &Arc<dyn Handler>,
        frame: Frame,
        ctx: &mut ConnectionContext<'_>,
    ) -> Result<(), Error> {
        handler.handle(frame, ctx).await
    }
}
