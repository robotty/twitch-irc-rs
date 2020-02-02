use super::transport::Transport;
use futures::future::Shared;
use futures::prelude::*;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

struct Connection<T>
where
    T: Transport,
{
    incoming_messages: Arc<Mutex<T::Incoming>>,
    outgoing_messages: Arc<Mutex<T::Outgoing>>,
    pub channels: Vec<String>,
}

impl<T: Transport> From<T> for Connection<T> {
    fn from(transport: T) -> Self {
        // destructure the given transport...
        let (incoming_messages, outgoing_messages) = transport.split();

        // and build a Connection from the parts
        Connection {
            incoming_messages: Arc::new(Mutex::new(incoming_messages)),
            outgoing_messages: Arc::new(Mutex::new(outgoing_messages)),
            channels: vec![],
        }
    }
}

struct ConnectionPool<T: Transport> {
    pub connections: Mutex<
        VecDeque<
            Shared<
                Pin<Box<dyn Future<Output = Arc<Result<Connection<T>, T::ConnectError>>> + Send>>,
            >,
        >,
    >,
}

impl<T: Transport> ConnectionPool<T> {
    pub fn new() -> ConnectionPool<T> {
        ConnectionPool {
            connections: Mutex::new(VecDeque::new()),
        }
    }

    pub async fn checkout_connection(&self) -> Arc<Result<Connection<T>, T::ConnectError>> {
        // TODO: maybe a std::sync::Mutex performs better here since there is no .await inside the critical section?
        let mut connections = self.connections.lock().await;

        // TODO: if logic for picking a connection by some condition is required, do it here
        let maybe_conn_fut = connections.pop_front();

        // if we got None, then make a new connection (unwrap_or_else)
        let conn_fut = maybe_conn_fut.unwrap_or_else(|| {
            async {
                let res = async {
                    let new_transport = T::new().await?;
                    let conn = Connection::from(new_transport);
                    Ok::<Connection<T>, T::ConnectError>(conn)
                }
                .await;
                Arc::new(res)
            }
            .boxed()
            .shared()
        });

        connections.push_back(Shared::clone(&conn_fut));

        drop(connections); // unlock mutex

        conn_fut.await
    }
}
