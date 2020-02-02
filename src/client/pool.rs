use super::transport::Transport;
use crate::client::transport::TCPTransport;
use crate::message::IRCMessage;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::future::Shared;
use futures::prelude::*;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Connection<T: Transport> {
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

type ConnectionFut<T> = Shared<
    Pin<
        Box<dyn Future<Output = Arc<Result<Connection<T>, <T as Transport>::ConnectError>>> + Send>,
    >,
>;

pub struct ConnectionPool<T: Transport> {
    pub connections: Mutex<VecDeque<ConnectionFut<T>>>,

    pub incoming_messages: Receiver<Result<IRCMessage, T::IncomingError>>,
    incoming_messages_sender: Sender<Result<IRCMessage, T::IncomingError>>,
}

impl<T: Transport> ConnectionPool<T> {
    pub fn new() -> ConnectionPool<T> {
        let (incoming_messages_sender, incoming_messages_receiver) = channel(16);
        ConnectionPool {
            connections: Mutex::new(VecDeque::new()),
            incoming_messages: incoming_messages_receiver,
            incoming_messages_sender,
        }
    }

    fn new_connection(&self) -> ConnectionFut<T> {
        let mut own_sender = Sender::clone(&self.incoming_messages_sender);
        async move {
            // TODO: once try blocks stabilize, replace this async{}.await with a try{} block
            let res = async {
                let new_transport = T::new().await?;
                let conn = Connection::from(new_transport);

                let own_incoming_messages = Arc::clone(&conn.incoming_messages);
                tokio::spawn(async move {
                    let mut incoming_messages = own_incoming_messages.lock().await;
                    while let Some(message) = incoming_messages.next().await {
                        let res = own_sender.send(message).await;
                        if let Err(send_error) = res {
                            if send_error.is_disconnected() {
                                break;
                            } else {
                                panic!("unexpected send error")
                            }
                        }
                    }
                });

                Ok(conn)
            }
            .await;

            // The Arc<> wrapper is so the Shared<> can clone the result
            Arc::new(res)
        }
        // the BoxFut is so we can use "dyn Future"
        .boxed()
        .shared()
    }

    pub async fn checkout_connection(&self) -> Arc<Result<Connection<T>, T::ConnectError>> {
        // TODO: maybe a std::sync::Mutex performs better here since there is no .await inside the critical section (short critical section)?
        let mut connections = self.connections.lock().await;

        // TODO: if logic for picking a connection by some condition is required, do it here
        let maybe_conn_fut = connections.pop_front();

        // if we got None, then make a new connection (unwrap_or_else)
        let conn_fut = maybe_conn_fut.unwrap_or_else(|| self.new_connection());

        connections.push_back(Shared::clone(&conn_fut));

        drop(connections); // unlock mutex

        conn_fut.await
    }
}

fn main() {
    let _conn_pool: ConnectionPool<TCPTransport> = ConnectionPool::new();
}
