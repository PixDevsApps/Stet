//! The ids that name tabs in `session.json` and the backup files of their text.

use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;
use std::str::FromStr;

/// The longest tab or backup id, in bytes.
pub const MAX_ID_LEN: usize = 64;

/// Whether `id` can name a tab or backup: 1 to [`MAX_ID_LEN`] ASCII letters, digits, `-` and
/// `_`, so it is always a plain file name.
pub fn is_valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// The id of a tab in the session, and of the backup that holds a tab's text: valid as
/// [`is_valid_id`] says, so it is always a plain file name, or empty.
///
/// The empty id, the default, names nothing. In `session.json` an id is a plain string, and a
/// string that isn't a valid id reads as the empty id: [`Session::repair`](super::Session::repair)
/// then gives such a tab a fresh id, and a backup reference with it counts as a missing backup.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RecordId(String);

impl RecordId {
    /// The id as text; empty for the empty id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A text that isn't a valid tab or backup id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidId;

impl fmt::Display for InvalidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an id is 1 to {MAX_ID_LEN} ASCII letters, digits, `-` and `_`"
        )
    }
}

impl std::error::Error for InvalidId {}

impl TryFrom<String> for RecordId {
    type Error = InvalidId;

    fn try_from(id: String) -> Result<Self, InvalidId> {
        if is_valid_id(&id) {
            Ok(Self(id))
        } else {
            Err(InvalidId)
        }
    }
}

impl FromStr for RecordId {
    type Err = InvalidId;

    fn from_str(id: &str) -> Result<Self, InvalidId> {
        Self::try_from(id.to_owned())
    }
}

impl From<RecordId> for String {
    fn from(id: RecordId) -> Self {
        id.0
    }
}

impl<'de> Deserialize<'de> for RecordId {
    /// Any string: one that isn't a valid id reads as the empty id.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::try_from(String::deserialize(deserializer)?).unwrap_or_default())
    }
}

impl Deref for RecordId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for RecordId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for RecordId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for RecordId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for RecordId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_valid_ids_parse() {
        let id: RecordId = "tab_1".parse().unwrap();
        assert_eq!(id, "tab_1");
        assert_eq!(String::from(id), "tab_1");
        for text in ["", "..", "a/b", "a b", "é"] {
            assert_eq!(text.parse::<RecordId>(), Err(InvalidId), "{text}");
        }
    }

    #[test]
    fn invalid_ids_read_as_the_empty_id() {
        let id: RecordId = serde_json::from_str(r#""../x""#).unwrap();
        assert!(id.is_empty());
        let id: RecordId = serde_json::from_str(r#""a-1""#).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), r#""a-1""#);
        assert!(serde_json::from_str::<RecordId>("7").is_err());
    }
}
