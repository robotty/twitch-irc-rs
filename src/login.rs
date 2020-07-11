use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

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
