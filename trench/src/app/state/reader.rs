use super::NotesContext;

/// One open paper inside a reader pane. Holds a tread Reader plus the
/// per-tab image cache (`ImageState`) and an optional arXiv id the reader
/// was opened with — used by `:reload` to refetch. When the source isn't
/// an arXiv paper (PDF / EPUB / HTML extract), we pass plain-text lines
/// into `tread::PaperData::from_plain_lines` and `arxiv_id` stays `None`.
pub struct ReaderTab {
  pub title: String,
  /// Used by the planned `:reload` command in tread to refetch arXiv
  /// sources. Slice 2 follow-up.
  #[allow(dead_code)]
  pub arxiv_id: Option<String>,
  pub notes_context: Option<NotesContext>,
  pub reader: tread::Reader,
  pub image_state: tread::ImageState,
  /// Per-tab burst gate for image emission.  Marked by the reader key
  /// handler on scroll keys; consulted by `after_draw_guarded` so a
  /// burst in one tab can't suppress images in another tab.
  pub burst: tread::BurstTracker,
  /// Last (width, height) we passed to `reader.resize`. Used to skip
  /// no-op resize calls every frame in the steady state — `tread::Reader`
  /// doesn't guarantee its own short-circuit.
  pub last_resize: Option<(u16, u16)>,
}
