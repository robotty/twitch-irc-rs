//! Logic for getting credentials to log into chat with.

use async_trait::async_trait;
use std::convert::Infallible;
use std::fmt::{Debug, Display};

#[cfg(feature = "refreshing-token")]
use {
    chrono::DateTime,
    chrono::Utc,
    std::{sync::Arc, time::Duration},
    thiserror::Error,
    tokio::sync::Mutex,
};

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// A pair of login name and OAuth token.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
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
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
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
    /// OAuth access token
    pub access_token: String,
    /// OAuth refresh token
    pub refresh_token: String,
    /// Timestamp of when this user access token was created
    pub created_at: DateTime<Utc>,
    /// Timestamp of when this user access token expires. `None` if this token never expires.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Represents the Twitch API response to `POST /oauth2/token` API requests.
///
/// Provided as a convenience for your own implementations, as you will typically need
/// to parse this response during the process of getting the inital token after user authorization
/// has been granted.
///
/// Includes a `impl From<GetAccessTokenResponse> for UserAccessToken` for simple
/// conversion to a `UserAccessToken`:
///
/// ```
/// # use twitch_irc::login::{GetAccessTokenResponse, UserAccessToken};
/// let json_response = r#"{"access_token":"xxxxxxxxxxxxxxxxxxxxxxxxxxx","expires_in":14346,"refresh_token":"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx","scope":["user_read"],"token_type":"bearer"}"#;
/// let decoded_response: GetAccessTokenResponse = serde_json::from_str(json_response).unwrap();
/// let user_access_token: UserAccessToken = UserAccessToken::from(decoded_response);
/// ```
#[cfg(feature = "refreshing-token")]
#[derive(Serialize, Deserialize)]
pub struct GetAccessTokenResponse {
    // {
    //   "access_token": "xxxxxxxxxxxxxxxxxxxxxxxxxxx",
    //   "expires_in": 14346, // this is entirely OMITTED for infinitely-lived tokens
    //   "refresh_token": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
    //   "scope": [
    //     "user_read"
    //   ], // scope is also entirely omitted if we didn't request any scopes in the request
    //   "token_type": "bearer"
    // }
    /// OAuth access token
    pub access_token: String,
    /// OAuth refresh token
    pub refresh_token: String,
    /// Specifies the time when this token expires (number of seconds from now). `None` if this token
    /// never expires.
    pub expires_in: Option<u64>,
}

#[cfg(feature = "refreshing-token")]
impl From<GetAccessTokenResponse> for UserAccessToken {
    fn from(response: GetAccessTokenResponse) -> Self {
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
#[derive(Debug, Clone)]
pub struct RefreshingLoginCredentials<S: TokenStorage> {
    http_client: reqwest::Client,
    user_login: Arc<Mutex<Option<String>>>,
    client_id: String,
    client_secret: String,
    token_storage: Arc<Mutex<S>>,
}

#[cfg(feature = "refreshing-token")]
impl<S: TokenStorage> RefreshingLoginCredentials<S> {
    /// Create new login credentials with a backing token storage.
    pub fn new(
        client_id: String,
        client_secret: String,
        token_storage: S,
    ) -> RefreshingLoginCredentials<S> {
        RefreshingLoginCredentials {
            http_client: reqwest::Client::new(),
            user_login: Arc::new(Mutex::new(None)),
            client_id,
            client_secret,
            token_storage: Arc::new(Mutex::new(token_storage)),
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
                .json::<GetAccessTokenResponse>()
                .await
                .map_err(RefreshingLoginError::RefreshError)?;

            // replace the current token
            current_token = UserAccessToken::from(response);

            token_storage
                .update_token(&current_token)
                .await
                .map_err(RefreshingLoginError::UpdateError)?;
        }

        let mut current_login = self.user_login.lock().await;

        let login = match &*current_login {
            Some(login) => login.clone(),
            None => {
                let response = self
                    .http_client
                    .get("https://api.twitch.tv/helix/users")
                    .header("Client-Id", &self.client_id)
                    .bearer_auth(&current_token.access_token)
                    .send()
                    .await
                    .map_err(RefreshingLoginError::RefreshError)?;

                let users_response = response
                    .json::<UsersResponse>()
                    .await
                    .map_err(RefreshingLoginError::RefreshError)?;

                // If no users are specified in the query, the API reponds with the user of the bearer token.
                let user = users_response.data.into_iter().next().unwrap();

                // TODO Have the fetched login name expire automatically to be resilient to bot's namechanges
                // should then also automatically reconnect all connections with the new username, so the change
                // will be a little more complex than just adding an expiry to this logic here.
                log::info!(
                    "Fetched login name `{}` for provided auth token",
                    &user.login
                );

                *current_login = Some(user.login.clone());

                user.login
            }
        };

        Ok(CredentialsPair {
            login,
            token: Some(current_token.access_token.clone()),
        })
    }
}

/// Represents the Twitch API response to `/helix/users` API requests.
/// It is used when fetching the username from the API in `RefreshingLoginCredentials`.
#[cfg(feature = "refreshing-token")]
#[derive(Deserialize)]
struct UsersResponse {
    data: Vec<UserObject>,
}

/// Represents a user object in Twitch API responses.
#[cfg(feature = "refreshing-token")]
#[derive(Deserialize)]
struct UserObject {
    id: String,
    login: String,
    display_name: String,
}
