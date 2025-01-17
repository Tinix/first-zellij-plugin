mod ui;

use ui::color::Colors;
use ui::panes::PaneUi;
use ui::widgets::compose_ui;

use zellij_tile::prelude::*;

use nohash_hasher::IntMap;
use std::collections::BTreeMap;

#[derive(Default, Clone)]
struct State {
    is_loading: bool,
    panes: IntMap<usize, PaneUi>,
    selected_pane: Option<PaneUi>,
    cursor_pane_index: Option<usize>,
    colors: Colors,
    new_width: u8,
    new_height: u8,
    input_buffer: String,
    awaiting_length_input: bool,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[
            EventType::SessionUpdate,
            EventType::Key,
            EventType::ModeUpdate,
        ]);
        self.is_loading = true;
    }

    fn update(&mut self, event: Event) -> bool {
        let mut render = false;
        match event {
            Event::ModeUpdate(mode_info) => {
                self.colors = Colors::new(mode_info.style.colors);
                render = true;
            }
            Event::Key(key) => {
                self.handle_key(key);
                render = true;
            }
            Event::SessionUpdate(session_info) => {
                self.get_panes(&session_info);
                if self.selected_pane.is_some() {
                    self.update_selected_pane(&session_info);
                }
                self.is_loading = false;
                render = true;
            }
            Event::PermissionRequestResult(_result) => {
                render = true;
            }
            _ => {
                self.is_loading = true;
            }
        }
        render
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if !self.is_loading {
            let panes: Vec<PaneUi> = self.panes.values().cloned().collect();
            compose_ui(
                rows,
                cols,
                self.colors,
                panes,
                self.selected_pane.as_ref(),
                self.cursor_pane_index,
                self.new_width,
                self.new_height,
            );
        }
    }
}

impl State {
    fn get_panes(&mut self, session: &[SessionInfo]) {
        self.panes.clear();
        let current_session = session
            .iter()
            .find(|session| session.is_current_session)
            .expect("no current session");
        let mut start_idx = 1;

        for tab in &current_session.tabs {
            if let Some(related_panes) = current_session.panes.panes.get(&tab.position) {
                let filtered_panes: Vec<PaneUi> = related_panes
                    .iter()
                    .filter_map(|pane| {
                        if pane.is_floating {
                            Some(PaneUi::new(pane, tab))
                        } else {
                            None
                        }
                    })
                    .collect();

                for pane in filtered_panes {
                    self.panes.insert(start_idx, pane);
                    start_idx += 1;
                }
            }
        }
    }

    fn update_selected_pane(&mut self, session: &[SessionInfo]) {
        let current_session = session
            .iter()
            .find(|session| session.is_current_session)
            .expect("no current session");

        if let Some(selected_pane) = &self.selected_pane {
            let selected_tab_id = selected_pane.parent_tab.tab_id;
            let selected_pane_id = selected_pane.pane_id;

            if let Some(pane_info) =
                current_session
                    .panes
                    .panes
                    .get(&selected_tab_id)
                    .and_then(|pane_from_tab| {
                        pane_from_tab
                            .iter()
                            .find(|pane_info| pane_info.id == selected_pane_id)
                    })
            {
                if let Some(parent_tab) = current_session.tabs.get(selected_tab_id) {
                    self.selected_pane = Some(PaneUi::new(pane_info, parent_tab));
                }
            }
        }
    }

    fn send_resize_event(&mut self) {
        let size = ResizeByPercent {
            width: self.new_width as u32,
            height: self.new_height as u32,
        };

        let tab_pos = self.selected_pane.as_ref().unwrap().parent_tab.tab_id;
        let pane_id = if let Some(pane) = self.selected_pane.as_ref() {
            if pane.is_plugin {
                Some(PaneId::Plugin(pane.pane_id))
            } else {
                Some(PaneId::Terminal(pane.pane_id))
            }
        } else {
            None
        };

        resize_floating_pane_by_percent(size, Some(tab_pos.try_into().unwrap()), pane_id);

        self.new_width = 0;
        self.new_height = 0;
    }

    fn handle_key(&mut self, e: Key) {
        match e {
            Key::Down => match self.cursor_pane_index {
                Some(idx) if idx < self.panes.len() => {
                    self.cursor_pane_index = Some(idx + 1);
                }
                Some(idx) if idx == self.panes.len() => {
                    self.cursor_pane_index = Some(1);
                }
                Some(_) => {
                    unreachable!()
                }
                None => self.cursor_pane_index = Some(1),
            },
            Key::Up => match self.cursor_pane_index {
                Some(idx) if idx > 1 => {
                    self.cursor_pane_index = Some(idx - 1);
                }
                Some(idx) if idx == 1 => {
                    self.cursor_pane_index = Some(self.panes.len());
                }
                Some(_) => {
                    unreachable!()
                }
                None => self.cursor_pane_index = Some(1),
            },
            Key::Ctrl(c) => {
                if c == 's' && self.selected_pane.is_some() {
                    self.send_resize_event();
                } else if c == 'r' && self.selected_pane.is_some() {
                    self.new_width = 0;
                    self.new_height = 0;
                    self.input_buffer.clear();
                    self.awaiting_length_input = false;
                } else if c == 'e' {
                    close_focus();
                }
            }
            Key::Esc => {
                if self.selected_pane.is_some() {
                    self.selected_pane = None;
                    self.new_width = 0;
                    self.new_height = 0;
                } else {
                    hide_self();
                }
            }
            Key::Delete => {
                if self.selected_pane.is_some() {
                    self.selected_pane = None;
                } else {
                    hide_self();
                }
            }
            Key::Char(c) => match c {
                '\n' if self.selected_pane.is_none() => {
                    self.selected_pane = self
                        .cursor_pane_index
                        .and_then(|idx| self.panes.get(&idx).cloned());
                }
                '\n' if self.selected_pane.is_some() => {
                    if self.awaiting_length_input {
                        self.new_height = self.input_buffer.parse::<u8>().unwrap_or(0);
                        self.input_buffer.clear();
                        self.awaiting_length_input = false;
                    } else {
                        self.new_width = self.input_buffer.parse::<u8>().unwrap_or(0);
                        self.input_buffer.clear();
                        self.awaiting_length_input = true;
                    }
                }
                '0'..='9' => {
                    if self.selected_pane.is_some() {
                        self.capture_number_input(c);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn capture_number_input(&mut self, c: char) {
        self.input_buffer.push(c);
    }
}
