pub mod error;
pub mod main_loop;

use crate::config::{ClientConfig, LoginCredentials};
use crate::connection::error::ConnErr;
use crate::connection::main_loop::{MainLoopCommand, MainLoopWorker};
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::Transport;
use futures::channel::{mpsc, oneshot};
use std::sync::Arc;

pub struct Connection<T: Transport<L>, L: LoginCredentials> {
    /// sends commands to the this connection's main event loop.
    main_loop_tx: mpsc::UnboundedSender<MainLoopCommand<T, L>>,
    /// provides the incoming messages. This is an `Option<>` so it can be taken ownership of using
    /// `.take()`
    pub connection_incoming_rx:
        Option<mpsc::UnboundedReceiver<Result<ServerMessage, ConnErr<T, L>>>>,
}

impl<T: Transport<L>, L: LoginCredentials> Connection<T, L> {
    pub fn new(config: Arc<ClientConfig<L>>) -> Connection<T, L> {
        let (main_loop_tx, main_loop_rx) = mpsc::unbounded();
        let (connection_incoming_tx, connection_incoming_rx) = mpsc::unbounded();

        MainLoopWorker::new(
            config,
            connection_incoming_tx,
            main_loop_tx.clone(),
            main_loop_rx,
        )
        .spawn();

        Connection {
            main_loop_tx,
            connection_incoming_rx: Some(connection_incoming_rx),
        }
    }

    pub async fn send_message(&mut self, message: IRCMessage) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        // unwrap: We don't expect the main loop to exit (and drop the Receiver) as long
        // as this Connection handle lives
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::SendMessage(message, Some(return_tx)))
            .unwrap();
        // unwrap: The main loop will always reply instead of dropping the sender.
        return_rx.await.unwrap()
    }

    pub async fn join(&mut self, channel_login: String) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Join(channel_login, return_tx))
            .unwrap();
        return_rx.await.unwrap()
    }

    pub async fn part(&mut self, channel_login: String) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Part(channel_login, return_tx))
            .unwrap();
        return_rx.await.unwrap()
    }

    pub async fn close(&mut self) {
        let (return_tx, return_rx) = oneshot::channel();
        // params for MainLoopCommand::Clone:
        // 1) optional reason error, 2) return channel
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Close(None, Some(return_tx)))
            .unwrap();
        return_rx.await.unwrap();
    }
}

impl<T: Transport<L>, L: LoginCredentials> Drop for Connection<T, L> {
    fn drop(&mut self) {
        // send the main Loop a Close command, so it ends itself as soon as every clone
        // of main_loop_tx is dropped

        // params for MainLoopCommand::Clone:
        // 1) optional reason error, 2) return channel
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Close(None, None))
            .unwrap();
    }
}
