//! Structured feed-search query parsing + non-text gates (ADR-012 grammar,
//! ADR-013 mechanism).
//!
//! The search bar text is parsed once into field-scoped terms, free text,
//! a category list, and an optional year constraint. Fuzzy *ranking* runs
//! off-thread in [`engine::FeedSearch`] (nucleo); this module owns the
//! parse plus the `cat:` / `year:` gates and a substring fallback.
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

use crate::models::FeedItem;

// ADR-013: async, incremental ranking runs in `engine::FeedSearch`
// (nucleo, off-thread). This module owns query *parsing* and the
// non-text *gates* (`cat:`, `year:`) that nucleo can't express, plus a
// substring fallback for tabs the engine doesn't index.
pub mod engine;

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

  /// Controlled-vocabulary / numeric gates (`cat:`, `year:`) that the
  /// nucleo text matcher can't express. `true` if the item satisfies all
  /// of them. Applied alongside the nucleo ranking (ADR-013 §D4).
  pub fn passes_gates(&self, item: &FeedItem) -> bool {
    if let Some(yc) = &self.year {
      match item_year(item) {
        Some(y) if yc.satisfied_by(y) => {}
        _ => return false,
      }
    }
    for cat in &self.category {
      if !crate::models::arxiv_taxonomy::item_matches_category(
        &item.domain_tags,
        cat,
      ) {
        return false;
      }
    }
    true
  }

  /// Case-insensitive substring fallback for tabs the nucleo engine
  /// doesn't index (Discoveries) or before its first snapshot. Every
  /// field-scoped and free term must appear in its field(s); `cat:` /
  /// `year:` gates apply too. Not fuzzy — that's the engine's job.
  pub fn matches_substring(&self, item: &FeedItem) -> bool {
    if !self.passes_gates(item) {
      return false;
    }
    let has = |hay: &str, needle: &str| {
      hay.to_lowercase().contains(&needle.to_lowercase())
    };
    let in_authors = |needle: &str| item.authors.iter().any(|a| has(a, needle));
    self.title.iter().all(|t| has(&item.title, t))
      && self.summary.iter().all(|t| has(&item.summary_short, t))
      && self.author.iter().all(|t| in_authors(t))
      && self.free.iter().all(|t| {
        has(&item.title, t) || has(&item.summary_short, t) || in_authors(t)
      })
  }
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
  fn year_gate_excludes_out_of_range() {
    let q = Query::parse("year:2024");
    assert!(q.passes_gates(&item("Any", &["A"], "x", "2024")));
    assert!(!q.passes_gates(&item("Any", &["A"], "x", "2019")));
  }

  #[test]
  fn cat_gate_matches_exact_code_and_archive() {
    let mut paper = item("Some Paper", &["A"], "x", "2024");
    paper.domain_tags = vec!["cs.LG".to_string()];
    assert!(Query::parse("cat:cs.LG").passes_gates(&paper)); // exact code
    assert!(Query::parse("cat:cs").passes_gates(&paper)); // archive-level
    assert!(Query::parse("cat:CS.lg").passes_gates(&paper)); // case-insensitive
    assert!(!Query::parse("cat:math.NT").passes_gates(&paper)); // different cat
  }

  #[test]
  fn substring_field_scope_restricts_to_its_field() {
    let q = Query::parse("au:hinton");
    let by_hinton = item("Capsules", &["Geoffrey Hinton"], "x", "2024");
    let by_other = item("Capsules", &["Yann LeCun"], "x", "2024");
    assert!(q.matches_substring(&by_hinton));
    assert!(!q.matches_substring(&by_other));
  }

  #[test]
  fn substring_all_free_terms_must_match() {
    let q = Query::parse("diffusion robotics");
    let both = item("Diffusion Policies for Robotics", &["A"], "", "2023");
    let one = item("Diffusion Models", &["A"], "image synthesis", "2023");
    assert!(q.matches_substring(&both));
    assert!(!q.matches_substring(&one));
  }

  #[test]
  fn substring_respects_cat_gate() {
    let mut ml = item("Attention", &["A"], "x", "2024");
    ml.domain_tags = vec!["cs.LG".to_string()];
    let mut nt = item("Attention", &["A"], "x", "2024");
    nt.domain_tags = vec!["math.NT".to_string()];
    let q = Query::parse("cat:cs attention");
    assert!(q.matches_substring(&ml));
    assert!(!q.matches_substring(&nt));
  }

  #[test]
  fn parse_routes_cat_prefix() {
    let q = Query::parse("cat:cs.LG transformers");
    assert_eq!(q.category, vec!["cs.LG"]);
    assert_eq!(q.free, vec!["transformers"]);
  }
}
