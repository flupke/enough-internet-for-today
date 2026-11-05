//! The panel's zones: each has its sources, its schedule, its collection and its file.

use std::path::{Path, PathBuf};

/// A zone of the panel, collected on its own schedule into `<data>/<key>.json`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Zone {
    /// Slack, every half hour; its selections add up over a day.
    Slack,
    /// The Gmail inbox, every morning.
    Email,
    /// Social media, X for now, every morning.
    Social,
}

impl Zone {
    /// Every zone, in the order the panel shows them.
    pub const ALL: [Self; 3] = [Self::Slack, Self::Email, Self::Social];

    /// The zone's name in its file and the panel's commands.
    pub fn key(self) -> &'static str {
        match self {
            Self::Slack => "slack",
            Self::Email => "email",
            Self::Social => "social",
        }
    }

    /// The source the helper gathers for the zone through MCP; Social's posts come from X.
    pub fn gathered(self) -> Option<&'static str> {
        match self {
            Self::Slack => Some("Slack"),
            Self::Email => Some("Email"),
            Self::Social => None,
        }
    }

    /// The file the zone's state and selection live in, which the panel reads.
    pub fn file(self, data: &Path) -> PathBuf {
        data.join(format!("{}.json", self.key()))
    }
}
