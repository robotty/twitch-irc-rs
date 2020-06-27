pub mod error;
pub mod event_loop;

use crate::config::{ClientConfig, LoginCredentials};
use crate::connection::error::ConnErr;
use crate::connection::event_loop::{ConnectionLoopCommand, ConnectionLoopWorker};
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::Transport;
use futures::channel::{mpsc, oneshot};
use std::sync::Arc;

pub struct Connection<T: Transport<L>, L: LoginCredentials> {
    /// sends commands to the this connection's event loop.
    connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
    /// provides the incoming messages. This is an `Option<>` so it can be taken ownership of using
    /// `.take()`
    pub connection_incoming_rx:
        Option<mpsc::UnboundedReceiver<Result<ServerMessage, ConnErr<T, L>>>>,
}

impl<T: Transport<L>, L: LoginCredentials> Connection<T, L> {
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
            connection_incoming_rx: Some(connection_incoming_rx),
        }
    }

    pub async fn send_message(&mut self, message: IRCMessage) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        // unwrap: We don't expect the connection loop to exit (and drop the Receiver) as long
        // as this Connection handle lives
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::SendMessage(message, Some(return_tx)))
            .unwrap();
        // unwrap: The connection loop will always reply instead of dropping the sender.
        return_rx.await.unwrap()
    }

    pub async fn join(&mut self, channel_login: String) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Join(channel_login, return_tx))
            .unwrap();
        return_rx.await.unwrap()
    }

    pub async fn part(&mut self, channel_login: String) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Part(channel_login, return_tx))
            .unwrap();
        return_rx.await.unwrap()
    }

    pub async fn close(&mut self) {
        let (return_tx, return_rx) = oneshot::channel();
        // params for ConnectionLoopCommand::Clone:
        // 1) optional reason error, 2) return channel
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Close(None, Some(return_tx)))
            .unwrap();
        return_rx.await.unwrap();
    }
}

impl<T: Transport<L>, L: LoginCredentials> Drop for Connection<T, L> {
    fn drop(&mut self) {
        // send the connection loop a Close command, so it ends itself as soon as every clone
        // of connection_loop_tx is dropped

        // params for ConnectionLoopCommand::Clone:
        // 1) optional reason error, 2) return channel
        self.connection_loop_tx
            .unbounded_send(ConnectionLoopCommand::Close(None, None))
            .unwrap();
    }
}
