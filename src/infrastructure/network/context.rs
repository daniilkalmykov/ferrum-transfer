use crate::{ErrorsRegistry, Storage};

use bytes::BytesMut;
use std::sync::Arc;
use tokio::{net::TcpStream, sync::Semaphore};

pub struct ConnectionContext<'a> {
    pub stream: &'a mut TcpStream,
    pub buffer: &'a mut BytesMut,
    pub storage: &'a Arc<dyn Storage>,
    pub transfer_semaphore: Arc<Semaphore>,
    pub errors_registry: &'a Arc<ErrorsRegistry>,
    pub queue_timeout: usize,
    pub transfer_timeout: usize,
    pub handler_deadline: usize,
}
