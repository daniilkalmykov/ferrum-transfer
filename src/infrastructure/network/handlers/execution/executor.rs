use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    Error,
    infrastructure::network::{ConnectionContext, Frame, handlers::Handler},
};

#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(
        &self,
        handler: &Arc<dyn Handler>,
        frame: Frame,
        ctx: &mut ConnectionContext<'_>,
    ) -> Result<(), Error>;
}
