//! Domain types for snags, independent of how Padagonia stores them.

use clap::ValueEnum;
use serde::Serialize;
use std::fmt;

/// How bad a snag is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "medium" => Self::Medium,
            "high" => Self::High,
            "critical" => Self::Critical,
            _ => Self::Low,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Current status of a snag, derived from its most recent status event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Resolved,
}

impl Status {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A trace or log captured alongside a snag.
#[derive(Debug, Clone, Serialize)]
pub struct Attachment {
    pub id: u64,
    pub kind: AttachmentKind,
    pub filename: String,
    pub content: String,
    pub captured_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AttachmentKind {
    Trace,
    Log,
}

impl AttachmentKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Log => "log",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "trace" => Self::Trace,
            _ => Self::Log,
        }
    }
}

impl fmt::Display for AttachmentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A status change event recorded against a snag (resolve / reopen).
#[derive(Debug, Clone, Serialize)]
pub struct StatusEvent {
    pub status: Status,
    pub note: String,
    pub actor: String,
    pub at: u64,
}

/// Fully hydrated snag, as read back from the store for display.
#[derive(Debug, Clone, Serialize)]
pub struct Snag {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub tags: Vec<String>,
    pub reporter: String,
    pub created_at: u64,
    pub status: Status,
    pub last_event: Option<StatusEvent>,
    pub attachment_count: usize,
}
