use crate::client::config::LoginCredentials;
use crate::client::connection::Connection;
use crate::client::transport::Transport;
use crate::irc;
use crate::message::IRCMessage;
use async_trait::async_trait;
use futures::SinkExt;
use std::fmt::{Debug, Display};
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
enum LoginError<L, T>
where
    L: Display + Debug,
    T: Display + Debug,
{
    #[error("{0}")]
    CredentialsError(L),
    #[error("{0}")]
    TransportOutgoingError(T),
}

#[async_trait]
trait ConnectionOperations<T: Transport, L: LoginCredentials> {
    async fn send_msg(&self, message: IRCMessage) -> Result<(), T::OutgoingError>;

    async fn login(&self) -> Result<(), LoginError<L::Error, T::OutgoingError>>;

    async fn privmsg(&self, channel: String, message: String) -> Result<(), T::OutgoingError>;
    async fn say(&self, channel: String, message: String) -> Result<(), T::OutgoingError>;
    async fn me(&self, channel: String, message: &str) -> Result<(), T::OutgoingError>;
    async fn whisper(&self, recipient: String, message: &str) -> Result<(), T::OutgoingError>;
    async fn set_color(&self, new_color: &str) -> Result<(), T::OutgoingError>;
    async fn ban(&self, channel: String, target_user: &str) -> Result<(), T::OutgoingError>;
    async fn unban(&self, channel: String, target_user: &str) -> Result<(), T::OutgoingError>;
    async fn timeout(
        &self,
        channel: String,
        target_user: &str,
        length: &Duration,
    ) -> Result<(), T::OutgoingError>;
    async fn untimeout(&self, channel: String, target_user: &str) -> Result<(), T::OutgoingError>;
    async fn enable_slowmode(
        &self,
        channel: String,
        time_between_messages: &Duration,
    ) -> Result<(), T::OutgoingError>;
    async fn disable_slowmode(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn enable_r9k(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn disable_r9k(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn enable_emote_only(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn disable_emote_only(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn clear_chat(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn enable_susbcribers_only(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn disable_subscribers_only(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn enable_followers_only(
        &self,
        channel: String,
        must_follow_for: &Option<Duration>,
    ) -> Result<(), T::OutgoingError>;
    async fn disable_followers_only(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn host(&self, channel: String, hostee: &str) -> Result<(), T::OutgoingError>;
    async fn exit_host_mode(&self, channel: String) -> Result<(), T::OutgoingError>;
    async fn start_raid(&self, channel: String, raidee: &str) -> Result<(), T::OutgoingError>;
    async fn cancel_raid(&self, channel: String) -> Result<(), T::OutgoingError>;
}

#[async_trait]
impl<T: Transport, L: LoginCredentials> ConnectionOperations<T, L> for Connection<T, L>
where
    T::Outgoing: Unpin,
{
    async fn send_msg(&self, message: IRCMessage) -> Result<(), T::OutgoingError> {
        let mut outgoing_messages = self.outgoing_messages.lock().await;
        outgoing_messages.send(message).await?;
        Ok(())
    }

    async fn login(&self) -> Result<(), LoginError<L::Error, T::OutgoingError>> {
        let nick = self.config.login_credentials.get_nick();
        let pass = self
            .config
            .login_credentials
            .get_pass()
            .await
            .map_err(LoginError::CredentialsError)?;

        if let Some(pass) = pass {
            // if no password is present we only send NICK, e.g. for anonymous login
            self.send_msg(irc!["PASS", pass])
                .await
                .map_err(LoginError::TransportOutgoingError)?;
        }
        self.send_msg(irc!["NICK", nick])
            .await
            .map_err(LoginError::TransportOutgoingError)?;

        Ok(())
    }

    async fn privmsg(&self, channel: String, message: String) -> Result<(), T::OutgoingError> {
        self.send_msg(irc!["PRIVMSG", format!("#{}", channel), message])
            .await
    }

    async fn say(&self, channel: String, message: String) -> Result<(), T::OutgoingError> {
        let message_to_send = if message.starts_with('/') || message.starts_with('.') {
            format!("/ {}", message)
        } else {
            message
        };

        self.privmsg(channel, message_to_send).await
    }

    async fn me(&self, channel: String, message: &str) -> Result<(), T::OutgoingError> {
        self.privmsg(channel, format!("/me {}", message)).await
    }

    async fn whisper(&self, recipient: String, message: &str) -> Result<(), T::OutgoingError> {
        // we use /w in our own channel since it's a) guaranteed to exist and b) is beneficial for rate limits
        self.privmsg(
            self.config.login_credentials.get_nick().to_owned(),
            format!("/w {} {}", recipient, message),
        )
        .await
    }

    async fn set_color(&self, new_color: &str) -> Result<(), T::OutgoingError> {
        self.privmsg(
            self.config.login_credentials.get_nick().to_owned(),
            format!("/color {}", new_color),
        )
        .await
    }

    async fn ban(&self, channel: String, target_user: &str) -> Result<(), T::OutgoingError> {
        self.privmsg(channel, format!("/ban {}", target_user)).await
    }

    async fn unban(
        &self,
        channel: String,
        target_user: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/unban {}", target_user))
            .await
    }

    async fn timeout(
        &self,
        channel: String,
        target_user: &str,
        length: &Duration,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(
            channel,
            format!("/timeout {} {}", target_user, length.as_secs()),
        )
        .await
    }

    async fn untimeout(
        &self,
        channel: String,
        target_user: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/untimeout {}", target_user))
            .await
    }

    async fn enable_slowmode(
        &self,
        channel: String,
        time_between_messages: &Duration,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(
            channel,
            format!("/slow {}", time_between_messages.as_secs()),
        )
        .await
    }

    async fn disable_slowmode(
        &self,
        channel: String,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/slowoff".to_owned()).await
    }

    async fn enable_r9k(&self, channel: String) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/r9kbeta".to_owned()).await
    }

    async fn disable_r9k(&self, channel: String) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/r9kbetaoff".to_owned()).await
    }

    async fn enable_emote_only(
        &self,
        channel: String,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/emoteonly".to_owned()).await
    }

    async fn disable_emote_only(
        &self,
        channel: String,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/emoteonlyoff".to_owned()).await
    }

    async fn clear_chat(&self, channel: String) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/clear".to_owned()).await
    }

    async fn enable_susbcribers_only(
        &self,
        channel: String,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/susbcribers".to_owned()).await
    }

    async fn disable_subscribers_only(
        &self,
        channel: String,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/subscribersoff".to_owned()).await
    }

    async fn enable_followers_only(
        &self,
        channel: String,
        must_follow_for: &Option<Duration>,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        let msg_to_send = if let Some(must_follow_for) = must_follow_for {
            format!("/followers {}", must_follow_for.as_secs() / 60)
        } else {
            "/followers".to_owned()
        };

        self.privmsg(channel, msg_to_send).await
    }

    async fn disable_followers_only(
        &self,
        channel: String,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/followersonlyoff".to_owned()).await
    }

    async fn host(
        &self,
        channel: String,
        hostee: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/host {}", hostee)).await
    }

    async fn exit_host_mode(&self, channel: String) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/hostoff".to_owned()).await
    }

    async fn start_raid(
        &self,
        channel: String,
        raidee: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/raid {}", raidee)).await
    }

    async fn cancel_raid(&self, channel: String) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/unraid".to_owned()).await
    }
}
