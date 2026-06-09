use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::Local;

use crate::history::{HistoryEntry, HistoryKind};
use crate::models::FeedItem;

#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
  Markdown,
  Jsonl,
}

impl ExportFormat {
  pub fn from_arg(s: &str) -> Option<Self> {
    match s.trim().to_lowercase().as_str() {
      "" | "md" | "markdown" => Some(Self::Markdown),
      "jsonl" | "json" => Some(Self::Jsonl),
      _ => None,
    }
  }

  fn extension(self) -> &'static str {
    match self {
      Self::Markdown => "md",
      Self::Jsonl => "jsonl",
    }
  }
}

fn export_dir() -> Option<PathBuf> {
  let mut p = std::env::var_os("HOME").map(PathBuf::from)?;
  p.push(".config/one-research/exports");
  Some(p)
}

fn timestamped_path(kind: &str, format: ExportFormat) -> Option<PathBuf> {
  let mut dir = export_dir()?;
  fs::create_dir_all(&dir).ok()?;
  let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
  dir.push(format!("{kind}-{stamp}.{}", format.extension()));
  Some(dir)
}

pub fn export_history(
  entries: &[&HistoryEntry],
  format: ExportFormat,
) -> io::Result<PathBuf> {
  let path = timestamped_path("history", format)
    .ok_or_else(|| io::Error::other("could not resolve export directory"))?;
  let mut buf: Vec<u8> = Vec::new();
  match format {
    ExportFormat::Jsonl => write_history_jsonl(&mut buf, entries)?,
    ExportFormat::Markdown => write_history_markdown(&mut buf, entries)?,
  }
  crate::store::atomic_write(&path, &buf)?;
  Ok(path)
}

pub fn export_library(
  items: &[&FeedItem],
  filter_label: &str,
  format: ExportFormat,
) -> io::Result<PathBuf> {
  let path = timestamped_path("library", format)
    .ok_or_else(|| io::Error::other("could not resolve export directory"))?;
  let mut buf: Vec<u8> = Vec::new();
  match format {
    ExportFormat::Jsonl => write_library_jsonl(&mut buf, items)?,
    ExportFormat::Markdown => {
      write_library_markdown(&mut buf, items, filter_label)?
    }
  }
  crate::store::atomic_write(&path, &buf)?;
  Ok(path)
}

// ── Writers ──────────────────────────────────────────────────────────────────
//
// Writers accumulate into an in-memory buffer rather than streaming to the
// file directly; the caller hands the full buffer to `store::atomic_write`,
// which writes via tmp-file + rename. A disk-full or panic mid-export now
// leaves either no file or the complete file at the final path — never a
// truncated one.

fn write_history_jsonl(
  buf: &mut Vec<u8>,
  entries: &[&HistoryEntry],
) -> io::Result<()> {
  for entry in entries {
    let line = serde_json::to_string(entry).map_err(io::Error::other)?;
    writeln!(buf, "{line}")?;
  }
  Ok(())
}

fn write_history_markdown(
  buf: &mut Vec<u8>,
  entries: &[&HistoryEntry],
) -> io::Result<()> {
  let now = Local::now().format("%Y-%m-%d %H:%M");
  writeln!(buf, "# One Research history — exported {now}")?;
  writeln!(buf)?;
  writeln!(buf, "{} entries.", entries.len())?;
  writeln!(buf)?;

  let mut current_day = String::new();
  for entry in entries {
    let local = entry.opened_at.with_timezone(&Local);
    let day = local.format("%Y-%m-%d").to_string();
    if day != current_day {
      writeln!(buf)?;
      writeln!(buf, "## {day}")?;
      writeln!(buf)?;
      current_day = day;
    }
    let time = local.format("%H:%M").to_string();
    let visit = if entry.visit_count > 1 {
      format!(" · ×{}", entry.visit_count)
    } else {
      String::new()
    };
    let kind = match entry.kind {
      HistoryKind::Paper => "paper",
      HistoryKind::Query => "query",
    };
    writeln!(
      buf,
      "- **{}** · {} · {} · {time}{visit}",
      escape_md(&entry.title),
      kind,
      escape_md(&entry.source)
    )?;
  }
  Ok(())
}

fn write_library_jsonl(
  buf: &mut Vec<u8>,
  items: &[&FeedItem],
) -> io::Result<()> {
  for item in items {
    let line = serde_json::to_string(item).map_err(io::Error::other)?;
    writeln!(buf, "{line}")?;
  }
  Ok(())
}

fn write_library_markdown(
  buf: &mut Vec<u8>,
  items: &[&FeedItem],
  filter_label: &str,
) -> io::Result<()> {
  let now = Local::now().format("%Y-%m-%d %H:%M");
  writeln!(
    buf,
    "# One Research library — {filter_label} ({} items, exported {now})",
    items.len()
  )?;
  writeln!(buf)?;

  for item in items {
    let source = if item.source_name.is_empty() {
      item.source_platform.short_label().to_string()
    } else {
      item.source_name.clone()
    };
    let authors = if item.authors.is_empty() {
      String::new()
    } else {
      format!(" — {}", item.authors.join(", "))
    };
    writeln!(
      buf,
      "- **{}** — {source}{authors} — {}",
      escape_md(&item.title),
      item.url
    )?;
  }
  Ok(())
}

fn escape_md(s: &str) -> String {
  // Minimal markdown escape: only the characters most likely to corrupt a list line.
  s.replace('\\', "\\\\").replace('*', "\\*").replace('_', "\\_")
}
