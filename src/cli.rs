//! Command-line surface for snags.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};

use crate::model::{AttachmentKind, Severity, Status};
use crate::repo::{NewAttachment, NewSnag, Repo};
use crate::report;

#[derive(Parser)]
#[command(
    name = "snags",
    version,
    about = "Padagonia-backed issues-encountered index for agents and users"
)]
pub struct Cli {
    /// Path to the snags store [default: .snags/store.pad, or $SNAGS_STORE]
    #[arg(long, global = true)]
    store: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Record a new snag, optionally attaching traces and logs
    Record {
        /// Short summary of the issue
        #[arg(short, long)]
        title: String,
        /// Longer explanation of what happened
        #[arg(short, long, default_value = "")]
        description: String,
        #[arg(short, long, value_enum, default_value_t = Severity::Medium)]
        severity: Severity,
        /// Repeatable free-form tag
        #[arg(short = 'g', long = "tag")]
        tags: Vec<String>,
        /// Who or what is reporting this snag
        #[arg(short, long, default_value = "unknown")]
        reporter: String,
        /// Path to a trace file to attach (repeatable)
        #[arg(long = "trace")]
        traces: Vec<PathBuf>,
        /// Path to a log file to attach (repeatable)
        #[arg(long = "log")]
        logs: Vec<PathBuf>,
    },
    /// List snags
    List {
        #[arg(long, value_enum, default_value_t = StatusFilter::All)]
        status: StatusFilter,
        /// Print as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Report all outstanding (open) snags
    Report {
        /// Print as JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Show full detail for one snag, including its attachments
    Show {
        id: u64,
        /// Print full attachment contents, not just their names
        #[arg(long)]
        full: bool,
        /// Print as JSON instead of plain text
        #[arg(long)]
        json: bool,
    },
    /// Mark a snag resolved
    Resolve {
        id: u64,
        #[arg(long, default_value = "")]
        note: String,
        #[arg(long, default_value = "unknown")]
        actor: String,
    },
    /// Reopen a previously-resolved snag
    Reopen {
        id: u64,
        #[arg(long, default_value = "")]
        note: String,
        #[arg(long, default_value = "unknown")]
        actor: String,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum StatusFilter {
    Open,
    Resolved,
    All,
}

impl StatusFilter {
    const fn to_status(self) -> Option<Status> {
        match self {
            Self::Open => Some(Status::Open),
            Self::Resolved => Some(Status::Resolved),
            Self::All => None,
        }
    }
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let path = store_path(cli.store.as_deref());

    match cli.command {
        Command::Record {
            title,
            description,
            severity,
            tags,
            reporter,
            traces,
            logs,
        } => {
            let mut attachments = Vec::with_capacity(traces.len() + logs.len());
            for path in &traces {
                attachments.push(read_attachment(AttachmentKind::Trace, path)?);
            }
            for path in &logs {
                attachments.push(read_attachment(AttachmentKind::Log, path)?);
            }

            let mut repo = Repo::open(&path)?;
            let id = repo.record(NewSnag {
                title,
                description,
                severity,
                tags,
                reporter,
                attachments,
            })?;
            println!("recorded snag #{id}");
        }
        Command::List { status, json } => {
            let repo = Repo::open(&path)?;
            let snags = repo.list(status.to_status());
            if json {
                report::print_snags_json(&snags)?;
            } else {
                report::print_table(&snags);
            }
        }
        Command::Report { json } => {
            let repo = Repo::open(&path)?;
            let snags = repo.list(Some(Status::Open));
            if json {
                report::print_snags_json(&snags)?;
            } else {
                report::print_report(&snags);
            }
        }
        Command::Show { id, full, json } => {
            let repo = Repo::open(&path)?;
            let snag = repo.get(id).with_context(|| format!("no snag #{id}"))?;
            let attachments = repo.attachments(id);
            if json {
                report::print_show_json(&snag, &attachments)?;
            } else {
                report::print_show(&snag, &attachments, full);
            }
        }
        Command::Resolve { id, note, actor } => {
            let mut repo = Repo::open(&path)?;
            repo.resolve(id, note, actor)?;
            println!("resolved snag #{id}");
        }
        Command::Reopen { id, note, actor } => {
            let mut repo = Repo::open(&path)?;
            repo.reopen(id, note, actor)?;
            println!("reopened snag #{id}");
        }
    }

    Ok(())
}

fn store_path(explicit: Option<&Path>) -> PathBuf {
    explicit.map(Path::to_path_buf).unwrap_or_else(|| {
        std::env::var_os("SNAGS_STORE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".snags/store.pad"))
    })
}

fn read_attachment(kind: AttachmentKind, path: &Path) -> Result<NewAttachment> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let filename = path
        .file_name()
        .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    Ok(NewAttachment {
        kind,
        filename,
        content,
    })
}
