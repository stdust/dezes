use crate::config::CMD_INPUT_HIST_SIZE;
use std::collections::VecDeque;
use tui_input::Input;

#[derive(Default)]
pub struct InputHistory {
    pub input: Input,
    pub history: VecDeque<String>,
    pub history_index: Option<usize>,
    pub cursor_pos: usize,
    pub selection_anchor: Option<usize>,
}

impl InputHistory {
    pub fn push(&mut self, entry: String) {
        if entry.trim().is_empty() {
            return;
        }
        if self.history.contains(&entry) {
            return;
        }
        if self.history.len() >= CMD_INPUT_HIST_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(entry);
        self.history_index = None;
        self.cursor_pos = 0;
        self.selection_anchor = None;
    }

    #[allow(dead_code)]
    pub fn reset_selection(&mut self) {
        self.selection_anchor = None;
    }

    pub fn up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let len = self.history.len();
        let new_index = match self.history_index {
            None => len - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_index = Some(new_index);
        let text = self.history[new_index].clone();
        self.cursor_pos = text.chars().count();
        self.selection_anchor = None;
        self.input = Input::new(text);
    }

    pub fn down(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let len = self.history.len();
        match self.history_index {
            None => {}
            Some(i) if i >= len - 1 => {
                self.history_index = None;
                self.cursor_pos = 0;
                self.selection_anchor = None;
                self.input = Input::default();
            }
            Some(i) => {
                self.history_index = Some(i + 1);
                let text = self.history[i + 1].clone();
                self.cursor_pos = text.chars().count();
                self.selection_anchor = None;
                self.input = Input::new(text);
            }
        }
    }
}
