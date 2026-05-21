//! Structured, fuzzy feed search (ADR-012).
//!
//! The search bar text is parsed once into field-scoped terms, free
//! text, and an optional year constraint, then each [`FeedItem`] is
//! scored with a field-weighted fuzzy match. The best match floats to
//! the top — relevance ordering, not just a filter.
//!
//! Grammar (whitespace-separated; double quotes group a value with
//! spaces, e.g. `author:"Yann LeCun"`):
//! - `ti:` / `title:` — term must fuzzy-match the title.
//! - `abs:` / `abstract:` — term must fuzzy-match the summary.
//! - `au:` / `author:` — term must fuzzy-match some author.
//! - `cat:` / `category:` — arXiv subject; `cs.LG` (exact), `cs` (all cs.*).
//! - `year:` / `yr:` — `2024`, `2020-2024`, `>2020`, `>=2020`, `<2024`, `<=2024`.
//! - anything else — free term, matches title, author, or abstract.
//!
//! Field-scoped terms and free terms are conjunctive: every term must
//! match for the item to appear. An empty query is the "no search"
//! state — callers keep their normal [`crate::feed::FeedSortMode`].

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

use crate::models::FeedItem;

// Relative field weights: at equal fuzzy quality a title hit outranks an
// author hit outranks an abstract hit. This is what surfaces the "right"
// paper first when a term could match in several places.
const W_TITLE: i64 = 3;
const W_AUTHOR: i64 = 2;
const W_ABSTRACT: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YearConstraint {
  Exact(i32),
  /// Inclusive on both ends; constructed with `lo <= hi`.
  Range(i32, i32),
  AtLeast(i32),
  AtMost(i32),
}

impl YearConstraint {
  fn satisfied_by(&self, year: i32) -> bool {
    match *self {
      YearConstraint::Exact(y) => year == y,
      YearConstraint::Range(lo, hi) => (lo..=hi).contains(&year),
      YearConstraint::AtLeast(y) => year >= y,
      YearConstraint::AtMost(y) => year <= y,
    }
  }

  fn parse(value: &str) -> Option<YearConstraint> {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix(">=") {
      return rest.trim().parse().ok().map(YearConstraint::AtLeast);
    }
    if let Some(rest) = v.strip_prefix("<=") {
      return rest.trim().parse().ok().map(YearConstraint::AtMost);
    }
    if let Some(rest) = v.strip_prefix('>') {
      return rest
        .trim()
        .parse::<i32>()
        .ok()
        .map(|y| YearConstraint::AtLeast(y + 1));
    }
    if let Some(rest) = v.strip_prefix('<') {
      return rest
        .trim()
        .parse::<i32>()
        .ok()
        .map(|y| YearConstraint::AtMost(y - 1));
    }
    if let Some((lo, hi)) = v.split_once('-') {
      let lo: i32 = lo.trim().parse().ok()?;
      let hi: i32 = hi.trim().parse().ok()?;
      let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
      return Some(YearConstraint::Range(lo, hi));
    }
    v.parse().ok().map(YearConstraint::Exact)
  }
}

/// A parsed search query. Construct with [`Query::parse`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Query {
  pub free: Vec<String>,
  pub title: Vec<String>,
  pub author: Vec<String>,
  pub summary: Vec<String>,
  pub category: Vec<String>,
  pub year: Option<YearConstraint>,
}

impl Query {
  pub fn is_empty(&self) -> bool {
    self.free.is_empty()
      && self.title.is_empty()
      && self.author.is_empty()
      && self.summary.is_empty()
      && self.category.is_empty()
      && self.year.is_none()
  }

  pub fn parse(raw: &str) -> Query {
    let mut q = Query::default();
    for token in tokenize(raw) {
      if let Some((key, value)) = token.split_once(':') {
        let value = value.trim();
        match key {
          "ti" | "title" if !value.is_empty() => {
            q.title.push(value.to_string());
            continue;
          }
          "abs" | "abstract" if !value.is_empty() => {
            q.summary.push(value.to_string());
            continue;
          }
          "au" | "author" if !value.is_empty() => {
            q.author.push(value.to_string());
            continue;
          }
          "cat" | "category" if !value.is_empty() => {
            q.category.push(value.to_string());
            continue;
          }
          "year" | "yr" => {
            // Unparseable year constraints are dropped, not demoted to
            // free text — `year:soon` shouldn't silently search titles.
            if let Some(c) = YearConstraint::parse(value) {
              q.year = Some(c);
            }
            continue;
          }
          _ => {}
        }
      }
      q.free.push(token);
    }
    q
  }

