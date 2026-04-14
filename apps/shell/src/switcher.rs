use sola_bus::topics::App;

#[derive(Default)]
pub struct SwitcherState {
    pub active: bool,
    pub apps: Vec<App>,
    pub selected: usize,
}

impl SwitcherState {
    pub fn selected_app_id(&self) -> Option<&str> {
        self.apps.get(self.selected).map(|a| a.app_id.as_str())
    }

    pub fn select_next(&mut self) {
        if !self.apps.is_empty() {
            self.selected = (self.selected + 1) % self.apps.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.apps.is_empty() {
            self.selected = (self.selected + self.apps.len() - 1) % self.apps.len();
        }
    }
}
