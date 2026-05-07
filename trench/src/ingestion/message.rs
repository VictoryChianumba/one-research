use crate::models::FeedItem;

pub enum FetchMessage {
  Items(Vec<FeedItem>),
  EnrichedItems(Vec<FeedItem>),
  SourceComplete(String),
  SourceError(String, String),
  AllComplete,
}
