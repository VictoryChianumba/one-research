#[derive(PartialEq)]
pub enum RepoPane {
  Tree,
  File,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoFileKind {
  Markdown,
  Code,
  PlainText,
}

pub struct RepoContext {
  pub owner: String,
  pub repo_name: String,
  pub default_branch: String,
  pub tree_path: String,
  pub tree_nodes: Vec<crate::github::TreeNode>,
  pub tree_cursor: usize,
  pub file_path: Option<String>,
  pub file_name: Option<String>,
  pub raw_file_content: String,
  pub file_kind: RepoFileKind,
  pub file_lines: Vec<String>,
  pub file_highlighted: Vec<Vec<(u8, u8, u8, String)>>,
  pub markdown_cache: Option<crate::ui::repo_markdown::MarkdownRenderCache>,
  pub rendered_line_count: usize,
  pub markdown_has_pannable_lines: bool,
  pub file_scroll: usize,
  pub pane_focus: RepoPane,
  pub status_message: Option<String>,
  pub no_token: bool,
  /// Horizontal character offset for panning (file pane only).
  pub h_offset: usize,
  /// Effective render width (0 = use pane width). +/- keys adjust this.
  pub wrap_width: usize,
  /// Momentum scroll velocity (lines/frame). Positive = down.
  pub scroll_velocity: f32,
}

/// Pre-computed bundle from a background file fetch. The classify +
/// syntect highlight pass was previously done on the UI thread inside
/// `repo_apply_file`; moved to the background thread that already has
/// the content. For a 10K-line file syntect highlighting can take
/// hundreds of ms to seconds — that work no longer blocks the UI.
pub struct RepoFileFetched {
  pub raw_content: String,
  pub file_kind: RepoFileKind,
  pub file_lines: Vec<String>,
  pub file_highlighted: Vec<Vec<(u8, u8, u8, String)>>,
}

/// Result from a background repo fetch operation.
pub enum RepoFetchResult {
  /// Initial repo open: default branch + root tree.
  RepoOpened {
    branch: String,
    tree: Result<Vec<crate::github::TreeNode>, String>,
  },
  /// Dir navigation (forward or back): path + tree.
  DirLoaded {
    path: String,
    result: Result<Vec<crate::github::TreeNode>, String>,
  },
  /// File view: path, filename, and the pre-computed file bundle
  /// (classify + lines split + syntect highlight done on the worker).
  FileLoaded {
    path: String,
    name: String,
    result: Result<RepoFileFetched, String>,
  },
}

/// What action should be taken when Enter is pressed in the repo tree pane.
pub enum RepoEnterTarget {
  Dir(String),
  File(String, String), // path, filename
}
