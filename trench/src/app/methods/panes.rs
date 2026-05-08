use ratatui::layout::Rect;

use crate::app::{App, NavDirection, PaneId, PaneInfo, PANE_COUNT};

impl App {
  pub fn pane(&self, id: PaneId) -> &PaneInfo {
    &self.panes[id as usize]
  }

  pub fn pane_mut(&mut self, id: PaneId) -> &mut PaneInfo {
    &mut self.panes[id as usize]
  }

  /// Called from layout every frame with the computed screen rects.
  /// Pass `None` for a pane that is not currently rendered.
  pub fn update_pane_rects(
    &mut self,
    feed: Option<Rect>,
    reader: Option<Rect>,
    notes: Option<Rect>,
    details: Option<Rect>,
    chat: Option<Rect>,
    secondary_reader: Option<Rect>,
    secondary_notes: Option<Rect>,
  ) {
    let updates: [(PaneId, Option<Rect>); PANE_COUNT] = [
      (PaneId::Feed, feed),
      (PaneId::Reader, reader),
      (PaneId::Notes, notes),
      (PaneId::Details, details),
      (PaneId::Chat, chat),
      (PaneId::SecondaryReader, secondary_reader),
      (PaneId::SecondaryNotes, secondary_notes),
    ];
    for (id, opt) in updates {
      let info = self.pane_mut(id);
      info.is_open = opt.is_some();
      if let Some(r) = opt {
        info.rect = r;
      }
    }
  }

  /// Returns the `PaneId` of the nearest open pane in the given direction,
  /// using center-to-center Euclidean distance among directional candidates.
  pub fn find_pane_in_direction(&self, dir: NavDirection) -> Option<PaneId> {
    let current = self.pane(self.focused_pane);
    if !current.is_open {
      return None;
    }
    let cx = current.rect.x as i32 + current.rect.width as i32 / 2;
    let cy = current.rect.y as i32 + current.rect.height as i32 / 2;

    self
      .panes
      .iter()
      .filter(|p| {
        p.id != self.focused_pane
          && p.is_open
          && p.is_focusable()
          && p.rect.width > 0
          && p.rect.height > 0
      })
      .filter(|p| {
        let px = p.rect.x as i32;
        let py = p.rect.y as i32;
        let pw = p.rect.width as i32;
        let ph = p.rect.height as i32;
        match dir {
          NavDirection::Right => px + pw / 2 > cx,
          NavDirection::Left => px + pw / 2 < cx,
          NavDirection::Down => py + ph / 2 > cy,
          NavDirection::Up => py + ph / 2 < cy,
        }
      })
      .min_by_key(|p| {
        let pcx = p.rect.x as i32 + p.rect.width as i32 / 2;
        let pcy = p.rect.y as i32 + p.rect.height as i32 / 2;
        (pcx - cx) * (pcx - cx) + (pcy - cy) * (pcy - cy)
      })
      .map(|p| p.id)
  }

  /// Returns the `PaneId` of the open pane whose rect contains the given
  /// terminal cell, or `None` if no open pane covers that cell.
  pub fn pane_at(&self, col: u16, row: u16) -> Option<PaneId> {
    self
      .panes
      .iter()
      .filter(|p| p.is_open && p.rect.width > 0 && p.rect.height > 0)
      .find(|p| {
        col >= p.rect.x
          && col < p.rect.x + p.rect.width
          && row >= p.rect.y
          && row < p.rect.y + p.rect.height
      })
      .map(|p| p.id)
  }

  /// Returns the focusable open pane whose rect contains the given terminal
  /// cell. Passive panes like Details remain hit-testable via `pane_at` but do
  /// not receive focus.
  pub fn focusable_pane_at(&self, col: u16, row: u16) -> Option<PaneId> {
    self
      .panes
      .iter()
      .filter(|p| {
        p.is_open && p.is_focusable() && p.rect.width > 0 && p.rect.height > 0
      })
      .find(|p| {
        col >= p.rect.x
          && col < p.rect.x + p.rect.width
          && row >= p.rect.y
          && row < p.rect.y + p.rect.height
      })
      .map(|p| p.id)
  }

  /// Returns secondary open panes sorted top-to-bottom then left-to-right.
  pub fn secondary_panes_sorted(&self) -> Vec<PaneId> {
    let primary =
      if self.reader_active { PaneId::Reader } else { PaneId::Feed };
    let mut secondaries: Vec<&PaneInfo> = self
      .panes
      .iter()
      .filter(|p| p.id != primary && p.is_open && p.is_focusable())
      .collect();
    secondaries.sort_by_key(|p| (p.rect.y, p.rect.x));
    secondaries.iter().map(|p| p.id).collect()
  }
}
