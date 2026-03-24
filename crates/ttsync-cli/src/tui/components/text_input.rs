use crossterm::event::KeyCode;

#[derive(Debug, Clone)]
pub struct TextInput {
    pub value: String,
    cursor: usize,
}

impl TextInput {
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    pub fn visualize(&self) -> String {
        let cursor = self.cursor();
        let mut out = String::new();

        for (i, ch) in self.value.chars().enumerate() {
            if i == cursor {
                out.push('▏');
            }
            out.push(ch);
        }

        if cursor == self.value.chars().count() {
            out.push('▏');
        }

        out
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.cursor = self.value.chars().count();
    }

    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char(c) => {
                self.insert_char(c);
                true
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.value.chars().count();
                true
            }
            _ => false,
        }
    }

    fn insert_char(&mut self, c: char) {
        let idx = self.cursor_byte_index();
        self.value.insert(idx, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let idx = self.cursor_byte_index();
        let prev_idx = self.prev_cursor_byte_index();
        self.value.drain(prev_idx..idx);
        self.cursor -= 1;
        true
    }

    fn delete(&mut self) -> bool {
        if self.cursor >= self.value.chars().count() {
            return false;
        }
        let idx = self.cursor_byte_index();
        let next_idx = self.next_cursor_byte_index();
        self.value.drain(idx..next_idx);
        true
    }

    fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        true
    }

    fn move_right(&mut self) -> bool {
        if self.cursor >= self.value.chars().count() {
            return false;
        }
        self.cursor += 1;
        true
    }

    fn cursor_byte_index(&self) -> usize {
        if self.cursor == self.value.chars().count() {
            return self.value.len();
        }
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .expect("cursor must be within value")
    }

    fn prev_cursor_byte_index(&self) -> usize {
        let prev = self
            .value
            .char_indices()
            .nth(self.cursor.saturating_sub(1))
            .map(|(i, _)| i)
            .expect("cursor must be > 0");
        prev
    }

    fn next_cursor_byte_index(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor + 1)
            .map(|(i, _)| i)
            .unwrap_or(self.value.len())
    }
}
