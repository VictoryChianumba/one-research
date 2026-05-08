/// Help overlay state. Grouped from `help_active`, `help_section`, `help_scroll`.
#[derive(Default)]
pub struct HelpState {
  pub active: bool,
  pub section: usize,
  pub scroll: u16,
}
