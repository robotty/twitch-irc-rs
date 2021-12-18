/// Spawns a tokio task with a name.
/// This is useful in the tokio-console.
#[macro_export]
macro_rules! spawn_task {
    ($name:literal, $task:expr) => {
        if cfg!(feature = "tokio-tracing") {
            tokio::task::Builder::new().name($name).spawn($task)
        } else {
            tokio::spawn($task)
        }
    };
}
