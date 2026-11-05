//! Errors the user reads, apart from those that only say something failed.
//!
//! A [`Shown`] error, or a JSON file that does not parse, carries a message meant for the
//! panel. Any other error only names its kind there: the panel says collection failed, without
//! details that could hold private data.

use std::fmt;

/// An error whose message the panel shows as it is.
#[derive(Debug)]
pub struct Shown(pub String);

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Shown {}

/// An error the panel names only by its kind, such as a subprocess timing out.
#[derive(Debug)]
pub struct Unexpected(pub &'static str);

impl fmt::Display for Unexpected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Unexpected {}

/// A [`Shown`] error, formatted.
#[macro_export]
macro_rules! shown {
    ($($arg:tt)*) => { ::anyhow::Error::new($crate::error::Shown(format!($($arg)*))) };
}

/// Whether the error's message is meant for the user.
pub fn is_shown(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Shown>().is_some() || error.downcast_ref::<serde_json::Error>().is_some()
}

/// The text the panel shows for a failed collection.
pub fn collection_message(error: &anyhow::Error) -> String {
    if is_shown(error) {
        return error.to_string();
    }
    let kind = if let Some(io) = error.downcast_ref::<std::io::Error>() {
        match io.kind() {
            std::io::ErrorKind::NotFound => "FileNotFoundError",
            std::io::ErrorKind::PermissionDenied => "PermissionError",
            _ => "OSError",
        }
    } else if let Some(unexpected) = error.downcast_ref::<Unexpected>() {
        unexpected.0
    } else {
        "Error"
    };
    format!("{kind}: collection failed; previous selection kept")
}
