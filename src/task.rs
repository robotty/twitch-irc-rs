use std::future::Future;
use tokio::task::JoinHandle;

/// Spawns a tokio task with a name.
/// This is useful in the tokio-console.
#[cfg(feature = "tokio-tracing")]
#[inline]
pub fn spawn_task<T>(name: &str, future: T) -> JoinHandle<T::Output>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    tokio::task::Builder::new().name(name).spawn(future)
}
#[cfg(not(feature = "tokio-tracing"))]
#[inline]
pub fn spawn_task<T>(_name: &str, future: T) -> JoinHandle<T::Output>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    tokio::spawn(future)
}
