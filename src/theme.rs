use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub border_active: Color,
    pub border_inactive: Color,
    pub title: Color,
    pub accent: Color,
    pub success: Color,
    pub error: Color,
    pub warning: Color,
    pub muted: Color,

    pub table_header_fg: Color,
    pub table_header_bg: Color,
    pub table_selected_bg: Color,
    pub table_selected_fg: Color,

    pub sql_keyword: Style,
    pub sql_string: Style,
    pub sql_number: Style,
    pub sql_comment: Style,
    pub sql_function: Style,
    pub sql_identifier: Style,
    pub sql_operator: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::Rgb(26, 27, 38),              // #1a1b26
            fg: Color::Rgb(192, 202, 245),           // #c0caf5
            border_active: Color::Rgb(122, 162, 247),// #7aa2f7
            border_inactive: Color::Rgb(59, 66, 97), // #3b4261
            title: Color::Rgb(187, 154, 247),        // #bb9af7
            accent: Color::Rgb(125, 207, 255),       // #7dcfff
            success: Color::Rgb(115, 218, 202),      // #73daca
            error: Color::Rgb(247, 118, 142),        // #f7768e
            warning: Color::Rgb(224, 175, 104),      // #e0af68
            muted: Color::Rgb(86, 95, 137),          // #565f89

            table_header_fg: Color::Rgb(122, 162, 247),
            table_header_bg: Color::Rgb(36, 40, 59),
            table_selected_bg: Color::Rgb(61, 89, 161),
            table_selected_fg: Color::Rgb(255, 255, 255),

            sql_keyword: Style::default()
                .fg(Color::Rgb(187, 154, 247))
                .add_modifier(Modifier::BOLD),
            sql_string: Style::default().fg(Color::Rgb(158, 206, 106)),
            sql_number: Style::default().fg(Color::Rgb(255, 158, 100)),
            sql_comment: Style::default()
                .fg(Color::Rgb(86, 95, 137))
                .add_modifier(Modifier::ITALIC),
            sql_function: Style::default()
                .fg(Color::Rgb(122, 162, 247))
                .add_modifier(Modifier::BOLD),
            sql_identifier: Style::default().fg(Color::Rgb(192, 202, 245)),
            sql_operator: Style::default().fg(Color::Rgb(137, 221, 255)),
        }
    }
}
