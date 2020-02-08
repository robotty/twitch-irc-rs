use crate::client::config::{ClientConfig, LoginCredentials};
use crate::client::connection::Connection;
use crate::client::operations::LoginError;
use crate::client::transport::Transport;
use crate::message::IRCMessage;
use futures::channel::mpsc::{channel, Receiver, Sender};
use futures::future::Shared;
use futures::prelude::*;
use std::collections::VecDeque;
use std::fmt::{Debug, Display};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Error, Debug)]
pub enum ConnectionInitError<TC, L, TO>
where
    TC: Display + Debug,
    L: Display + Debug,
    TO: Display + Debug,
{
    #[error("{0}")]
    TransportConnectError(TC),
    #[error("{0}")]
    CredentialsError(L),
    #[error("{0}")]
    TransportOutgoingError(TO),
}

impl<TC, L, TO> From<LoginError<L, TO>> for ConnectionInitError<TC, L, TO>
where
    TC: Display + Debug,
    L: Display + Debug,
    TO: Display + Debug,
{
    fn from(e: LoginError<L, TO>) -> Self {
        match e {
            LoginError::CredentialsError(inner) => ConnectionInitError::CredentialsError(inner),
            LoginError::TransportOutgoingError(inner) => {
                ConnectionInitError::TransportOutgoingError(inner)
            }
        }
    }
}

type ConnectionFut<T, L> = Shared<
    Pin<
        Box<
            dyn Future<
                    Output = Result<
                        Arc<Connection<T, L>>,
                        Arc<
                            ConnectionInitError<
                                <T as Transport>::ConnectError,
                                <L as LoginCredentials>::Error,
                                <T as Transport>::OutgoingError,
                            >,
                        >,
                    >,
                > + Send,
        >,
    >,
>;

pub struct ConnectionPool<T: Transport, L: LoginCredentials> {
    pub connections: Mutex<VecDeque<ConnectionFut<T, L>>>,

    pub incoming_messages: Option<Receiver<Result<IRCMessage, T::IncomingError>>>,
    incoming_messages_sender: Sender<Result<IRCMessage, T::IncomingError>>,

    pub config: Arc<ClientConfig<L>>,
}

impl<T: Transport, L: LoginCredentials> ConnectionPool<T, L> {
    pub fn new(config: Arc<ClientConfig<L>>) -> ConnectionPool<T, L> {
        let (incoming_messages_sender, incoming_messages_receiver) = channel(16);
        ConnectionPool {
            connections: Mutex::new(VecDeque::new()),
            incoming_messages: Some(incoming_messages_receiver),
            incoming_messages_sender,
            config,
        }
    }

    fn new_connection(&self) -> ConnectionFut<T, L> {
        let mut own_sender = Sender::clone(&self.incoming_messages_sender);
        let own_config = Arc::clone(&self.config);

        async move {
            let new_transport = T::new()
                .await
                .map_err(|e| Arc::new(ConnectionInitError::TransportConnectError(e)))?;
            let conn = Connection::new(new_transport, own_config);

            // forward incoming messages
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

            conn.initialize().await.map_err(|e| Arc::new(e.into()))?;

            Ok(Arc::new(conn))
        }
        // the BoxFut is so we can use "dyn Future"
        .boxed()
        .shared()
    }

    pub async fn checkout_connection(
        &self,
    ) -> Result<
        Arc<Connection<T, L>>,
        Arc<ConnectionInitError<T::ConnectError, L::Error, T::OutgoingError>>,
    > {
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
