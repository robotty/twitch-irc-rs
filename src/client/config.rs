use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[async_trait]
pub trait LoginCredentials: Debug {
    type Error: Debug + Display;
    async fn get_nick_pass(&self) -> Result<(&str, &str), Self::Error>;
}

#[derive(Debug)]
pub struct StaticLoginCredentials {
    nick: String,
    pass: String,
}

impl StaticLoginCredentials {
    pub fn new(nick: String, pass: String) -> StaticLoginCredentials {
        StaticLoginCredentials { nick, pass }
    }
}

#[async_trait]
impl LoginCredentials for StaticLoginCredentials {
    type Error = Infallible;

    async fn get_nick_pass(&self) -> Result<(&str, &str), Infallible> {
        Ok((&self.nick, &self.pass))
    }
}

// TODO: Login credentials that can use a non-infinite token and refreshes on demand

pub struct ClientConfig<L: LoginCredentials> {
    pub login_credentials: L,
}
