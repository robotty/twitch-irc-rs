pub mod error;
pub mod event_loop;

use crate::config::ClientConfig;
use crate::connection::error::ConnectionError;
use crate::connection::event_loop::{ConnectionLoopCommand, ConnectionLoopWorker};
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::Transport;
use futures::channel::{mpsc, oneshot};
use std::collections::HashSet;
use std::sync::Arc;

pub struct Connection<T: Transport, L: LoginCredentials> {
    /// sends commands to the this connection's event loop.
    connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
    /// provides the incoming messages. This is an `Option<>` so it can be taken ownership of using
    /// `.take()`. The received error type holds the error, and since the stream (and the connection)
    /// will end after this last message, it also contains the list of channels that were
    /// joined at the very moment that the client closed, for the purposes of re-joining those
    /// channels.
    pub incoming_messages: Option<
        mpsc::UnboundedReceiver<Result<ServerMessage, (ConnectionError<T, L>, HashSet<String>)>>,
    >,
}

impl<T: Transport, L: LoginCredentials> Connection<T, L> {
    pub fn new(config: Arc<ClientConfig<L>>) -> Connection<T, L> {
        let (connection_loop_tx, connection_loop_rx) = mpsc::unbounded();
        let (connection_incoming_tx, connection_incoming_rx) = mpsc::unbounded();

        ConnectionLoopWorker::new(
            config,
            connection_incoming_tx,
            connection_loop_tx.clone(),
            connection_loop_rx,
        )
        .spawn();

        Connection {
            connection_loop_tx,
            incoming_messages: Some(connection_incoming_rx),
        }
    }

    pub async fn send_message(&self, message: IRCMessage) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        // unwrap: We don't expect the connection loop to exit (and drop the Receiver) as long
        // as this Connection handle lives
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::SendMessage(message, Some(return_tx)))
            .unwrap();
        // unwrap: The connection loop will always reply instead of dropping the sender.
        return_rx.await.unwrap()
    }

    pub async fn join(&self, channel_login: String) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Join(channel_login, return_tx))
            .unwrap();
        return_rx.await.unwrap()
    }

    pub async fn part(&self, channel_login: String) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Part(channel_login, return_tx))
            .unwrap();
        return_rx.await.unwrap()
    }

    pub async fn close(&self) {
        let (return_tx, return_rx) = oneshot::channel();
        // params for ConnectionLoopCommand::Close:
        // 1) optional reason error, 2) return channel
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Close(None, Some(return_tx)))
            .unwrap();
        return_rx.await.unwrap();
    }
}

impl<T: Transport, L: LoginCredentials> Drop for Connection<T, L> {
    fn drop(&mut self) {
        // send the connection loop a Close command, so it ends itself as soon as every clone
        // of connection_loop_tx is dropped

        // params for ConnectionLoopCommand::Close:
        // 1) optional reason error, 2) return channel
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Close(None, None))
            .unwrap();
    }
}
