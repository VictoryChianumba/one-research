use super::super::{
  classify_repo_file_kind, encode_repo_url_path, validate_download_name,
};
use crate::app::{App, AppView, RepoEnterTarget, RepoFileKind, RepoPane};

impl App {
  pub fn close_repo_viewer(&mut self) {
    self.view = AppView::Feed;
    self.repo_context = None;
  }

  pub fn set_repo_status(&mut self, msg: impl Into<String>) {
    if let Some(ctx) = &mut self.repo_context {
      // Sanitize at the setter chokepoint — repo status renders into a
      // styled Span and reqwest error strings include URLs that may
      // carry attacker-supplied bytes via redirects.
      ctx.status_message =
        Some(crate::sanitize::sanitize_terminal_text(&msg.into()));
    }
  }

  /// Returns the action to take when Enter is pressed in the tree pane.
  pub fn repo_enter_target(&self) -> Option<RepoEnterTarget> {
    let ctx = self.repo_context.as_ref()?;
    if ctx.no_token {
      return None;
    }
    let node = ctx.tree_nodes.get(ctx.tree_cursor)?;
    match node.node_type {
      crate::github::NodeType::Dir => {
        Some(RepoEnterTarget::Dir(node.path.clone()))
      }
      crate::github::NodeType::File => {
        Some(RepoEnterTarget::File(node.path.clone(), node.name.clone()))
      }
    }
  }

  /// Returns the parent path for `b` (go up), or None if already at root.
  pub fn repo_back_target(&self) -> Option<String> {
    let ctx = self.repo_context.as_ref()?;
    if ctx.no_token || ctx.tree_path.is_empty() {
      return None;
    }
    let parent = match ctx.tree_path.rfind('/') {
      Some(pos) => ctx.tree_path[..pos].to_string(),
      None => String::new(),
    };
    Some(parent)
  }

  pub fn repo_apply_dir(
    &mut self,
    path: String,
    result: Result<Vec<crate::github::TreeNode>, String>,
  ) {
    let ctx = match self.repo_context.as_mut() {
      Some(c) => c,
      None => return,
    };
    match result {
      Ok(nodes) => {
        ctx.tree_path = path;
        ctx.tree_nodes = nodes;
        ctx.tree_cursor = 0;
        ctx.pane_focus = RepoPane::Tree;
        ctx.status_message = None;
      }
      Err(e) => {
        ctx.status_message = Some(crate::sanitize::sanitize_terminal_text(
          &format!("Error: {e}"),
        ));
      }
    }
  }

  pub fn repo_apply_file(
    &mut self,
    path: String,
    name: String,
    result: Result<String, String>,
  ) {
    let ctx = match self.repo_context.as_mut() {
      Some(c) => c,
      None => return,
    };
    match result {
      Ok(raw_content) => {
        let file_kind = classify_repo_file_kind(&name, &raw_content);
        let highlighted = match file_kind {
          RepoFileKind::Code => {
            crate::syntax::highlight_file(&raw_content, &name)
              .unwrap_or_default()
          }
          _ => Vec::new(),
        };
        let lines: Vec<String> =
          raw_content.lines().map(|l| l.to_string()).collect();
        ctx.file_path = Some(path);
        ctx.file_name = Some(name);
        ctx.raw_file_content = raw_content;
        ctx.file_kind = file_kind;
        ctx.file_lines = lines;
        ctx.file_highlighted = highlighted;
        ctx.markdown_cache = None;
        ctx.rendered_line_count = 0;
        ctx.markdown_has_pannable_lines = false;
        ctx.file_scroll = 0;
        ctx.h_offset = 0;
        ctx.scroll_velocity = 0.0;
        ctx.pane_focus = RepoPane::File;
        ctx.status_message = None;
      }
      Err(e) => {
        ctx.status_message = Some(crate::sanitize::sanitize_terminal_text(
          &format!("Error: {e}"),
        ));
      }
    }
  }

