//! Storage layer: maps snags/attachments/status events onto a Padagonia graph.
//!
//! Padagonia nodes and edges are append-only, so a status change (resolve /
//! reopen) is recorded as a new `StatusEvent` node linked from the snag via a
//! `status_change` edge, rather than mutating the snag in place. The current
//! status is whichever event has the latest `at` timestamp.

use anyhow::{bail, Context, Result};
use padagonia::{KeyId, Node, NodeId, Provenance, QueryEngine, Scalar, Store, StringTableExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{Attachment, AttachmentKind, Severity, Snag, Status, StatusEvent};

const LABEL_SNAG: &str = "Snag";
const LABEL_ATTACHMENT: &str = "Attachment";
const LABEL_STATUS_EVENT: &str = "StatusEvent";
const REL_HAS_ATTACHMENT: &str = "has_attachment";
const REL_STATUS_CHANGE: &str = "status_change";

const PROP_TITLE: &str = "title";
const PROP_DESCRIPTION: &str = "description";
const PROP_SEVERITY: &str = "severity";
const PROP_TAG: &str = "tag";
const PROP_REPORTER: &str = "reporter";

const PROP_KIND: &str = "kind";
const PROP_FILENAME: &str = "filename";
const PROP_CONTENT: &str = "content";
const PROP_CAPTURED_AT: &str = "captured_at";

const PROP_STATUS: &str = "status";
const PROP_NOTE: &str = "note";
const PROP_ACTOR: &str = "actor";
const PROP_AT: &str = "at";

const AGENT: &str = "snags";
const MODEL: &str = env!("CARGO_PKG_VERSION");

/// Fields for a newly-recorded snag.
pub struct NewSnag {
    pub title: String,
    pub description: String,
    pub severity: Severity,
    pub tags: Vec<String>,
    pub reporter: String,
    pub attachments: Vec<NewAttachment>,
}

/// A trace/log to attach while recording a snag.
pub struct NewAttachment {
    pub kind: AttachmentKind,
    pub filename: String,
    pub content: String,
}

/// A Padagonia store holding snags, their attachments, and status history.
pub struct Repo {
    store: Store,
    path: PathBuf,
}

impl Repo {
    /// Open an existing store or start an empty one at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let store = if path.exists() {
            Store::load(&path).with_context(|| format!("loading store at {}", path.display()))?
        } else {
            Store::new()
        };
        Ok(Self { store, path })
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
        }
        self.store
            .save(&self.path)
            .with_context(|| format!("saving store at {}", self.path.display()))
    }

    /// Record a new snag (and any attachments) and persist immediately.
    pub fn record(&mut self, new: NewSnag) -> Result<u64> {
        let now = now_unix();
        let mut props: Vec<(&str, Scalar)> = vec![
            (PROP_TITLE, Scalar::String(new.title)),
            (PROP_DESCRIPTION, Scalar::String(new.description)),
            (
                PROP_SEVERITY,
                Scalar::String(new.severity.as_str().to_string()),
            ),
            (PROP_REPORTER, Scalar::String(new.reporter)),
        ];
        for tag in &new.tags {
            props.push((PROP_TAG, Scalar::String(tag.clone())));
        }
        let prov = Provenance::new(AGENT, MODEL, 1.0, 0.0, now, Vec::new());
        let snag_id = self.store.add_node(LABEL_SNAG, props, None, prov);

        for attachment in new.attachments {
            self.add_attachment(snag_id, attachment, now);
        }

        self.save()?;
        Ok(snag_id.0)
    }

    fn add_attachment(&mut self, snag_id: NodeId, attachment: NewAttachment, now: u64) {
        let props = vec![
            (
                PROP_KIND,
                Scalar::String(attachment.kind.as_str().to_string()),
            ),
            (PROP_FILENAME, Scalar::String(attachment.filename)),
            (PROP_CONTENT, Scalar::String(attachment.content)),
            (PROP_CAPTURED_AT, Scalar::Timestamp(now)),
        ];
        let prov = Provenance::new(AGENT, MODEL, 1.0, 0.0, now, Vec::new());
        let attachment_id = self.store.add_node(LABEL_ATTACHMENT, props, None, prov);

        let edge_prov = Provenance::new(AGENT, MODEL, 1.0, 0.0, now, Vec::new());
        self.store.add_edge(
            snag_id,
            attachment_id,
            REL_HAS_ATTACHMENT,
            Vec::new(),
            None,
            edge_prov,
        );
    }

    /// Attachments (traces/logs) captured against a snag, oldest first.
    #[must_use]
    pub fn attachments(&self, id: u64) -> Vec<Attachment> {
        self.attachments_of(NodeId(id))
    }

    fn attachments_of(&self, snag_id: NodeId) -> Vec<Attachment> {
        let Some(rel) = self.store.string_table().relation_id(REL_HAS_ATTACHMENT) else {
            return Vec::new();
        };
        let engine = QueryEngine::new(&self.store);
        let mut attachments: Vec<Attachment> = engine
            .outgoing(snag_id, Some(rel))
            .into_iter()
            .filter_map(|edge| self.store.nodes().get(&edge.dst))
            .filter_map(|node| self.attachment_from_node(node))
            .collect();
        attachments.sort_by_key(|a| a.captured_at);
        attachments
    }

    /// All snags, optionally filtered to a single status, newest-critical-first.
    #[must_use]
    pub fn list(&self, status_filter: Option<Status>) -> Vec<Snag> {
        let Some(label) = self.store.string_table().label_id(LABEL_SNAG) else {
            return Vec::new();
        };
        let engine = QueryEngine::new(&self.store);
        let mut snags: Vec<Snag> = engine
            .by_label(label)
            .into_iter()
            .filter_map(|node| self.snag_from_node(node))
            .filter(|snag| status_filter.is_none_or(|f| snag.status == f))
            .collect();
        snags.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then(a.created_at.cmp(&b.created_at))
        });
        snags
    }

    /// A single snag by id.
    #[must_use]
    pub fn get(&self, id: u64) -> Option<Snag> {
        let node = self.store.nodes().get(&NodeId(id))?;
        self.snag_from_node(node)
    }

    /// Mark a snag resolved. Errors if it is already resolved.
    pub fn resolve(&mut self, id: u64, note: String, actor: String) -> Result<()> {
        let snag = self.get(id).with_context(|| format!("no snag #{id}"))?;
        if snag.status == Status::Resolved {
            bail!("snag #{id} is already resolved");
        }
        self.append_status_event(NodeId(id), Status::Resolved, note, actor)
    }

    /// Reopen a previously-resolved snag. Errors if it is already open.
    pub fn reopen(&mut self, id: u64, note: String, actor: String) -> Result<()> {
        let snag = self.get(id).with_context(|| format!("no snag #{id}"))?;
        if snag.status == Status::Open {
            bail!("snag #{id} is already open");
        }
        self.append_status_event(NodeId(id), Status::Open, note, actor)
    }

    fn append_status_event(
        &mut self,
        snag_id: NodeId,
        status: Status,
        note: String,
        actor: String,
    ) -> Result<()> {
        let now = now_unix();
        let props = vec![
            (PROP_STATUS, Scalar::String(status.as_str().to_string())),
            (PROP_NOTE, Scalar::String(note)),
            (PROP_ACTOR, Scalar::String(actor)),
            (PROP_AT, Scalar::Timestamp(now)),
        ];
        let prov = Provenance::new(AGENT, MODEL, 1.0, 0.0, now, Vec::new());
        let event_id = self.store.add_node(LABEL_STATUS_EVENT, props, None, prov);

        let edge_prov = Provenance::new(AGENT, MODEL, 1.0, 0.0, now, Vec::new());
        self.store.add_edge(
            snag_id,
            event_id,
            REL_STATUS_CHANGE,
            Vec::new(),
            None,
            edge_prov,
        );
        self.save()
    }

    fn last_event(&self, snag_id: NodeId) -> Option<StatusEvent> {
        let rel = self.store.string_table().relation_id(REL_STATUS_CHANGE)?;
        let engine = QueryEngine::new(&self.store);
        engine
            .outgoing(snag_id, Some(rel))
            .into_iter()
            .filter_map(|edge| self.store.nodes().get(&edge.dst))
            .filter_map(|node| self.status_event_from_node(node))
            .max_by_key(|event| event.at)
    }

    fn snag_from_node(&self, node: &Node) -> Option<Snag> {
        let title = self.prop_string(&node.properties, PROP_TITLE)?;
        let description = self
            .prop_string(&node.properties, PROP_DESCRIPTION)
            .unwrap_or_default();
        let severity = self
            .prop_string(&node.properties, PROP_SEVERITY)
            .map_or(Severity::Low, |s| Severity::parse(&s));
        let tags = self.prop_strings(&node.properties, PROP_TAG);
        let reporter = self
            .prop_string(&node.properties, PROP_REPORTER)
            .unwrap_or_default();
        let last_event = self.last_event(node.id);
        let status = last_event.as_ref().map_or(Status::Open, |e| e.status);
        let attachment_count = self.attachments_of(node.id).len();
        Some(Snag {
            id: node.id.0,
            title,
            description,
            severity,
            tags,
            reporter,
            created_at: node.created_at,
            status,
            last_event,
            attachment_count,
        })
    }

    fn attachment_from_node(&self, node: &Node) -> Option<Attachment> {
        let kind = self
            .prop_string(&node.properties, PROP_KIND)
            .map(|s| AttachmentKind::parse(&s))?;
        let filename = self
            .prop_string(&node.properties, PROP_FILENAME)
            .unwrap_or_default();
        let content = self
            .prop_string(&node.properties, PROP_CONTENT)
            .unwrap_or_default();
        let captured_at = self
            .prop_timestamp(&node.properties, PROP_CAPTURED_AT)
            .unwrap_or(node.created_at);
        Some(Attachment {
            id: node.id.0,
            kind,
            filename,
            content,
            captured_at,
        })
    }

    fn status_event_from_node(&self, node: &Node) -> Option<StatusEvent> {
        let status = self.prop_string(&node.properties, PROP_STATUS)?;
        let status = if status == "resolved" {
            Status::Resolved
        } else {
            Status::Open
        };
        let note = self
            .prop_string(&node.properties, PROP_NOTE)
            .unwrap_or_default();
        let actor = self
            .prop_string(&node.properties, PROP_ACTOR)
            .unwrap_or_default();
        let at = self
            .prop_timestamp(&node.properties, PROP_AT)
            .unwrap_or(node.created_at);
        Some(StatusEvent {
            status,
            note,
            actor,
            at,
        })
    }

    fn prop_string(&self, props: &[(KeyId, Scalar)], key: &str) -> Option<String> {
        let key_id = self.store.string_table().key_id(key)?;
        props.iter().find(|(k, _)| *k == key_id).and_then(|(_, v)| match v {
            Scalar::String(s) => Some(s.clone()),
            _ => None,
        })
    }

    fn prop_strings(&self, props: &[(KeyId, Scalar)], key: &str) -> Vec<String> {
        let Some(key_id) = self.store.string_table().key_id(key) else {
            return Vec::new();
        };
        props
            .iter()
            .filter(|(k, _)| *k == key_id)
            .filter_map(|(_, v)| match v {
                Scalar::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    fn prop_timestamp(&self, props: &[(KeyId, Scalar)], key: &str) -> Option<u64> {
        let key_id = self.store.string_table().key_id(key)?;
        props.iter().find(|(k, _)| *k == key_id).and_then(|(_, v)| match v {
            Scalar::Timestamp(t) => Some(*t),
            _ => None,
        })
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