  /// Relevance score for `item`, or `None` if any required term fails.
  /// Higher is better. Only meaningful when `!self.is_empty()`.
  pub fn score(&self, item: &FeedItem, matcher: &SkimMatcherV2) -> Option<i64> {
    if let Some(yc) = &self.year {
      match item_year(item) {
        Some(y) if yc.satisfied_by(y) => {}
        _ => return None,
      }
    }

    // Category is controlled vocabulary, not free text — a hard gate
    // (like year), resolved via the taxonomy rather than fuzzy-matched.
    for cat in &self.category {
      if !crate::models::arxiv_taxonomy::item_matches_category(
        &item.domain_tags,
        cat,
      ) {
        return None;
      }
    }

    let mut total: i64 = 0;
    for term in &self.title {
      total += matcher.fuzzy_match(&item.title, term)? * W_TITLE;
    }
    for term in &self.summary {
      total += matcher.fuzzy_match(&item.summary_short, term)? * W_ABSTRACT;
    }
    for term in &self.author {
      total += best_author_score(matcher, &item.authors, term)? * W_AUTHOR;
    }
    for term in &self.free {
      let t = matcher.fuzzy_match(&item.title, term).map(|s| s * W_TITLE);
      let a =
        best_author_score(matcher, &item.authors, term).map(|s| s * W_AUTHOR);
      let b =
        matcher.fuzzy_match(&item.summary_short, term).map(|s| s * W_ABSTRACT);
      total += [t, a, b].into_iter().flatten().max()?;
    }
    Some(total)
  }
}

fn best_author_score(
  matcher: &SkimMatcherV2,
  authors: &[String],
  term: &str,
) -> Option<i64> {
  authors.iter().filter_map(|a| matcher.fuzzy_match(a, term)).max()
}

/// Year from an ISO `published_at` ("2024-05-21" / "2024-05-21T…").
fn item_year(item: &FeedItem) -> Option<i32> {
  item.published_at.get(0..4)?.parse().ok()
}

/// Split on whitespace, except inside double quotes. Quote characters
/// are consumed (not kept in the token), so `author:"Yann LeCun"`
/// yields the single token `author:Yann LeCun`.
fn tokenize(raw: &str) -> Vec<String> {
  let mut tokens = Vec::new();
  let mut cur = String::new();
  let mut in_quotes = false;
  for ch in raw.chars() {
    match ch {
      '"' => in_quotes = !in_quotes,
      c if c.is_whitespace() && !in_quotes => {
        if !cur.is_empty() {
          tokens.push(std::mem::take(&mut cur));
        }
      }
      c => cur.push(c),
    }
  }
  if !cur.is_empty() {
    tokens.push(cur);
  }
  tokens
}

#[cfg(test)]
mod tests {
  use super::*;

  fn item(
    title: &str,
    authors: &[&str],
    summary: &str,
    year: &str,
  ) -> FeedItem {
    let mut it = crate::models::fixtures::variant(0);
    it.title = title.to_string();
    it.authors = authors.iter().map(|s| s.to_string()).collect();
    it.summary_short = summary.to_string();
    it.published_at = format!("{year}-01-01T00:00:00Z");
    it
  }

  #[test]
  fn empty_query_is_empty() {
    assert!(Query::parse("").is_empty());
    assert!(Query::parse("   ").is_empty());
    assert!(!Query::parse("transformers").is_empty());
  }

