#[derive(
  Debug,
  Clone,
  Copy,
  PartialEq,
  Eq,
  Default,
  serde::Serialize,
  serde::Deserialize,
)]
pub enum QueryIntent {
  #[default]
  FindPapers,
  LiteratureReview,
  SotaLookup,
  ReadingList,
  CodeSearch,
  Compare,
  Digest,
  AuthorSearch,
  Trending,
}

impl QueryIntent {
  pub fn label(self) -> &'static str {
    match self {
      Self::FindPapers => "papers",
      Self::LiteratureReview => "lit review",
      Self::SotaLookup => "sota",
      Self::ReadingList => "reading list",
      Self::CodeSearch => "code",
      Self::Compare => "compare",
      Self::Digest => "digest",
      Self::AuthorSearch => "author",
      Self::Trending => "trending",
    }
  }
}

pub fn classify(topic: &str) -> QueryIntent {
  let t = topic.to_lowercase();

  if t.contains("benchmark")
    || t.contains("state of the art")
    || t.contains("sota")
    || t.contains("best model")
    || t.contains("leaderboard")
    || t.contains("beats ")
    || t.contains("performance on")
    || t.contains("score on")
  {
    return QueryIntent::SotaLookup;
  }

  if t.contains("survey")
    || t.contains("overview of")
    || t.contains("review of")
    || t.contains("state of")
    || t.contains("landscape")
    || t.contains("comprehensive")
    || t.contains("what is known")
  {
    return QueryIntent::LiteratureReview;
  }

  if t.contains("how to learn")
    || t.starts_with("learn ")
    || t.contains("getting started")
    || t.contains("beginner")
    || t.contains("roadmap")
    || t.contains("curriculum")
    || t.contains("reading list")
    || t.contains("from scratch")
  {
    return QueryIntent::ReadingList;
  }

  if t.contains("implementation")
    || t.contains("code for")
    || t.contains("github")
    || t.contains(" library")
    || t.contains("pytorch")
    || t.contains("tensorflow")
    || t.contains("how to implement")
  {
    return QueryIntent::CodeSearch;
  }

  if t.contains(" vs ")
    || t.contains(" versus ")
    || t.contains("compare ")
    || t.contains("comparison of")
    || t.contains("difference between")
    || t.contains("vs.")
  {
    return QueryIntent::Compare;
  }

  if t.contains("this week")
    || t.contains("weekly digest")
    || t.contains("latest news")
    || t.contains("recent developments")
    || t.contains("what happened")
    || t.contains("digest")
  {
    return QueryIntent::Digest;
  }

  if t.contains("papers by ")
    || t.contains("work by ")
    || t.contains("research by ")
    || t.contains("'s papers")
    || t.contains("'s research")
    || t.contains("authored by")
  {
    return QueryIntent::AuthorSearch;
  }

  if t.contains("trending")
    || t.contains("popular")
    || t.contains("most cited")
    || t.contains("what's hot")
    || t.contains("getting attention")
    || t.contains("viral")
  {
    return QueryIntent::Trending;
  }

  QueryIntent::FindPapers
}
