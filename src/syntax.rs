use crate::theme::Theme;
use ratatui::text::{Line, Span};

const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "OUTER",
    "CROSS",
    "ON",
    "GROUP",
    "BY",
    "ORDER",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "DROP",
    "ALTER",
    "ADD",
    "COLUMN",
    "INDEX",
    "VIEW",
    "SCHEMA",
    "DATABASE",
    "AS",
    "DISTINCT",
    "AND",
    "OR",
    "NOT",
    "IN",
    "BETWEEN",
    "LIKE",
    "IS",
    "NULL",
    "TRUE",
    "FALSE",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "UNION",
    "ALL",
    "EXISTS",
    "ANY",
    "CAST",
    "COALESCE",
    "WITH",
    "RECURSIVE",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "DEFAULT",
    "UNIQUE",
    "CHECK",
    "CONSTRAINT",
    "CASCADE",
];

const SQL_FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "NOW",
    "DATE",
    "SUBSTRING",
    "TRIM",
    "UPPER",
    "LOWER",
    "CONCAT",
    "LENGTH",
    "ROUND",
    "FLOOR",
    "CEIL",
    "ABS",
    "COALESCE",
    "NULLIF",
    "JSON_EXTRACT",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
];

pub fn highlight_sql<'a>(text: &'a str, theme: &Theme) -> Vec<Span<'a>> {
    let mut spans = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Comment --
        if i + 1 < len && chars[i] == '-' && chars[i + 1] == '-' {
            let start = i;
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            let s = &text[byte_index(text, start)..byte_index(text, i)];
            spans.push(Span::styled(s, theme.sql_comment));
            continue;
        }

        // Single quote string
        if chars[i] == '\'' {
            let start = i;
            i += 1;
            while i < len && chars[i] != '\'' {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if i < len {
                i += 1; // include closing quote
            }
            let s = &text[byte_index(text, start)..byte_index(text, i)];
            spans.push(Span::styled(s, theme.sql_string));
            continue;
        }

        // Double quote identifier
        if chars[i] == '"' {
            let start = i;
            i += 1;
            while i < len && chars[i] != '"' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            let s = &text[byte_index(text, start)..byte_index(text, i)];
            spans.push(Span::styled(s, theme.sql_identifier));
            continue;
        }

        // Number
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let s = &text[byte_index(text, start)..byte_index(text, i)];
            spans.push(Span::styled(s, theme.sql_number));
            continue;
        }

        // Word (Keywords, Functions, Identifiers)
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let s = &text[byte_index(text, start)..byte_index(text, i)];
            let upper = s.to_uppercase();

            if SQL_KEYWORDS.contains(&upper.as_str()) {
                spans.push(Span::styled(s, theme.sql_keyword));
            } else if SQL_FUNCTIONS.contains(&upper.as_str()) {
                spans.push(Span::styled(s, theme.sql_function));
            } else {
                spans.push(Span::styled(s, theme.sql_identifier));
            }
            continue;
        }

        // Operators & Punctuation
        if "=<>!+-*/%&|^,;().".contains(chars[i]) {
            let start = i;
            i += 1;
            let s = &text[byte_index(text, start)..byte_index(text, i)];
            spans.push(Span::styled(s, theme.sql_operator));
            continue;
        }

        // Whitespace & other
        let start = i;
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i == start {
            i += 1;
        }
        let s = &text[byte_index(text, start)..byte_index(text, i)];
        spans.push(Span::raw(s));
    }

    spans
}

fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len())
}

#[allow(dead_code)]
pub fn highlight_lines<'a>(lines: &'a [String], theme: &Theme) -> Vec<Line<'a>> {
    lines
        .iter()
        .map(|l| Line::from(highlight_sql(l, theme)))
        .collect()
}
