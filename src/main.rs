use ferrum_transfer::{
    Config, DeadlinePermissionExecutor, Download, ErrorsRegistry, ExecutorsRegistry, FileStorage,
    HandlerRegistry, InstantExecutor, Opcode, Ping, Server, Upload,
    errors::{
        ConnectionError, DefaultError, ExecutorError, HandlerError, ProtocolError, ServerError,
        StorageError,
    },
    logging,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut errors_registry = ErrorsRegistry::new();

    if !errors_registry.register::<ProtocolError>(100) {
        return Err("Failed to register ProtocolError".into());
    }

    if !errors_registry.register::<StorageError>(200) {
        return Err("Failed to register StorageError".into());
    }

    if !errors_registry.register::<ServerError>(300) {
        return Err("Failed to register ServerError".into());
    }

    if !errors_registry.register::<HandlerError>(400) {
        return Err("Failed to register HandlerError".into());
    }

    if !errors_registry.register::<ExecutorError>(500) {
        return Err("Failed to register ExecutorError".into());
    }

    if !errors_registry.register::<ConnectionError>(600) {
        return Err("Failed to register ConnectionError".into());
    }

    if !errors_registry.register::<DefaultError>(900) {
        return Err("Failed to register DefaultError".into());
    }

    let handlers_registry = HandlerRegistry::new()
        .register(Ping)?
        .register(Upload)?
        .register(Download)?;

    let executors_registry = ExecutorsRegistry::new()
        .register(InstantExecutor, Opcode::Ping)?
        .register(DeadlinePermissionExecutor, Opcode::Upload)?
        .register(DeadlinePermissionExecutor, Opcode::Download)?;

    let config = Config::build_from_env()?;

    let _guard = logging::init(&config)?;
    let storage = FileStorage::new(&config).await?;

    let server = Server::new(
        storage,
        errors_registry,
        handlers_registry,
        executors_registry,
    )
    .await?;

    server.run(&config).await?;

    Ok(())
}
