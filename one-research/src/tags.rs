use std::collections::{BTreeSet, HashMap};

/// URL → list of tag names. Tag names are stored in lowercase, kebab-case-ish
/// to keep filtering consistent regardless of how the user typed them.
pub type ItemTags = HashMap<String, Vec<String>>;

pub fn normalize(tag: &str) -> String {
  tag.trim().to_lowercase()
}

pub fn add(tags: &mut ItemTags, url: &str, tag: String) {
  let tag = normalize(&tag);
  if tag.is_empty() {
    return;
  }
  let entry = tags.entry(url.to_string()).or_default();
  if !entry.contains(&tag) {
    entry.push(tag);
    entry.sort();
  }
}

pub fn remove(tags: &mut ItemTags, url: &str, tag: &str) {
  let tag = normalize(tag);
  if let Some(list) = tags.get_mut(url) {
    list.retain(|t| t != &tag);
    if list.is_empty() {
      tags.remove(url);
    }
  }
}

pub fn for_url<'a>(tags: &'a ItemTags, url: &str) -> &'a [String] {
  tags.get(url).map(|v| v.as_slice()).unwrap_or(&[])
}

/// Sorted unique list of every tag that exists across all items.
pub fn all_tags(tags: &ItemTags) -> Vec<String> {
  let mut set: BTreeSet<String> = BTreeSet::new();
  for list in tags.values() {
    for t in list {
      set.insert(t.clone());
    }
  }
  set.into_iter().collect()
}

/// Number of items carrying a given tag.
pub fn count_for(tags: &ItemTags, tag: &str) -> usize {
  tags.values().filter(|list| list.iter().any(|t| t == tag)).count()
}
