use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Note;

fn notes_dir() -> PathBuf {
  // Memoize + log-once on the rare $HOME-unset fallback, so notes landing
  // in a random CWD (CI, container, init-system) is observable instead of
  // silent.
  static CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
  CACHE
    .get_or_init(|| match dirs::config_dir() {
      Some(p) => p.join("one-research").join("notes"),
      None => {
        log::error!(
          "notes::notes_dir: dirs::config_dir() returned None — falling \
           back to ./one-research/notes. Set HOME or XDG_CONFIG_HOME to avoid \
           writing notes into the launching directory."
        );
        PathBuf::from(".").join("one-research").join("notes")
      }
    })
    .clone()
}

fn note_path(note_id: &str) -> PathBuf {
  notes_dir().join(format!("{note_id}.json"))
}

/// Reject note IDs that would escape the notes directory or collide with
/// system-special filenames. Defense-in-depth in front of every disk-
/// touching entry point — without this gate, an imported `*.json` whose
/// `note_id` field carried `"../../etc/foo"` would resolve to a path
/// outside `notes_dir()`. See `crate::sanitize` for rationale.
fn validate_id(note_id: &str) -> anyhow::Result<()> {
  if !crate::sanitize::is_safe_id(note_id) {
    anyhow::bail!("rejected unsafe note id: {note_id:?}");
  }
  Ok(())
}

/// Crash-safe write: writes to `<path>.tmp`, fsyncs, then renames onto `path`.
/// A panic, SIGINT, or power loss either leaves the original unchanged or
/// replaces it atomically — never truncated. Notes are user-authored content,
/// so torn writes are particularly costly here.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
  let mut tmp_name: OsString = path.as_os_str().to_owned();
  tmp_name.push(".tmp");
  let tmp_path = PathBuf::from(tmp_name);
  let _ = std::fs::remove_file(&tmp_path);
  {
    let mut f = std::fs::File::create(&tmp_path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
  }
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(
      &tmp_path,
      std::fs::Permissions::from_mode(0o600),
    );
  }
  std::fs::rename(&tmp_path, path)
}

/// Move a corrupt note file aside (`<path>.broken-<unix_nanos>-<pid>`) so
/// the user keeps a recovery copy and the next save_note doesn't overwrite
/// it. Mirrors the one-research-side `quarantine_corrupted` helper. Best-effort:
/// rename failure is non-fatal.
fn quarantine_corrupt_note(path: &Path, err: &dyn std::fmt::Display) {
  use std::sync::atomic::{AtomicU64, Ordering};
  static COUNTER: AtomicU64 = AtomicU64::new(0);
  let ts_nanos = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_nanos())
    .unwrap_or(0);
  let pid = std::process::id();
  let n = COUNTER.fetch_add(1, Ordering::Relaxed);
  let mut new_name: std::ffi::OsString = path.as_os_str().to_owned();
  new_name.push(format!(".broken-{ts_nanos}-{pid}-{n}"));
  let new_path = PathBuf::from(&new_name);
  log::error!(
    "notes: parse failed at {} — quarantining to {}: {err}",
    path.display(),
    new_path.display()
  );
  if let Err(rename_err) = std::fs::rename(path, &new_path) {
    log::error!(
      "notes: quarantine rename failed at {} — corrupt file remains \
       and may be overwritten on next save: {rename_err}",
      path.display()
    );
  }
}

/// Cap on per-file load size for notes. Defends against an attacker-planted
/// 4-GB JSON OOMing one-research at startup. Notes are user-authored, so the
/// legit upper bound is small; 8 MB is generous.
const MAX_NOTE_BYTES: u64 = 8 * 1024 * 1024;

fn read_capped_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
  use std::io::Read;
  let f = std::fs::File::open(path)?;
  let mut buf = Vec::new();
  f.take(MAX_NOTE_BYTES + 1).read_to_end(&mut buf)?;
  if buf.len() as u64 > MAX_NOTE_BYTES {
    return Err(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      format!("file exceeds {} MB cap", MAX_NOTE_BYTES / 1024 / 1024),
    ));
  }
  Ok(buf)
}

pub fn load_all_notes() -> anyhow::Result<Vec<Note>> {
  let dir = notes_dir();
  if !dir.exists() {
    return Ok(Vec::new());
  }
  let mut notes = Vec::new();
  for entry in std::fs::read_dir(&dir)? {
    let entry = entry?;
    let path = entry.path();
    if path.extension().and_then(|e| e.to_str()) == Some("json") {
      match read_capped_bytes(&path) {
        Err(e) => {
          log::warn!("Failed to read note at {}: {e}", path.display());
        }
        Ok(bytes) => match serde_json::from_slice::<Note>(&bytes) {
          Ok(note) => {
            // Reject deserialized notes whose `note_id` field would resolve
            // outside notes_dir on the next save_note call. The deserialize
            // succeeded but the note is unsafe to round-trip through disk.
            if validate_id(&note.note_id).is_err() {
              log::warn!(
                "Skipping note with unsafe id at {}: {:?}",
                path.display(),
                note.note_id
              );
              continue;
            }
            notes.push(note);
          }
          Err(parse_err) => {
            // Quarantine the corrupt file so the user keeps a recovery
            // copy and the next save_note doesn't overwrite it.
            quarantine_corrupt_note(&path, &parse_err);
          }
        },
      }
    }
  }
  Ok(notes)
}

pub fn load_note(note_id: &str) -> Option<Note> {
  if validate_id(note_id).is_err() {
    log::warn!("notes::load_note: rejected unsafe id {note_id:?}");
    return None;
  }
  let path = note_path(note_id);
  let bytes = read_capped_bytes(&path).ok()?;
  serde_json::from_slice(&bytes).ok()
}

pub fn save_note(note: &Note) -> anyhow::Result<()> {
  validate_id(&note.note_id)?;
  let dir = notes_dir();
  std::fs::create_dir_all(&dir)?;
  let bytes = serde_json::to_vec(note)?;
  atomic_write(&note_path(&note.note_id), &bytes)?;
  Ok(())
}

pub fn delete_note(note_id: &str) -> anyhow::Result<()> {
  validate_id(note_id)?;
  let path = note_path(note_id);
  if path.exists() {
    std::fs::remove_file(&path)?;
  }
  Ok(())
}
