use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[async_trait]
pub trait LoginCredentials: Debug + Send + Sync + 'static {
    type Error: Debug + Display;
    fn get_nick(&self) -> &str;
    async fn get_pass(&self) -> Result<&Option<String>, Self::Error>;
}

#[derive(Debug)]
pub struct StaticLoginCredentials {
    nick: String,
    pass: Option<String>,
}

impl StaticLoginCredentials {
    pub fn new(nick: String, pass: Option<String>) -> StaticLoginCredentials {
        StaticLoginCredentials { nick, pass }
    }

    pub fn anonymous() -> StaticLoginCredentials {
        StaticLoginCredentials {
            nick: "justinfan12345".to_owned(),
            pass: None,
        }
    }
}

#[async_trait]
impl LoginCredentials for StaticLoginCredentials {
    type Error = Infallible;

    fn get_nick(&self) -> &str {
        &self.nick
    }

    async fn get_pass(&self) -> Result<&Option<String>, Infallible> {
        Ok(&self.pass)
    }
}

// TODO: Login credentials that can use a non-infinite token and refreshes on demand

pub struct ClientConfig<L: LoginCredentials> {
    pub login_credentials: L,
}

impl<L: LoginCredentials> ClientConfig<L> {
    pub fn new(login_credentials: L) -> ClientConfig<L> {
        ClientConfig { login_credentials }
    }
}
