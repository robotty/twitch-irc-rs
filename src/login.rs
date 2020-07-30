use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[cfg(feature = "refreshing-token")]
use {
    chrono::DateTime, chrono::Utc, serde::Deserialize, serde::Serialize, std::time::Duration,
    thiserror::Error, tokio::sync::Mutex,
};

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

#[cfg(feature = "refreshing-token")]
#[derive(Debug, Serialize, Deserialize)]
pub struct UserAccessToken {
    access_token: String,
    refresh_token: String,
    created_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
}

#[cfg(feature = "refreshing-token")]
#[derive(Deserialize)]
struct RefreshAccessTokenResponse {
    // {
    //   "access_token": "xxxxxxxxxxxxxxxxxxxxxxxxxxx",
    //   "expires_in": 14346, // this is entirely OMITTED for infinitely-lived tokens
    //   "refresh_token": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    //   "scope": [
    //     "user_read"
    //   ], // scope is also entirely omitted if we didn't request any scopes in the request
    //   "token_type": "bearer"
    // }
    access_token: String,
    refresh_token: String,
    expires_in: Option<u64>,
}

#[cfg(feature = "refreshing-token")]
impl From<RefreshAccessTokenResponse> for UserAccessToken {
    fn from(response: RefreshAccessTokenResponse) -> Self {
        let now = Utc::now();
        UserAccessToken {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            created_at: now,
            expires_at: response
                .expires_in
                .map(|d| now + chrono::Duration::from_std(Duration::from_secs(d)).unwrap()),
        }
    }
}

#[cfg(feature = "refreshing-token")]
#[async_trait]
pub trait TokenStorage: Debug + Send + 'static {
    type LoadError: Send + Sync + Debug + Display;
    type UpdateError: Send + Sync + Debug + Display;

    async fn load_token(&mut self) -> Result<UserAccessToken, Self::LoadError>;
    async fn update_token(&mut self, token: &UserAccessToken) -> Result<(), Self::UpdateError>;
}

#[cfg(feature = "refreshing-token")]
#[derive(Debug)]
pub struct RefreshingLoginCredentials<S: TokenStorage> {
    http_client: reqwest::Client,
    // TODO we could fetch this using the API, based on the token provided.
    user_login: String,
    client_id: String,
    client_secret: String,
    token_storage: Mutex<S>,
}

#[cfg(feature = "refreshing-token")]
impl<S: TokenStorage> RefreshingLoginCredentials<S> {
    pub fn new(
        user_login: String,
        client_id: String,
        client_secret: String,
        token_storage: S,
    ) -> RefreshingLoginCredentials<S> {
        RefreshingLoginCredentials {
            http_client: reqwest::Client::new(),
            user_login,
            client_id,
            client_secret,
            token_storage: Mutex::new(token_storage),
        }
    }
}

#[cfg(feature = "refreshing-token")]
#[derive(Error, Debug)]
pub enum RefreshingLoginError<S: TokenStorage> {
    #[error("Failed to retrieve token from storage: {0:?}")]
    LoadError(S::LoadError),
    #[error("Failed to refresh token: {0:?}")]
    RefreshError(reqwest::Error),
    #[error("Failed to update token in storage: {0:?}")]
    UpdateError(S::UpdateError),
}

#[cfg(feature = "refreshing-token")]
const SHOULD_REFRESH_AFTER_FACTOR: f64 = 0.9;

#[cfg(feature = "refreshing-token")]
#[async_trait]
impl<S: TokenStorage> LoginCredentials for RefreshingLoginCredentials<S> {
    type Error = RefreshingLoginError<S>;

    async fn get_credentials(&self) -> Result<CredentialsPair, RefreshingLoginError<S>> {
        let mut token_storage = self.token_storage.lock().await;

        let mut current_token = token_storage
            .load_token()
            .await
            .map_err(RefreshingLoginError::LoadError)?;

        let token_expires_after = if let Some(expires_at) = current_token.expires_at {
            // to_std() converts the time::duration::Duration chrono uses to a std::time::Duration
            (expires_at - current_token.created_at).to_std().unwrap()
        } else {
            // 24 hours
            Duration::from_secs(24 * 60 * 60)
        };
        let token_age = (Utc::now() - current_token.created_at).to_std().unwrap();
        let max_token_age = token_expires_after.mul_f64(SHOULD_REFRESH_AFTER_FACTOR);
        let is_token_expired = token_age >= max_token_age;

        if is_token_expired {
            let response = self
                .http_client
                .post("https://id.twitch.tv/oauth2/token")
                .query(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &current_token.refresh_token),
                    ("client_id", &self.client_id),
                    ("client_secret", &self.client_secret),
                ])
                .send()
                .await
                .map_err(RefreshingLoginError::RefreshError)?
                .json::<RefreshAccessTokenResponse>()
                .await
                .map_err(RefreshingLoginError::RefreshError)?;

            // replace the current token
            current_token = UserAccessToken::from(response);

            token_storage
                .update_token(&current_token)
                .await
                .map_err(RefreshingLoginError::UpdateError)?;
        }

        Ok(CredentialsPair {
            login: self.user_login.clone(),
            token: Some(current_token.access_token.clone()),
        })
    }
}
