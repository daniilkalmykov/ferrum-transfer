use async_trait::async_trait;

use crate::{
    Error, Opcode,
    infrastructure::network::{ConnectionContext, Frame},
};

#[async_trait]
pub trait Handler: Send + Sync {
    fn opcode(&self) -> Opcode;

    async fn handle(&self, frame: Frame, ctx: &mut ConnectionContext<'_>) -> Result<(), Error>;
}
