//! Serde helpers for the lenient, forward-compatible format.

use crate::text::Eol;
use serde::de::IgnoredAny;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// A value of the expected shape, or anything else.
#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient<T> {
    Valid(T),
    Invalid(IgnoredAny),
}

impl<T> Lenient<T> {
    fn valid(self) -> Option<T> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Invalid(_) => None,
        }
    }
}

/// Deserializes `T`, or gives its default when the value has another shape.
pub fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Lenient::deserialize(deserializer)?
        .valid()
        .unwrap_or_default())
}

/// Deserializes a list, skipping elements of another shape; anything but a list is empty.
pub fn lenient_seq<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let items: Vec<Lenient<T>> = lenient(deserializer)?;
    Ok(items.into_iter().filter_map(Lenient::valid).collect())
}

/// A path as text, or as raw bytes when it isn't UTF-8.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum PathRepr<'a> {
    Text(Cow<'a, str>),
    Bytes { bytes: Cow<'a, [u8]> },
}

impl<'a> From<&'a Path> for PathRepr<'a> {
    fn from(path: &'a Path) -> Self {
        match path.to_str() {
            Some(text) => Self::Text(Cow::Borrowed(text)),
            None => Self::Bytes {
                bytes: Cow::Borrowed(path.as_os_str().as_bytes()),
            },
        }
    }
}

impl From<PathRepr<'_>> for PathBuf {
    fn from(repr: PathRepr<'_>) -> Self {
        match repr {
            PathRepr::Text(text) => PathBuf::from(text.into_owned()),
            PathRepr::Bytes { bytes } => PathBuf::from(OsString::from_vec(bytes.into_owned())),
        }
    }
}

pub mod opt_path {
    use super::*;

    #[expect(clippy::ref_option, reason = "serde's `serialize_with` signature")]
    pub fn serialize<S: Serializer>(
        path: &Option<PathBuf>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        path.as_deref().map(PathRepr::from).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<PathBuf>, D::Error> {
        let repr: Option<PathRepr<'_>> = lenient(deserializer)?;
        Ok(repr.map(PathBuf::from))
    }
}

pub mod path_seq {
    use super::*;

    pub fn serialize<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(paths.iter().map(|path| PathRepr::from(path.as_path())))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<PathBuf>, D::Error> {
        let reprs: Vec<PathRepr<'_>> = lenient_seq(deserializer)?;
        Ok(reprs.into_iter().map(PathBuf::from).collect())
    }
}

pub mod opt_eol {
    use super::*;

    #[expect(
        clippy::ref_option,
        clippy::trivially_copy_pass_by_ref,
        reason = "serde's `serialize_with` signature"
    )]
    pub fn serialize<S: Serializer>(eol: &Option<Eol>, serializer: S) -> Result<S::Ok, S::Error> {
        eol.map(Eol::label).serialize(serializer)
    }

    /// An unknown label reads as `None`.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Eol>, D::Error> {
        let label: Option<String> = lenient(deserializer)?;
        Ok(label.and_then(|label| Eol::ALL.into_iter().find(|eol| eol.label() == label)))
    }
}
