mod event_loop;
mod pool_connection;

use crate::client::event_loop::{ClientLoopCommand, ClientLoopWorker};
use crate::config::ClientConfig;
use crate::connection::error::ConnectionError;
use crate::irc;
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::Transport;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

/// A send-only handle to control the Twitch IRC Client.
#[derive(Debug)]
pub struct TwitchIRCClient<T: Transport, L: LoginCredentials> {
    // we use an Arc<>.
    // the client loop has to also hold a handle to this sender to be able to feed itself
    // with commands as well. (e.g. to rejoin channels)
    // the client loop gets a Weak<> (a weak reference) and this client holds strong
    // references. That means when the last client handle is dropped, the client loop
    // exits, because the underlying mpsc::UnboundedSender will be dropped.
    // The client will then also no longer be able to send "itself" messages, because
    // it always only holds a Weak<> and has to check whether the weak reference is still
    // valid before sending itself messages.
    client_loop_tx: Arc<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
}

// we have to implement Debug and Clone manually, the derive macro places
// the requirement `T: Clone` which we cannot currently satisfy and don't need
impl<T: Transport, L: LoginCredentials> Clone for TwitchIRCClient<T, L> {
    fn clone(&self) -> Self {
        TwitchIRCClient {
            client_loop_tx: self.client_loop_tx.clone(),
        }
    }
}

impl<T: Transport, L: LoginCredentials> TwitchIRCClient<T, L> {
    pub fn new(
        config: ClientConfig<L>,
    ) -> (
        mpsc::UnboundedReceiver<ServerMessage>,
        TwitchIRCClient<T, L>,
    ) {
        let config = Arc::new(config);
        let (client_loop_tx, client_loop_rx) = mpsc::unbounded_channel();
        let client_loop_tx = Arc::new(client_loop_tx);
        let (client_incoming_messages_tx, client_incoming_messages_rx) = mpsc::unbounded_channel();

        ClientLoopWorker::spawn(
            config,
            // the worker gets only a weak reference
            Arc::downgrade(&client_loop_tx),
            client_loop_rx,
            client_incoming_messages_tx,
        );

        (
            client_incoming_messages_rx,
            TwitchIRCClient { client_loop_tx },
        )
    }
}

impl<T: Transport, L: LoginCredentials> TwitchIRCClient<T, L> {
    /// Connect to Twitch IRC without joining any channels.
    ///
    /// **You typically do not need to call this method.** This is only provided for the rare
    /// case that one would only want to receive incoming whispers without joining channels
    /// or ever sending messages out. If your application joins channels during startup,
    /// calling `.connect()` is superfluous.
    ///
    /// The client will automatically open the necessary connections when you join channels
    /// or send messages.
    pub async fn connect(&self) {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .send(ClientLoopCommand::Connect {
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    pub async fn send_message(&self, message: IRCMessage) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .send(ClientLoopCommand::SendMessage {
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
        self.send_message(irc!["PRIVMSG", format!("#{}", channel_login), message])
            .await
    }

    pub async fn say(
        &self,
        channel_login: String,
        message: String,
    ) -> Result<(), ConnectionError<T, L>> {
        // The prefixed "." prevents execution of commands
        self.privmsg(channel_login, format!(". {}", message)).await
    }

    /// Join the given channel. (When a channel is joined, the client will receive messages
    /// sent to it)
    ///
    /// The client will internally ensure that this channel is always joined.
    ///
    /// However this does not necessarily mean the join is always successful on an
    /// "application level". If the given `channel_login` does not exist then the IRC server
    /// will ignore the `JOIN` and you will not be joined to the given channel (what channel would
    /// you even expect to join if the channel does not exist?).
    ///
    /// However, the client listens for a server-side confirmation to this `JOIN` command.
    /// If the server confirms that the `JOIN` was successful, then the client saves this information.
    /// This information can be queried using `get_channel_status()`.
    ///
    /// If you later issue another `join()` call, and the server previously confirmed the successful
    /// joining of `channel_login`, then no message will be sent out.
    ///
    /// However if the server *did not* confirm the successful `JOIN` command previously, then the
    /// `JOIN` is attempted again.
    ///
    /// You can use this mechanism to e.g. periodically re-try `JOIN`ing a given channel if
    /// joining to freshly created channels or freshly renamed channels is a concern in your application.
    ///
    /// Another note on Twitch behaviour: If a channel gets suspended, the `JOIN` membership stays
    /// active as long as the connection with that `JOIN` membership stays active. For this reason,
    /// there is no special logic or handling required for when a channel gets suspended.
    /// (The `JOIN` membership in that channel will continue to count as confirmed for as long
    /// as the connection stays alive. If the connection fails, the "confirmed" status for that
    /// channel is reset, and the client will automatically attempt to re-join that channel on a
    /// different or new connection.
    /// Unless an answer is again received by the server, the `join()` will then make attempts again
    /// to join that channel.
    pub fn join(&self, channel_login: String) {
        self.client_loop_tx
            .send(ClientLoopCommand::Join { channel_login })
            .unwrap();
    }

    /// Query the client for what status a certain channel is in.
    ///
    /// Returns two booleans: The first indicates wheter a channel is `wanted`. This is true
    /// if the last operation for this channel was a `join()` method.
    ///
    /// The second boolean indicates whether this channel is currently joined server-side.
    /// (This is purely based on `JOIN` and `PART` messages being received from the server).
    ///
    /// Note that any combination of `true` and `false` is possible here.
    ///
    /// For example, `(true, false)` could indicate that the `JOIN` message to join this channel is currently
    /// being sent or already sent, but no response confirming the `JOIN` has been received yet.
    /// **Note this status can also mean that the server did not answer the `JOIN` request because
    /// the channel did not exist/was suspended or similar conditions.**
    ///
    /// `(false, true)` might on the other hand (similarly) that a `PART` message is sent but not
    /// answered yet by the server.
    ///
    /// `(true, true)` confirms that the channel is currently successfully joined in a normal fashion.
    ///
    /// `(false, false)` is returned for a channel that has not been joined previously at all
    /// or where a previous `PART` command has completed.
    pub async fn get_channel_status(&self, channel_login: String) -> (bool, bool) {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .send(ClientLoopCommand::GetChannelStatus {
                channel_login,
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }

    /// Part (leave) a channel, to stop receiving messages sent to that channel.
    ///
    /// This has the same semantics as `join()`. Similarly, a `part()` call will have no effect
    /// if the channel is not currently joined.
    pub fn part(&self, channel_login: String) {
        self.client_loop_tx
            .send(ClientLoopCommand::Part { channel_login })
            .unwrap();
    }

    /// Ping a random connection from the server. This does not await the response from Twitch.
    /// (The future resolves once the `PING` command is sent to the wire, or an error has occurred)
    pub async fn ping(&self) -> Result<(), ConnectionError<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.client_loop_tx
            .send(ClientLoopCommand::Ping {
                return_sender: return_tx,
            })
            .unwrap();
        // unwrap: ClientLoopWorker should not die before all sender handles have been dropped
        return_rx.await.unwrap()
    }
}
