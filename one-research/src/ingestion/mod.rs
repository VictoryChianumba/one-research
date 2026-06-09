pub mod arxiv;
pub mod core;
pub mod crossref;
pub mod fulltext;
pub mod huggingface;
pub mod message;
pub mod openreview;
pub mod pipeline;
pub mod registry;
pub mod rss;
pub mod semantic_scholar;

pub(super) fn collapse_whitespace(s: &str) -> String {
  s.split_whitespace().collect::<Vec<_>>().join(" ")
}
