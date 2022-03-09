//! Contains an utility to validate channel names

use thiserror::Error;

/// Validate a given login name. Returns an error detailing the issue
/// if the string is found to be invalid.
pub fn validate_login(channel_login: &str) -> Result<(), Error> {
    let mut length: usize = 0;
    for char in channel_login.chars() {
        if !(matches!(char, 'a'..='z' | '0'..='9' | '_')) {
            return Err(Error::InvalidCharacter {
                login: channel_login.to_owned(),
                position: length,
                character: char,
            });
        }

        length += 1;
        if length > 25 {
            return Err(Error::TooLong {
                login: channel_login.to_owned(),
            });
        }
    }
    if length < 1 {
        return Err(Error::TooShort {
            login: channel_login.to_owned(),
        });
    }

    Ok(())
}

/// Types of errors that can be found as a result of validating a channel login name. See the enum
/// variants for details
#[derive(Error, Debug, PartialEq)]
pub enum Error {
    /// A character not allowed in login names was found at a certain position in the given string
    #[error("Invalid login name `{login}`: Invalid character `{character}` encountered at position `{position}`")]
    InvalidCharacter {
        /// The login name that failed validation.
        login: String,
        /// Index of the found invalid character in the original string
        position: usize,
        /// The invalid character
        character: char,
    },
    /// Login name exceeds maximum length of 25 characters
    #[error("Invalid login name `{login}`: Login name exceeds maximum length of 25 characters")]
    TooLong {
        /// The login name that failed validation.
        login: String,
    },
    /// Login name is too short (must be at least one character long)
    #[error("Invalid login name `{login}`: Login name is too short (must be at least one character long)")]
    TooShort {
        /// The login name that failed validation.
        login: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::validate::validate_login;
    use crate::validate::Error;

    #[test]
    pub fn test_validate_login() {
        assert_eq!(Ok(()), validate_login("pajlada"));
        assert_eq!(
            Err(Error::InvalidCharacter {
                login: "pajLada".to_owned(),
                position: 3,
                character: 'L',
            }),
            validate_login("pajLada")
        );
        assert_eq!(
            Err(Error::InvalidCharacter {
                login: "pajlada,def".to_owned(),
                position: 7,
                character: ',',
            }),
            validate_login("pajlada,def")
        );
        assert_eq!(
            Err(Error::InvalidCharacter {
                login: "pajlada-def".to_owned(),
                position: 7,
                character: '-',
            }),
            validate_login("pajlada-def")
        );
        assert_eq!(Ok(()), validate_login("1234567890123456789012345"));
        assert_eq!(
            Err(Error::TooLong {
                login: "12345678901234567890123456".to_owned()
            }),
            validate_login("12345678901234567890123456")
        );
        assert_eq!(Ok(()), validate_login("a"));
        assert_eq!(Ok(()), validate_login("abc"));
        assert_eq!(Ok(()), validate_login("xqco"));
        assert_eq!(Ok(()), validate_login("cool_user___"));
        assert_eq!(Ok(()), validate_login("cool_7user___7"));
        assert_eq!(
            Err(Error::TooShort {
                login: "".to_owned()
            }),
            validate_login("")
        );
    }
}