  #[test]
  fn tokenize_groups_quoted_values() {
    assert_eq!(
      tokenize(r#"author:"Yann LeCun" deep"#),
      vec!["author:Yann LeCun".to_string(), "deep".to_string(),]
    );
  }

  #[test]
  fn parse_routes_field_prefixes() {
    let q = Query::parse(r#"ti:diffusion au:"Jane Doe" abs:elbo free"#);
    assert_eq!(q.title, vec!["diffusion"]);
    assert_eq!(q.author, vec!["Jane Doe"]);
    assert_eq!(q.summary, vec!["elbo"]);
    assert_eq!(q.free, vec!["free"]);
  }

  #[test]
  fn unknown_prefix_is_free_text() {
    // A colon that isn't a known field stays a single free term, so
    // pasted URLs / ratios don't get silently dropped.
    let q = Query::parse("http://example.com");
    assert!(q.title.is_empty() && q.author.is_empty());
    assert_eq!(q.free, vec!["http://example.com"]);
  }

  #[test]
  fn year_constraint_parsing() {
    assert_eq!(
      Query::parse("year:2024").year,
      Some(YearConstraint::Exact(2024))
    );
    assert_eq!(
      Query::parse("year:2020-2024").year,
      Some(YearConstraint::Range(2020, 2024))
    );
    assert_eq!(
      Query::parse("year:>2020").year,
      Some(YearConstraint::AtLeast(2021))
    );
    assert_eq!(
      Query::parse("year:>=2020").year,
      Some(YearConstraint::AtLeast(2020))
    );
    assert_eq!(
      Query::parse("year:<2024").year,
      Some(YearConstraint::AtMost(2023))
    );
    // Unparseable → no constraint, and not demoted to free text.
    let bad = Query::parse("year:soon");
    assert!(bad.year.is_none() && bad.free.is_empty());
  }

  #[test]
  fn year_filter_excludes_out_of_range() {
    let m = SkimMatcherV2::default();
    let q = Query::parse("year:2024");
    let hit = item("Any", &["A"], "x", "2024");
    let miss = item("Any", &["A"], "x", "2019");
    assert!(q.score(&hit, &m).is_some());
    assert!(q.score(&miss, &m).is_none());
  }

  #[test]
  fn field_term_excludes_non_matching_item() {
    let m = SkimMatcherV2::default();
    let q = Query::parse("au:hinton");
    let by_hinton = item("Capsules", &["Geoffrey Hinton"], "x", "2024");
    let by_other = item("Capsules", &["Yann LeCun"], "x", "2024");
    assert!(q.score(&by_hinton, &m).is_some());
    assert!(q.score(&by_other, &m).is_none());
  }

  #[test]
  fn title_hit_outranks_abstract_hit() {
    // "attention" in the title should beat "attention" only in the
    // abstract — the whole point of relevance ranking.
    let m = SkimMatcherV2::default();
    let q = Query::parse("attention");
    let in_title = item("Attention Is All You Need", &["A"], "a model", "2017");
    let in_abstract =
      item("A Model", &["A"], "we revisit attention mechanisms", "2017");
    let st = q.score(&in_title, &m).unwrap();
    let sa = q.score(&in_abstract, &m).unwrap();
    assert!(st > sa, "title score {st} should beat abstract score {sa}");
  }

  #[test]
  fn parse_routes_cat_prefix() {
    let q = Query::parse("cat:cs.LG transformers");
    assert_eq!(q.category, vec!["cs.LG"]);
    assert_eq!(q.free, vec!["transformers"]);
  }

  #[test]
  fn cat_matches_exact_code_and_archive() {
    let m = SkimMatcherV2::default();
    let mut paper = item("Some Paper", &["A"], "x", "2024");
    paper.domain_tags = vec!["cs.LG".to_string()];

    // Exact category code.
    assert!(Query::parse("cat:cs.LG").score(&paper, &m).is_some());
    // Archive-level query matches any cs.* category.
    assert!(Query::parse("cat:cs").score(&paper, &m).is_some());
    // Case-insensitive.
    assert!(Query::parse("cat:CS.lg").score(&paper, &m).is_some());
    // Different category is excluded.
    assert!(Query::parse("cat:math.NT").score(&paper, &m).is_none());
  }

  #[test]
  fn cat_combines_with_free_text_as_a_gate() {
    let m = SkimMatcherV2::default();
    let mut ml = item("Attention", &["A"], "x", "2024");
    ml.domain_tags = vec!["cs.LG".to_string()];
    let mut nt = item("Attention", &["A"], "x", "2024");
    nt.domain_tags = vec!["math.NT".to_string()];

    let q = Query::parse("cat:cs attention");
    // Same title match, but the category gate keeps only the cs paper.
    assert!(q.score(&ml, &m).is_some());
    assert!(q.score(&nt, &m).is_none());
  }

  #[test]
  fn all_free_terms_must_match() {
    let m = SkimMatcherV2::default();
    let q = Query::parse("diffusion robotics");
    let both = item("Diffusion Policies for Robotics", &["A"], "", "2023");
    let one = item("Diffusion Models", &["A"], "image synthesis", "2023");
    assert!(q.score(&both, &m).is_some());
    assert!(q.score(&one, &m).is_none());
  }
}
