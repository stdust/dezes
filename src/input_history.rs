use crate::config::CMD_INPUT_HIST_SIZE;
use crate::hex::edit_dialog::EditDialogFocus;
use std::collections::VecDeque;
use tui_input::Input;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogHistoryEntry {
    pub focus: EditDialogFocus,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryQueue<T> {
    pub history: VecDeque<T>,
    pub history_index: Option<usize>,
    pub draft: Option<T>,
}

impl<T> Default for HistoryQueue<T> {
    fn default() -> Self {
        Self {
            history: VecDeque::new(),
            history_index: None,
            draft: None,
        }
    }
}

impl<T: Clone + PartialEq> HistoryQueue<T> {
    pub fn push(&mut self, entry: T) {
        if let Some(pos) = self.history.iter().position(|x| x == &entry) {
            self.history.remove(pos);
        } else if self.history.len() >= CMD_INPUT_HIST_SIZE {
            self.history.pop_front();
        }
        self.history.push_back(entry);
        self.reset_nav();
    }

    pub fn reset_nav(&mut self) {
        self.history_index = None;
        self.draft = None;
    }

    pub fn navigate_up(&mut self, current: &T) -> Option<T> {
        if self.history.is_empty() {
            return None;
        }
        let len = self.history.len();
        if self.history_index.is_none() {
            self.draft = Some(current.clone());
        }
        let new_index = match self.history_index {
            None => len - 1,
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_index = Some(new_index);
        Some(self.history[new_index].clone())
    }

    pub fn navigate_down(&mut self, current: &T) -> Option<T> {
        if self.history.is_empty() {
            return None;
        }
        let len = self.history.len();
        match self.history_index {
            None => {
                self.draft = Some(current.clone());
                let new_index = len - 1;
                self.history_index = Some(new_index);
                Some(self.history[new_index].clone())
            }
            Some(i) if i >= len - 1 => {
                self.history_index = None;
                Some(self.draft.take().unwrap_or_else(|| current.clone()))
            }
            Some(i) => {
                self.history_index = Some(i + 1);
                Some(self.history[i + 1].clone())
            }
        }
    }
}

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
        if let Some(pos) = self.history.iter().position(|x| x == &entry) {
            self.history.remove(pos);
        } else if self.history.len() >= CMD_INPUT_HIST_SIZE {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duplicate_entry_moves_to_latest() {
        let mut hist = InputHistory::default();
        hist.push("set theme sage23".to_string());
        hist.push("goto 100".to_string());
        hist.push("set theme dark".to_string());

        // Re-entering "set theme sage23" should move it to the most recent position
        hist.push("set theme sage23".to_string());
        assert_eq!(hist.history.len(), 3);

        // First Up should give the most recent entry: "set theme sage23"
        hist.up();
        assert_eq!(hist.input.value(), "set theme sage23");

        // Next Up should give "set theme dark"
        hist.up();
        assert_eq!(hist.input.value(), "set theme dark");

        // Next Up should give "goto 100"
        hist.up();
        assert_eq!(hist.input.value(), "goto 100");
    }

    #[test]
    fn test_history_capacity_limit() {
        let mut hist = InputHistory::default();
        for i in 0..(CMD_INPUT_HIST_SIZE + 10) {
            hist.push(format!("cmd_{}", i));
        }
        assert_eq!(hist.history.len(), CMD_INPUT_HIST_SIZE);
        // The first 10 items should have been dropped
        assert!(!hist.history.contains(&"cmd_0".to_string()));
        assert!(hist.history.contains(&format!("cmd_{}", CMD_INPUT_HIST_SIZE + 9)));
    }

    #[test]
    fn test_history_queue_up_down_draft() {
        let mut q: HistoryQueue<String> = HistoryQueue::default();
        q.push("A".to_string());
        q.push("B".to_string());
        q.push("C".to_string());

        let draft = "draft_text".to_string();

        // 1st Up: C (latest)
        assert_eq!(q.navigate_up(&draft), Some("C".to_string()));
        // 2nd Up: B
        assert_eq!(q.navigate_up(&draft), Some("B".to_string()));
        // 3rd Up: A
        assert_eq!(q.navigate_up(&draft), Some("A".to_string()));
        // 4th Up: stays at A
        assert_eq!(q.navigate_up(&draft), Some("A".to_string()));

        // Down from A: B
        assert_eq!(q.navigate_down(&draft), Some("B".to_string()));
        // Down from B: C
        assert_eq!(q.navigate_down(&draft), Some("C".to_string()));
        // Down past C: restores draft
        assert_eq!(q.navigate_down(&draft), Some("draft_text".to_string()));

        // Down directly on draft also loads latest (C)
        assert_eq!(q.navigate_down(&draft), Some("C".to_string()));
    }

    #[test]
    fn test_history_queue_dedup() {
        let mut q: HistoryQueue<String> = HistoryQueue::default();
        q.push("100".to_string());
        q.push("200".to_string());
        q.push("100".to_string());
        assert_eq!(q.history.len(), 2);
        assert_eq!(q.history[0], "200");
        assert_eq!(q.history[1], "100");
    }
}
