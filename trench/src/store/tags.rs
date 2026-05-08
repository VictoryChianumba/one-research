use std::fs;
use std::path::PathBuf;

use crate::tags::ItemTags;

pub fn load() -> ItemTags {
  let Some(path) = path() else { return ItemTags::default() };
  let Ok(bytes) = fs::read(&path) else { return ItemTags::default() };
  match serde_json::from_slice(&bytes) {
    Ok(v) => v,
    Err(e) => {
      super::quarantine_corrupted(&path, "trench/tags", &e);
      ItemTags::default()
    }
  }
}

pub fn save(tags: &ItemTags) {
  let Some(path) = path() else { return };
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  if let Ok(json) = serde_json::to_vec(tags) {
    if let Err(e) = super::atomic_write(&path, &json) {
      log::error!("trench/tags: atomic_write failed at {}: {e}", path.display());
    }
  }
}

fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/trench/tags.json");
  Some(p)
}
