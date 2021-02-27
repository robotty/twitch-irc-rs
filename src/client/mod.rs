mod event_loop;
mod pool_connection;

use crate::client::event_loop::{ClientLoopCommand, ClientLoopWorker};
use crate::config::ClientConfig;
use crate::error::Error;
use crate::irc;
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::message::{IRCTags, PrivmsgMessage};
use crate::transport::Transport;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
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
    /// Create a new client from the given configuration.
    ///
    /// Note this method is not side-effect-free - a background task will be spawned
    /// as a result of calling this function.
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

        #[cfg(feature = "metrics-collection")]
        if let Some(ref metrics_identifier) = config.metrics_identifier {
            metrics::register_counter!(
                "twitch_irc_messages_received",
                "Counts all incoming messages",
                "client" => metrics_identifier.clone(),
                "command" => "PRIVMSG"
            );
            metrics::register_counter!(
                "twitch_irc_messages_sent",
                "Counts all outgoing messages",
                "client" => metrics_identifier.clone(),
                "command" => "PING"
            );
            metrics::register_gauge!(
                "twitch_irc_channels",
                "Number of joined channels",
                "client" => metrics_identifier.clone()
            );
            metrics::register_gauge!(
                "twitch_irc_connections",
                "Number of connections in use by this client",
                "client" => metrics_identifier.clone(),
                "type" => "server"
            );
            metrics::register_counter!(
                "twitch_irc_reconnects",
                "Counts up every time a connection in the connection pool fails unexpectedly",
                "client" => metrics_identifier.clone()
            );
        }

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
    /// calling `.connect()` is superfluous, as the client will automatically open the necessary
    /// connections when you join channels or send messages.
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

    /// Send an arbitrary IRC message to one of the connections in the connection pool.
    ///
    /// An error is returned in case the message could not be sent over the picked connection.
    pub async fn send_message(&self, message: IRCMessage) -> Result<(), Error<T, L>> {
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

    /// Send a `PRIVMSG`-type IRC message to a Twitch channel. The `message` can be a normal
    /// chat message or a chat command like `/ban` or similar.
    ///
    /// If you want to just send a normal chat message, `say()` should be preferred since it
    /// prevents commands like `/ban` from accidentally being executed.
    pub async fn privmsg(&self, channel_login: String, message: String) -> Result<(), Error<T, L>> {
        self.send_message(irc!["PRIVMSG", format!("#{}", channel_login), message])
            .await
    }

    /// Ban a user with an optional reason from the given Twitch channel.
    ///
    /// Note that this will not throw an error if the target user is already banned, doesn't exist
    /// or if the logged-in user does not have the required permission to ban the user. An error
    /// is only returned if something prevented the command from being sent over the wire.
    pub async fn ban(
        &self,
        channel_login: String,
        target_login: &str,
        reason: Option<&str>,
    ) -> Result<(), Error<T, L>> {
        let command = match reason {
            Some(reason) => format!("/ban {} {}", target_login, reason),
            None => format!("/ban {}", target_login),
        };
        self.privmsg(channel_login, command).await
    }

    /// Unban a user from the given Twitch channel.
    ///
    /// Note that this will not throw an error if the target user is not currently banned, doesn't exist
    /// or if the logged-in user does not have the required permission to unban the user. An error
    /// is only returned if something prevented the command from being sent over the wire.
    pub async fn unban(
        &self,
        channel_login: String,
        target_login: &str,
    ) -> Result<(), Error<T, L>> {
        self.privmsg(channel_login, format!("/unban {}", target_login))
            .await
    }

    /// Timeout a user in the given Twitch channel.
    ///
    /// Note that this will not throw an error if the target user is banned, doesn't exist
    /// or if the logged-in user does not have the required permission to timeout the user. An error
    /// is only returned if something prevented the command from being sent over the wire.
    pub async fn timeout(
        &self,
        channel_login: String,
        target_login: &str,
        duration: Duration,
        reason: Option<&str>,
    ) -> Result<(), Error<T, L>> {
        let command = match reason {
            Some(reason) => format!(
                "/timeout {} {} {}",
                target_login,
                duration.as_secs(),
                reason
            ),
            None => format!("/timeout {} {}", target_login, duration.as_secs()),
        };

        self.privmsg(channel_login, command).await
    }

    /// Remove the timeout from a user in the given Twitch channel.
    ///
    /// Note that this will not throw an error if the target user is banned, not currently timed
    /// out, doesn't exist or if the logged-in user does not have the required permission to remove
    /// the timeout from the user. An error is only returned if something prevented the command from
    /// being sent over the wire.
    pub async fn untimeout(
        &self,
        channel_login: String,
        target_login: &str,
    ) -> Result<(), Error<T, L>> {
        self.privmsg(channel_login, format!("/untimeout {}", target_login))
            .await
    }

    /// Say a chat message in the given Twitch channel.
    ///
    /// This method automatically prevents commands from being executed. For example
    /// `say("a_channel", "/ban a_user") would not actually ban a user, instead it would
    /// send that exact message as a normal chat message instead.
    ///
    /// No particular filtering is performed on the message. If the message is too long for chat,
    /// it will not be cut short or split into multiple messages (what happens is determined
    /// by the behaviour of the Twitch IRC server).
    pub async fn say(&self, channel_login: String, message: String) -> Result<(), Error<T, L>> {
        self.say_in_response(channel_login, message, None).await
    }

    /// Say a chat message in the given Twitch channel, but send it as a response to another message if `reply_to_id` is specified.
    ///
    /// Behaves the same as `say()` when `reply_to_id` is None, but tags the original message and it's sender if specified.
    pub async fn say_in_response(
        &self,
        channel_login: String,
        message: String,
        reply_to_id: Option<String>,
    ) -> Result<(), Error<T, L>> {
        let mut tags = IRCTags::new();

        if let Some(id) = reply_to_id {
            tags.0.insert("reply-parent-msg-id".to_string(), Some(id));
        }

        let irc_message = IRCMessage::new(
            tags,
            None,
            "PRIVMSG".to_string(),
            vec![format!("#{}", channel_login), format!(". {}", message)], // The prefixed "." prevents commands from being executed
        );
        self.send_message(irc_message).await
    }

    /// Replies to a given `PrivmsgMessage`, tagging the original message and it's sender.
    ///
    /// Similarly to `say()`, this method strips the message of executing commands, but does not filter out messages which are too long.
    /// Refer to `say()` for the exact behaviour.
    pub async fn reply_to_privmsg(
        &self,
        message: String,
        reply_to: &PrivmsgMessage,
    ) -> Result<(), Error<T, L>> {
        self.say_in_response(
            reply_to.channel_login.clone(),
            message,
            Some(reply_to.message_id.clone()),
        )
        .await
    }

    /// Join the given Twitch channel (When a channel is joined, the client will receive messages
    /// sent to it).
    ///
    /// The client will internally ensure that there has always been at least _an attempt_ to join
    /// this channel. However this does not necessarily mean the join is always successful.
    ///
    /// If the given `channel_login` does not exist (or is suspended) then the IRC server
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

    /// Instruct the client to only be connected to these channels. Channels currently joined
    /// but not in the given set are parted, and channels in the set that are not currently
    /// joined are joined.
    pub fn set_wanted_channels(&self, channels: HashSet<String>) {
        self.client_loop_tx
            .send(ClientLoopCommand::SetWantedChannels { channels })
            .unwrap();
    }

    /// Query the client for what status a certain channel is in.
    ///
    /// Returns two booleans: The first indicates whether a channel is `wanted`. This is true
    /// if the last operation for this channel was a `join()` method, or alternatively whether
    /// it was included in the set of channels in a `set_wanted_channels` call.
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

    /// Ping a random connection. This does not await the `PONG` response from Twitch.
    /// The future resolves once the `PING` command is sent to the wire.
    /// An error is returned in case the message could not be sent over the picked connection.
    pub async fn ping(&self) -> Result<(), Error<T, L>> {
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
