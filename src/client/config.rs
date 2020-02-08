use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[async_trait]
pub trait LoginCredentials: Debug + Send + Sync + 'static {
    type Error: Debug + Display;
    fn get_login(&self) -> &str;
    async fn get_token(&self) -> Result<&Option<String>, Self::Error>;
}

#[derive(Debug)]
pub struct StaticLoginCredentials {
    pub login: String,
    pub token: Option<String>,
}

impl StaticLoginCredentials {
    pub fn new(login: String, token: Option<String>) -> StaticLoginCredentials {
        StaticLoginCredentials { login, token }
    }

    pub fn anonymous() -> StaticLoginCredentials {
        StaticLoginCredentials {
            login: "justinfan12345".to_owned(),
            token: None,
        }
    }
}

#[async_trait]
impl LoginCredentials for StaticLoginCredentials {
    type Error = Infallible;

    fn get_login(&self) -> &str {
        &self.login
    }

    async fn get_token(&self) -> Result<&Option<String>, Infallible> {
        Ok(&self.token)
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
