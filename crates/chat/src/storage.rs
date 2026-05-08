use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

use crate::{ChatIndex, ChatSession, ChatSessionMeta};

/// Crash-safe write: writes to `<path>.tmp`, fsyncs, then renames onto `path`.
/// A panic, SIGINT, or power loss either leaves the original unchanged or
/// replaces it atomically — never truncated.
fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
  let mut tmp_name: OsString = path.as_os_str().to_owned();
  tmp_name.push(".tmp");
  let tmp_path = PathBuf::from(tmp_name);
  let _ = fs::remove_file(&tmp_path);
  {
    let mut f = fs::File::create(&tmp_path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
  }
  fs::rename(&tmp_path, path)
}

/// On parse failure, rename `<path>` to `<path>.broken-<unix_ts>` so the next
/// save doesn't clobber the user's only recovery copy. Best-effort.
/// Mirrors `trench::store::quarantine_corrupted`. Unique nanos+pid+counter
/// suffix prevents same-second collisions (audit Rel HIGH H2); rename
/// failure is logged loudly so the user knows the recovery copy will
/// be overwritten on the next save.
fn quarantine_corrupted(
  path: &Path,
  label: &str,
  err: &dyn std::fmt::Display,
) {
  use std::sync::atomic::{AtomicU64, Ordering};
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let ts_nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let pid = std::process::id();
  let n = COUNTER.fetch_add(1, Ordering::Relaxed);
  let mut new_name: OsString = path.as_os_str().to_owned();
  new_name.push(format!(".broken-{ts_nanos}-{pid}-{n}"));
  let new_path = PathBuf::from(&new_name);
  log::error!(
    "{label}: parse failed at {} — quarantining to {}: {err}",
    path.display(),
    new_path.display()
  );
  if let Err(rename_err) = fs::rename(path, &new_path) {
    log::error!(
      "{label}: rename to quarantine path failed — corrupted file \
       remains at {} and will be overwritten on the next save: \
       {rename_err}",
      path.display()
    );
  }
}

fn chats_dir() -> PathBuf {
  let base = dirs::config_dir()
    .unwrap_or_else(|| PathBuf::from("."))
    .join("trench")
    .join("chats");
  base
}

fn index_path() -> PathBuf {
  chats_dir().join("index.json")
}

fn session_path(id: &str) -> PathBuf {
  chats_dir().join(format!("{id}.json"))
}

/// Reject session IDs that would escape the chats directory or collide
/// with system-special filenames. Defense-in-depth in front of every
/// disk-touching entry point — without this gate, an imported `index.json`
/// or `session.json` with `id = "../../etc/foo"` would resolve to a path
/// outside `chats_dir()`.
fn validate_id(id: &str) -> Result<(), anyhow::Error> {
  if !crate::sanitize::is_safe_id(id) {
    return Err(anyhow::anyhow!(
      "rejected unsafe chat session id: {id:?}"
    ));
  }
  Ok(())
}

fn ensure_dir() -> Result<()> {
  let dir = chats_dir();
  if !dir.exists() {
    fs::create_dir_all(&dir)?;
  }
  Ok(())
}

pub fn load_index() -> ChatIndex {
  let path = index_path();
  if !path.exists() {
    return ChatIndex {
      sessions: Vec::new(),
      default_provider: "claude".to_string(),
    };
  }
  let data = fs::read_to_string(&path).unwrap_or_default();
  match serde_json::from_str(&data) {
    Ok(v) => v,
    Err(e) => {
      quarantine_corrupted(&path, "trench/chat/index", &e);
      ChatIndex {
        sessions: Vec::new(),
        default_provider: "claude".to_string(),
      }
    }
  }
}

pub fn save_index(index: &ChatIndex) -> Result<()> {
  ensure_dir()?;
  let data = serde_json::to_string(index)?;
  atomic_write(&index_path(), data.as_bytes())?;
  Ok(())
}

pub fn load_session(id: &str) -> Option<ChatSession> {
  if validate_id(id).is_err() {
    log::warn!("chat::load_session: rejected unsafe id {id:?}");
    return None;
  }
  let path = session_path(id);
  if !path.exists() {
    return None;
  }
  let data = fs::read_to_string(&path).ok()?;
  match serde_json::from_str(&data) {
    Ok(v) => Some(v),
    Err(e) => {
      quarantine_corrupted(&path, "trench/chat/session", &e);
      None
    }
  }
}

pub fn save_session(session: &ChatSession) -> Result<()> {
  validate_id(&session.id)?;
  ensure_dir()?;
  let data = serde_json::to_string(session)?;
  atomic_write(&session_path(&session.id), data.as_bytes())?;
  Ok(())
}

pub fn delete_session(id: &str) -> Result<()> {
  validate_id(id)?;
  let path = session_path(id);
  if path.exists() {
    fs::remove_file(path)?;
  }
  Ok(())
}

pub fn create_session(title: String, provider: Option<String>) -> ChatSession {
  let now = Utc::now();
  ChatSession {
    id: Uuid::new_v4().to_string(),
    title,
    created_at: now,
    updated_at: now,
    messages: Vec::new(),
    provider,
    total_input_tokens: 0,
    total_output_tokens: 0,
  }
}

pub fn session_to_meta(session: &ChatSession) -> ChatSessionMeta {
  ChatSessionMeta {
    id: session.id.clone(),
    title: session.title.clone(),
    created_at: session.created_at,
    updated_at: session.updated_at,
    provider: session.provider.clone(),
  }
}