  pub fn repo_switch_pane(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      ctx.pane_focus = match ctx.pane_focus {
        RepoPane::Tree => RepoPane::File,
        RepoPane::File => RepoPane::Tree,
      };
    }
  }

  pub fn repo_nav_down(&mut self, file_visible_h: usize) {
    let _ = file_visible_h;
    if let Some(ctx) = &mut self.repo_context {
      match ctx.pane_focus {
        RepoPane::Tree => {
          let max = ctx.tree_nodes.len().saturating_sub(1);
          ctx.tree_cursor = (ctx.tree_cursor + 1).min(max);
        }
        RepoPane::File => {
          ctx.scroll_velocity += 3.0;
        }
      }
    }
  }

  pub fn repo_nav_up(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      match ctx.pane_focus {
        RepoPane::Tree => {
          ctx.tree_cursor = ctx.tree_cursor.saturating_sub(1);
        }
        RepoPane::File => {
          ctx.scroll_velocity -= 3.0;
        }
      }
    }
  }

  /// Advance momentum scroll by one frame.
  pub fn repo_tick(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.scroll_velocity.abs() >= 0.5 {
        let delta = ctx.scroll_velocity.round() as i64;
        let line_count = match ctx.file_kind {
          RepoFileKind::Markdown => ctx.rendered_line_count,
          _ => ctx.file_lines.len(),
        };
        let max = line_count.saturating_sub(1) as i64;
        let next = (ctx.file_scroll as i64 + delta).clamp(0, max) as usize;
        ctx.file_scroll = next;
        ctx.scroll_velocity *= 0.75;
      } else {
        ctx.scroll_velocity = 0.0;
      }
    }
  }

  pub fn repo_pan_left(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind == RepoFileKind::Markdown
        && !ctx.markdown_has_pannable_lines
      {
        return;
      }
      ctx.h_offset = ctx.h_offset.saturating_sub(4);
    }
  }

  pub fn repo_pan_right(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind == RepoFileKind::Markdown
        && !ctx.markdown_has_pannable_lines
      {
        return;
      }
      ctx.h_offset += 4;
    }
  }

  pub fn repo_zoom_in(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind != RepoFileKind::Markdown {
        return;
      }
      if ctx.wrap_width == 0 {
        // start from a sensible default — we don't know pane width here
        ctx.wrap_width = 120;
      }
      ctx.wrap_width = ctx.wrap_width.saturating_sub(10).max(20);
    }
  }

  pub fn repo_zoom_out(&mut self) {
    if let Some(ctx) = &mut self.repo_context {
      if ctx.file_kind != RepoFileKind::Markdown {
        return;
      }
      if ctx.wrap_width == 0 {
        ctx.wrap_width = 80;
      }
      ctx.wrap_width = ctx.wrap_width.saturating_add(10).min(200);
    }
  }

  /// Copy the currently selected path to clipboard.
  pub fn repo_copy_path(&mut self) {
    let path = if let Some(ctx) = &self.repo_context {
      match ctx.pane_focus {
        RepoPane::File => {
          ctx.file_path.clone().unwrap_or_else(|| ctx.tree_path.clone())
        }
        RepoPane::Tree => ctx
          .tree_nodes
          .get(ctx.tree_cursor)
          .map(|n| n.path.clone())
          .unwrap_or_default(),
      }
    } else {
      return;
    };

    // Strip any terminal-control bytes embedded in the GitHub-derived path
    // before writing to the OS clipboard. Otherwise a hostile repo could
    // ship ESC bytes that survive into whichever terminal the user later
    // pastes into.
    let safe_path = crate::sanitize::sanitize_terminal_text(&path);
    match arboard::Clipboard::new() {
      Ok(mut cb) => match cb.set_text(&safe_path) {
        Ok(()) => self.set_repo_status(format!("Copied: {safe_path}")),
        Err(e) => self.set_repo_status(format!("Clipboard error: {e}")),
      },
      Err(e) => self.set_repo_status(format!("Clipboard unavailable: {e}")),
    }
  }

  pub fn repo_copy_url(&mut self) {
    let Some(url) = self.repo_current_url() else {
      self.set_repo_status("No repo URL available.".to_string());
      return;
    };

    // Strip any terminal-control bytes from the GitHub-derived URL before
    // writing to the OS clipboard.
    let safe_url = crate::sanitize::sanitize_terminal_text(&url);
    match arboard::Clipboard::new() {
      Ok(mut cb) => match cb.set_text(&safe_url) {
        Ok(()) => self.set_repo_status(format!("Copied URL: {safe_url}")),
        Err(e) => self.set_repo_status(format!("Clipboard error: {e}")),
      },
      Err(e) => self.set_repo_status(format!("Clipboard unavailable: {e}")),
    }
  }

  pub fn repo_current_url(&self) -> Option<String> {
    let ctx = self.repo_context.as_ref()?;
    let base = format!("https://github.com/{}/{}", ctx.owner, ctx.repo_name);
    let branch = if ctx.default_branch.is_empty() {
      "HEAD"
    } else {
      ctx.default_branch.as_str()
    };

    match ctx.pane_focus {
      RepoPane::File => {
        let path = ctx.file_path.as_deref().or(ctx.file_name.as_deref())?;
        Some(format!("{base}/blob/{branch}/{}", encode_repo_url_path(path)))
      }
      RepoPane::Tree => {
        if let Some(node) = ctx.tree_nodes.get(ctx.tree_cursor) {
          let route = match node.node_type {
            crate::github::NodeType::Dir => "tree",
            crate::github::NodeType::File => "blob",
          };
          Some(format!(
            "{base}/{route}/{branch}/{}",
            encode_repo_url_path(&node.path)
          ))
        } else if ctx.tree_path.is_empty() {
          Some(base)
        } else {
          Some(format!(
            "{base}/tree/{branch}/{}",
            encode_repo_url_path(&ctx.tree_path)
          ))
        }
      }
    }
  }

  /// Save the current open file to ~/Downloads/{filename}.
  // (validate_download_name is defined as a free function below.)
  pub fn repo_download_file(&mut self) {
    let (name, content) = if let Some(ctx) = &self.repo_context {
      match (&ctx.file_name, &ctx.file_lines) {
        (Some(name), lines) if !lines.is_empty() => {
          (name.clone(), lines.join("\n"))
        }
        _ => return,
      }
    } else {
      return;
    };

    // Validate the GitHub-supplied filename against path-traversal. The
    // `name` field comes from `ctx.file_name` which originates in the
    // GitHub tree-listing response — a malicious or compromised repo could
    // populate it with `../etc/passwd` or `/etc/passwd`, both of which
    // `Path::join` happily accepts (an absolute join overwrites the base,
    // and `..` segments traverse up). Sec HIGH #4 from the audit.
    if let Err(e) = validate_download_name(&name) {
      self.set_repo_status(format!("Download rejected: {e}"));
      return;
    }

    let dest =
      dirs::download_dir().or_else(dirs::home_dir).map(|p| p.join(&name));

    if let Some(path) = dest {
      match std::fs::write(&path, &content) {
        Ok(()) => self.set_repo_status(format!("Saved to {}", path.display())),
        Err(e) => self.set_repo_status(format!("Download failed: {e}")),
      }
    }
  }

  pub fn repo_status_label(&self) -> Option<String> {
    let ctx = self.repo_context.as_ref()?;
    if ctx.no_token {
      Some("token required".to_string())
    } else {
      ctx.status_message.clone().or_else(|| Some("ready".to_string()))
    }
  }

}
