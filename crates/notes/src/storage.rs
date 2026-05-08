use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Note;

fn notes_dir() -> PathBuf {
  // Memoize + log-once on the rare $HOME-unset fallback, so notes landing
  // in a random CWD (CI, container, init-system) is observable instead of
  // silent (audit Sec MED #9).
  static CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
  CACHE
    .get_or_init(|| match dirs::config_dir() {
      Some(p) => p.join("trench").join("notes"),
      None => {
        log::error!(
          "notes::notes_dir: dirs::config_dir() returned None — falling \
           back to ./trench/notes. Set HOME or XDG_CONFIG_HOME to avoid \
           writing notes into the launching directory."
        );
        PathBuf::from(".").join("trench").join("notes")
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
      if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(note) = serde_json::from_slice::<Note>(&bytes) {
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
        } else {
          log::warn!("Failed to parse note at {}", path.display());
        }
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
  let bytes = std::fs::read(&path).ok()?;
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
