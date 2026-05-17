//! GitHub repo viewer load services. Three flavors:
//!
//! - `spawn_repo_open`: initial open — get default branch, fetch root tree.
//! - `spawn_repo_dir`: descend into a subdirectory — fetch its tree.
//! - `spawn_repo_file`: open a file — fetch its raw content.
//!
//! All wrap their worker bodies in `catch_unwind` so a panic in the
//! GitHub client routes back as `Err(...)` rather than killing the app.

use std::sync::mpsc;

use crate::app::RepoFetchResult;
use crate::github;
use crate::panic_msg;

pub(crate) fn spawn_repo_open(
  owner: String,
  repo: String,
  token: String,
  tx: mpsc::Sender<RepoFetchResult>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      let branch = match github::get_default_branch(&owner, &repo, &token) {
        Err(e) => {
          let _ = tx.send(RepoFetchResult::RepoOpened {
            branch: String::new(),
            tree: Err(e),
          });
          return;
        }
        Ok(b) => b,
      };
      let tree = github::fetch_tree_dir(&owner, &repo, &branch, "", &token);
      let _ = tx.send(RepoFetchResult::RepoOpened { branch, tree });
    }));
    if let Err(payload) = result {
      let msg = panic_msg(payload);
      log::error!("spawn_repo_open: thread panicked — {msg}");
      let _ = tx_panic.send(RepoFetchResult::RepoOpened {
        branch: String::new(),
        tree: Err(format!("repo-open thread panicked: {msg}")),
      });
    }
  });
}

pub(crate) fn spawn_repo_dir(
  owner: String,
  repo: String,
  branch: String,
  path: String,
  token: String,
  tx: mpsc::Sender<RepoFetchResult>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let path_panic = path.clone();
    let outcome =
      std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result =
          github::fetch_tree_dir(&owner, &repo, &branch, &path, &token);
        let _ = tx.send(RepoFetchResult::DirLoaded { path, result });
      }));
    if let Err(payload) = outcome {
      let msg = panic_msg(payload);
      log::error!("spawn_repo_dir: thread panicked — {msg}");
      let _ = tx_panic.send(RepoFetchResult::DirLoaded {
        path: path_panic,
        result: Err(format!("repo-dir thread panicked: {msg}")),
      });
    }
  });
}

pub(crate) fn spawn_repo_file(
  owner: String,
  repo: String,
  path: String,
  name: String,
  token: String,
  tx: mpsc::Sender<RepoFetchResult>,
) {
  std::thread::spawn(move || {
    let tx_panic = tx.clone();
    let path_panic = path.clone();
    let name_panic = name.clone();
    let outcome =
      std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // Fetch + classify + lines-split + syntect highlight, all on
        // the background thread. Previously the syntect pass ran on
        // the UI thread inside `repo_apply_file` and could block for
        // hundreds of ms to seconds on a large file.
        let result = match github::fetch_file(&owner, &repo, &path, &token)
        {
          Err(e) => Err(e),
          Ok(raw_content) => {
            let (file_kind, file_highlighted) =
              crate::app::classify_and_highlight_repo_file(
                &name,
                &raw_content,
              );
            let file_lines: Vec<String> =
              raw_content.lines().map(|l| l.to_string()).collect();
            Ok(crate::app::RepoFileFetched {
              raw_content,
              file_kind,
              file_lines,
              file_highlighted,
            })
          }
        };
        let _ = tx.send(RepoFetchResult::FileLoaded { path, name, result });
      }));
    if let Err(payload) = outcome {
      let msg = panic_msg(payload);
      log::error!("spawn_repo_file: thread panicked — {msg}");
      let _ = tx_panic.send(RepoFetchResult::FileLoaded {
        path: path_panic,
        name: name_panic,
        result: Err(format!("repo-file thread panicked: {msg}")),
      });
    }
  });
}
