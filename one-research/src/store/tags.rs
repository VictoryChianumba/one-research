use std::path::PathBuf;

use crate::tags::ItemTags;

pub fn load() -> ItemTags {
  let Some(path) = path() else { return ItemTags::default() };
  super::load_json(&path, "one-research/tags")
}

pub fn save(tags: &ItemTags) {
  if let Some(path) = path() {
    super::save_json(tags, &path, "one-research/tags");
  }
}

fn path() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/one-research/tags.json");
  Some(p)
}
