/// One note document open in the notes pane.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct NotesTab {
  #[serde(alias = "article_id")]
  pub note_id: String,
  pub title: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotesMode {
  PaperNotes,
  Library,
  Capture,
}

#[derive(Clone, Debug)]
pub struct NotesContext {
  pub paper: notes::PaperRef,
  pub source_label: String,
}

impl NotesMode {
  pub fn title(self) -> &'static str {
    match self {
      Self::PaperNotes => "Paper Notes",
      Self::Library => "Notes Library",
      Self::Capture => "Capture",
    }
  }

  pub fn footer_label(self) -> &'static str {
    match self {
      Self::PaperNotes => "paper notes",
      Self::Library => "notes library",
      Self::Capture => "capture",
    }
  }
}
