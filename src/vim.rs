#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

impl Default for VimMode {
    fn default() -> Self {
        VimMode::Normal
    }
}

#[derive(Debug, Clone, Default)]
pub struct VimState {
    pub mode: VimMode,
    pub pending_op: Option<char>, // 'd', 'c', 'y', 'g'
    pub visual_anchor: Option<(usize, usize)>, // (row, col)
    pub register: String,
    pub is_line_register: bool,
    pub undo_stack: Vec<(Vec<String>, usize, usize)>,
}

impl VimState {
    pub fn new() -> Self {
        Self {
            mode: VimMode::Normal,
            pending_op: None,
            visual_anchor: None,
            register: String::new(),
            is_line_register: false,
            undo_stack: Vec::new(),
        }
    }

    pub fn save_undo(&mut self, lines: &[String], row: usize, col: usize) {
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push((lines.to_vec(), row, col));
    }

    pub fn undo(&mut self, lines: &mut Vec<String>, row: &mut usize, col: &mut usize) -> bool {
        if let Some((prev_lines, prev_row, prev_col)) = self.undo_stack.pop() {
            *lines = prev_lines;
            *row = prev_row.min(lines.len().saturating_sub(1));
            *col = prev_col.min(lines[*row].len());
            true
        } else {
            false
        }
    }

    pub fn enter_insert(&mut self, lines: &[String], row: usize, col: usize) {
        self.save_undo(lines, row, col);
        self.mode = VimMode::Insert;
        self.pending_op = None;
        self.visual_anchor = None;
    }

    pub fn enter_normal(&mut self, row: usize, col: &mut usize, lines: &[String]) {
        self.mode = VimMode::Normal;
        self.pending_op = None;
        self.visual_anchor = None;
        if row < lines.len() && *col > 0 && *col >= lines[row].len() {
            *col = lines[row].len().saturating_sub(1);
        }
    }

    pub fn enter_visual(&mut self, row: usize, col: usize, line_mode: bool) {
        self.mode = if line_mode { VimMode::VisualLine } else { VimMode::Visual };
        self.visual_anchor = Some((row, col));
        self.pending_op = None;
    }
}

// Helpers for word motions
pub fn next_word_start(line: &str, start_col: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    if start_col >= len {
        return len;
    }

    let mut i = start_col;
    let is_ident = is_word_char(chars[i]);

    // Skip current word
    while i < len && is_word_char(chars[i]) == is_ident && !chars[i].is_whitespace() {
        i += 1;
    }
    // Skip whitespace
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }
    i.min(len)
}

pub fn prev_word_start(line: &str, start_col: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    if start_col == 0 || chars.is_empty() {
        return 0;
    }

    let mut i = start_col.min(chars.len() - 1);
    if i > 0 && chars[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && chars[i].is_whitespace() {
        i -= 1;
    }

    let is_ident = is_word_char(chars[i]);
    while i > 0 && is_word_char(chars[i - 1]) == is_ident && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    i
}

pub fn word_end(line: &str, start_col: usize) -> usize {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    if start_col >= len {
        return len;
    }

    let mut i = start_col + 1;
    while i < len && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= len {
        return len.saturating_sub(1);
    }

    let is_ident = is_word_char(chars[i]);
    while i + 1 < len && is_word_char(chars[i + 1]) == is_ident && !chars[i + 1].is_whitespace() {
        i += 1;
    }
    i.min(len.saturating_sub(1))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
