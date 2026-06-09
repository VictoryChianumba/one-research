use std::path::PathBuf;

use crate::discovery::SessionHistory;

pub fn load() -> SessionHistory {
  let Some(path) = path() else { return SessionHistory::default() };
  super::load_json(&path, "one-research/discovery_session")
}

pub fn save(session: &SessionHistory) {
  if let Some(path) = path() {
    super::save_json(session, &path, "one-research/discovery_session");
  }
}

pub fn clear() {
  // SEAM-EXEMPT: `clear()` writes a literal `{}` byte sequence to wipe
  // the session without going through serde_json. `save_json` would also
  // work, but this path is structurally an erase-to-empty, not a save —
  // keeping the raw atomic_write call surfaces that intent.
  if let Some(path) = path()
    && let Err(e) = super::atomic_write(&path, b"{}")
  {
    log::error!(
      "one-research/discovery_session: clear failed at {}: {e}",
      path.display()
    );
  }
}

fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/one-research/discovery_session.json");
  Some(p)
}
