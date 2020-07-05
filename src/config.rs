use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CredentialsPair {
    pub login: String,
    pub token: Option<String>,
}

#[async_trait]
pub trait LoginCredentials: Debug + Send + Sync + 'static {
    type Error: Send + Sync + Debug + Display;
    async fn get_credentials(&self) -> Result<CredentialsPair, Self::Error>;
}

#[derive(Debug, Clone)]
pub struct StaticLoginCredentials {
    pub credentials: CredentialsPair,
}

impl StaticLoginCredentials {
    pub fn new(login: String, token: Option<String>) -> StaticLoginCredentials {
        StaticLoginCredentials {
            credentials: CredentialsPair { login, token },
        }
    }

    pub fn anonymous() -> StaticLoginCredentials {
        StaticLoginCredentials::new("justinfan12345".to_owned(), None)
    }
}

#[async_trait]
impl LoginCredentials for StaticLoginCredentials {
    type Error = Infallible;

    async fn get_credentials(&self) -> Result<CredentialsPair, Infallible> {
        Ok(self.credentials.clone())
    }
}

// TODO: Login credentials that can use a non-infinite token and refreshes on demand

pub struct ClientConfig<L: LoginCredentials> {
    pub login_credentials: L,

    /// A new connection will automatically be created if a channel is joined and all
    /// currently established connections have joined at least this many channels.
    pub max_channels_per_connection: usize,

    /// A new connection will automatically be created if any message is to be sent
    /// and all currently established connections have recently sent more than this many
    /// messages (time interval is defined by `max_waiting_messages_duration_window`)
    pub max_waiting_messages_per_connection: usize,

    /// We assume messages to be "waiting" for this amount of time after sending them out, e.g.
    /// typically 100 or 150 milliseconds (purely a value that has been measured/observed,
    /// not documented or fixed in any way)
    pub time_per_message: Duration,
}

impl Default for ClientConfig<StaticLoginCredentials> {
    fn default() -> ClientConfig<StaticLoginCredentials> {
        ClientConfig {
            login_credentials: StaticLoginCredentials::anonymous(),
            max_channels_per_connection: 90,

            max_waiting_messages_per_connection: 5,
            time_per_message: Duration::from_millis(150),
        }
    }
}
