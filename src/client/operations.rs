use crate::client::config::LoginCredentials;
use crate::client::connection::Connection;
use crate::client::transport::Transport;
use crate::irc;
use crate::message::IRCMessage;
use async_trait::async_trait;
use futures::SinkExt;
use std::fmt::{Debug, Display};
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
trait ConnectionOperations<T: Transport> {
    async fn send_msg(&self, message: IRCMessage) -> Result<(), T::OutgoingError>;

    async fn login<L: LoginCredentials + Send + Sync>(
        &self,
        login_credentials: &L,
    ) -> Result<(), LoginError<L::Error, T::OutgoingError>>;
}

#[async_trait]
impl<T: Transport> ConnectionOperations<T> for Connection<T>
where
    T::Outgoing: Unpin,
{
    async fn send_msg(&self, message: IRCMessage) -> Result<(), T::OutgoingError> {
        let mut outgoing_messages = self.outgoing_messages.lock().await;
        outgoing_messages.send(message).await?;
        Ok(())
    }

    async fn login<L: LoginCredentials + Send + Sync>(
        &self,
        login_credentials: &L,
    ) -> Result<(), LoginError<L::Error, T::OutgoingError>> {
        let (pass, nick) = login_credentials
            .get_nick_pass()
            .await
            .map_err(LoginError::CredentialsError)?;

        self.send_msg(irc!["PASS", pass])
            .await
            .map_err(LoginError::TransportOutgoingError)?;
        self.send_msg(irc!["NICK", nick])
            .await
            .map_err(LoginError::TransportOutgoingError)?;

        Ok(())
    }
}
