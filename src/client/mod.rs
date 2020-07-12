mod event_loop;
mod pool_connection;

use crate::client::event_loop::{ClientLoopCommand, ClientLoopWorker};
use crate::config::ClientConfig;
use crate::connection::error::ConnectionError;
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::Transport;
use futures::channel::{mpsc, oneshot};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TwitchIRCClient<T: Transport, L: LoginCredentials> {
    client_loop_tx: mpsc::UnboundedSender<ClientLoopCommand<T, L>>,
}

impl<T: Transport, L: LoginCredentials> TwitchIRCClient<T, L> {
    pub fn new(
        config: ClientConfig<L>,
    ) -> (
        mpsc::UnboundedReceiver<ServerMessage>,
        TwitchIRCClient<T, L>,
    ) {
        let config = Arc::new(config);
        let (client_loop_tx, client_loop_rx) = mpsc::unbounded();
        let (client_incoming_messages_tx, client_incoming_messages_rx) = mpsc::unbounded();

        ClientLoopWorker::new(
            config,
            client_loop_tx.clone(),
            client_loop_rx,
            client_incoming_messages_tx,
        )
        .spawn();

        (
            client_incoming_messages_rx,
            TwitchIRCClient { client_loop_tx },
        )
    }
}

impl<T: Transport, L: LoginCredentials> TwitchIRCClient<T, L> {
    pub async fn connect(&self) {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Connect {
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn send_message(&self, message: IRCMessage) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::SendMessage {
                message,
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn privmsg(
        &self,
        channel_login: String,
        message: String,
    ) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Privmsg {
                channel_login,
                message,
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn join(&self, channel_login: String) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Join {
                channel_login,
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn part(&self, channel_login: String) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Part {
                channel_login,
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn ping(&self) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Ping {
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn close(&self) {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Close {
                return_sender: Some(return_tx),
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }
}

impl<T: Transport, L: LoginCredentials> Drop for TwitchIRCClient<T, L> {
    fn drop(&mut self) {
        self.client_loop_tx
            .unbounded_send(ClientLoopCommand::Close {
                return_sender: None,
            })
            .unwrap();
    }
}
