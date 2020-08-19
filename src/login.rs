//! Logic for getting credentials to log into chat with.

use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[cfg(feature = "refreshing-token")]
use {
    chrono::DateTime, chrono::Utc, serde::Deserialize, serde::Serialize, std::time::Duration,
    thiserror::Error, tokio::sync::Mutex,
};

/// A pair of login name and OAuth token.
#[derive(Debug, Clone)]
pub struct CredentialsPair {
    /// Login name of the user that the library should log into chat as.
    pub login: String,
    /// OAuth access token, without leading `oauth:` prefix.
    /// If `None`, then no password will be sent to the server at all (for anonymous
    /// credentials).
    pub token: Option<String>,
}

/// Encapsulates logic for getting the credentials to log into chat, whenever
/// a new connection is made.
#[async_trait]
pub trait LoginCredentials: Debug + Send + Sync + 'static {
    /// Error type that can occur when trying to fetch the credentials.
    type Error: Send + Sync + Debug + Display;

    /// Get a fresh set of credentials to be used right-away.
    async fn get_credentials(&self) -> Result<CredentialsPair, Self::Error>;
}

/// Simple `LoginCredentials` implementation that always returns the same `CredentialsPair`
/// and never fails.
#[derive(Debug, Clone)]
pub struct StaticLoginCredentials {
    /// The credentials that are always returned.
    pub credentials: CredentialsPair,
}

impl StaticLoginCredentials {
    /// Create new static login credentials from the given Twitch login name and OAuth access token.
    /// The `token` should be without the `oauth:` prefix.
    pub fn new(login: String, token: Option<String>) -> StaticLoginCredentials {
        StaticLoginCredentials {
            credentials: CredentialsPair { login, token },
        }
    }

    /// Creates login credentials for logging into chat as an anonymous user.
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

/// The necessary details about a Twitch OAuth Access Token. This information is provided
/// by Twitch's OAuth API after completing the user's authorization.
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

/// Load and store the currently valid version of the user's OAuth Access Token.
#[cfg(feature = "refreshing-token")]
#[async_trait]
pub trait TokenStorage: Debug + Send + 'static {
    /// Possible error type when trying to load the token from this storage.
    type LoadError: Send + Sync + Debug + Display;
    /// Possible error type when trying to update the token in this storage.
    type UpdateError: Send + Sync + Debug + Display;

    /// Load the currently stored token from the storage.
    async fn load_token(&mut self) -> Result<UserAccessToken, Self::LoadError>;
    /// Called after the token was updated successfully, to save the new token.
    /// After `update_token()` completes, the `load_token()` method should then return
    /// that token for future invocations
    async fn update_token(&mut self, token: &UserAccessToken) -> Result<(), Self::UpdateError>;
}

/// Login credentials backed by a token storage and using OAuth refresh tokens, allowing use of OAuth tokens that expire
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
    /// Create new login credentials with a backing token storage.
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

/// Error type for the `RefreshingLoginCredentials` implementation.
#[cfg(feature = "refreshing-token")]
#[derive(Error, Debug)]
pub enum RefreshingLoginError<S: TokenStorage> {
    /// Failed to retrieve token from storage: `<cause>`
    #[error("Failed to retrieve token from storage: {0}")]
    LoadError(S::LoadError),
    /// Failed to refresh token: `<cause>`
    #[error("Failed to refresh token: {0}")]
    RefreshError(reqwest::Error),
    /// Failed to update token in storage: `<cause>`
    #[error("Failed to update token in storage: {0}")]
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
