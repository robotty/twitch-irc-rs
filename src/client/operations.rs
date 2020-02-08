use crate::client::config::LoginCredentials;
use crate::client::connection::Connection;
use crate::client::transport::Transport;
use crate::irc;
use crate::message::IRCMessage;
use async_trait::async_trait;
use futures::SinkExt;
use std::fmt::{Debug, Display, Write};
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoginError<L, T>
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
pub trait ConnectionOperations<T: Transport, L: LoginCredentials> {
    async fn send_msg(&self, message: IRCMessage) -> Result<(), T::OutgoingError>;

    async fn login(&self) -> Result<(), LoginError<L::Error, T::OutgoingError>>;
    async fn request_capabilities(&self) -> Result<(), T::OutgoingError>;
    async fn join(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn part(&self, channel: &str) -> Result<(), T::OutgoingError>;

    async fn privmsg(&self, channel: &str, message: String) -> Result<(), T::OutgoingError>;
    async fn say(&self, channel: &str, message: String) -> Result<(), T::OutgoingError>;
    async fn me(&self, channel: &str, message: &str) -> Result<(), T::OutgoingError>;
    async fn whisper(&self, recipient: &str, message: &str) -> Result<(), T::OutgoingError>;
    async fn set_color(&self, new_color: &str) -> Result<(), T::OutgoingError>;
    async fn ban(&self, channel: &str, target_user: &str) -> Result<(), T::OutgoingError>;
    async fn unban(&self, channel: &str, target_user: &str) -> Result<(), T::OutgoingError>;
    async fn timeout(
        &self,
        channel: &str,
        target_user: &str,
        length: &Duration,
    ) -> Result<(), T::OutgoingError>;
    async fn untimeout(&self, channel: &str, target_user: &str) -> Result<(), T::OutgoingError>;
    async fn enable_slowmode(
        &self,
        channel: &str,
        time_between_messages: &Duration,
    ) -> Result<(), T::OutgoingError>;
    async fn disable_slowmode(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn enable_r9k(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn disable_r9k(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn enable_emote_only(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn disable_emote_only(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn clear_chat(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn enable_susbcribers_only(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn disable_subscribers_only(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn enable_followers_only(
        &self,
        channel: &str,
        must_follow_for: &Option<Duration>,
    ) -> Result<(), T::OutgoingError>;
    async fn disable_followers_only(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn host(&self, channel: &str, hostee: &str) -> Result<(), T::OutgoingError>;
    async fn exit_host_mode(&self, channel: &str) -> Result<(), T::OutgoingError>;
    async fn start_raid(&self, channel: &str, raidee: &str) -> Result<(), T::OutgoingError>;
    async fn cancel_raid(&self, channel: &str) -> Result<(), T::OutgoingError>;
}

#[async_trait]
impl<T: Transport, L: LoginCredentials> ConnectionOperations<T, L> for Connection<T, L> {
    async fn send_msg(&self, message: IRCMessage) -> Result<(), T::OutgoingError> {
        let mut outgoing_messages = self.outgoing_messages.lock().await;
        outgoing_messages.send(message).await?;
        Ok(())
    }

    async fn login(&self) -> Result<(), LoginError<L::Error, T::OutgoingError>> {
        let login = self.config.login_credentials.get_login();
        let token = self
            .config
            .login_credentials
            .get_token()
            .await
            .map_err(LoginError::CredentialsError)?;

        if let Some(token) = token {
            // if no password is present we only send NICK, e.g. for anonymous login
            self.send_msg(irc!["PASS", format!("oauth:{}", token)])
                .await
                .map_err(LoginError::TransportOutgoingError)?;
        }
        self.send_msg(irc!["NICK", login])
            .await
            .map_err(LoginError::TransportOutgoingError)?;

        Ok(())
    }

    async fn request_capabilities(&self) -> Result<(), T::OutgoingError> {
        self.send_msg(irc!["CAP", "REQ", "twitch.tv/tags twitch.tv/commands"])
            .await
    }

    async fn join(&self, channel: &str) -> Result<(), T::OutgoingError> {
        let mut channels = self.channels.lock().await;
        channels.insert(channel.to_owned());
        self.send_msg(irc!["JOIN", format!("#{}", channel)]).await
    }

    async fn part(&self, channel: &str) -> Result<(), T::OutgoingError> {
        let mut channels = self.channels.lock().await;
        channels.remove(channel);
        self.send_msg(irc!["PART", format!("#{}", channel)]).await
    }

    async fn privmsg(&self, channel: &str, message: String) -> Result<(), T::OutgoingError> {
        self.send_msg(irc!["PRIVMSG", format!("#{}", channel), message])
            .await
    }

    async fn say(&self, channel: &str, message: String) -> Result<(), T::OutgoingError> {
        let message_to_send = if message.starts_with('/') || message.starts_with('.') {
            format!("/ {}", message)
        } else {
            message
        };

        self.privmsg(channel, message_to_send).await
    }

    async fn me(&self, channel: &str, message: &str) -> Result<(), T::OutgoingError> {
        self.privmsg(channel, format!("/me {}", message)).await
    }

    async fn whisper(&self, recipient: &str, message: &str) -> Result<(), T::OutgoingError> {
        // we use /w in our own channel since it's a) guaranteed to exist and b) is beneficial for rate limits
        self.privmsg(
            self.config.login_credentials.get_login(),
            format!("/w {} {}", recipient, message),
        )
        .await
    }

    async fn set_color(&self, new_color: &str) -> Result<(), T::OutgoingError> {
        self.privmsg(
            self.config.login_credentials.get_login(),
            format!("/color {}", new_color),
        )
        .await
    }

    async fn ban(&self, channel: &str, target_user: &str) -> Result<(), T::OutgoingError> {
        self.privmsg(channel, format!("/ban {}", target_user)).await
    }

    async fn unban(
        &self,
        channel: &str,
        target_user: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/unban {}", target_user))
            .await
    }

    async fn timeout(
        &self,
        channel: &str,
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
        channel: &str,
        target_user: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/untimeout {}", target_user))
            .await
    }

    async fn enable_slowmode(
        &self,
        channel: &str,
        time_between_messages: &Duration,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(
            channel,
            format!("/slow {}", time_between_messages.as_secs()),
        )
        .await
    }

    async fn disable_slowmode(&self, channel: &str) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/slowoff".to_owned()).await
    }

    async fn enable_r9k(&self, channel: &str) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/r9kbeta".to_owned()).await
    }

    async fn disable_r9k(&self, channel: &str) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/r9kbetaoff".to_owned()).await
    }

    async fn enable_emote_only(
        &self,
        channel: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/emoteonly".to_owned()).await
    }

    async fn disable_emote_only(
        &self,
        channel: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/emoteonlyoff".to_owned()).await
    }

    async fn clear_chat(&self, channel: &str) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/clear".to_owned()).await
    }

    async fn enable_susbcribers_only(
        &self,
        channel: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/subscribers".to_owned()).await
    }

    async fn disable_subscribers_only(
        &self,
        channel: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/subscribersoff".to_owned()).await
    }

    async fn enable_followers_only(
        &self,
        channel: &str,
        must_follow_for: &Option<Duration>,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        let msg_to_send = if let Some(must_follow_for) = must_follow_for {
            // we need a format like 2mo29d23h59m59s
            // If we just did /followers <number>, the number would be taken as minutes
            // to reach seconds precision, we need to use the complex format described above
            // (The server does not accept large seconds amounts using /followers 999999s for example)

            let formats_and_seconds = [
                ("mo", 1 * 60 * 60 * 24 * 30),
                ("d", 1 * 60 * 60 * 24),
                ("h", 1 * 60 * 60),
                ("m", 1 * 60),
                ("s", 1),
            ];

            let mut seconds_remaining = must_follow_for.as_secs();
            let mut result = String::from("/followers ");
            for (format_name, seconds_in_unit) in &formats_and_seconds {
                let quantity_of_this_unit = seconds_remaining / seconds_in_unit;

                if quantity_of_this_unit > 0 {
                    write!(result, "{}{}", quantity_of_this_unit, format_name).unwrap();
                }

                seconds_remaining = seconds_remaining % seconds_in_unit;
            }

            result
        } else {
            "/followers".to_owned()
        };

        self.privmsg(channel, msg_to_send).await
    }

    async fn disable_followers_only(
        &self,
        channel: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/followersoff".to_owned()).await
    }

    async fn host(
        &self,
        channel: &str,
        hostee: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/host {}", hostee)).await
    }

    async fn exit_host_mode(&self, channel: &str) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/hostoff".to_owned()).await
    }

    async fn start_raid(
        &self,
        channel: &str,
        raidee: &str,
    ) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, format!("/raid {}", raidee)).await
    }

    async fn cancel_raid(&self, channel: &str) -> Result<(), <T as Transport>::OutgoingError> {
        self.privmsg(channel, "/unraid".to_owned()).await
    }
}
