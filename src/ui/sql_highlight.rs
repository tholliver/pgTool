use crate::ui::theme::Theme;

const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "IS", "NULL", "ORDER", "BY", "GROUP",
    "HAVING", "LIMIT", "OFFSET", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "ON", "AS", "INSERT",
    "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "ALTER", "DROP", "TRUNCATE",
    "DISTINCT", "ASC", "DESC", "UNION", "ALL", "EXISTS", "BETWEEN", "LIKE", "ILIKE", "CASE",
    "WHEN", "THEN", "ELSE", "END",
];

/// Builds a `TextEdit::layouter` closure that colors SQL tokens with theme
/// colors: strings, comments, numbers, and keywords (case-insensitive);
/// everything word-like else gets the identifier tint.
pub fn highlight_layouter(
    theme: &Theme,
) -> impl Fn(&egui::Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley> + '_ {
    move |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, wrap_width: f32| {
        let text = buffer.as_str();
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap_width;

        let mut chars = text.char_indices().peekable();
        while let Some((start, c)) = chars.next() {
            let (end, color) = if c == '\'' {
                // string literal — consume to the closing quote
                let mut e = start + c.len_utf8();
                while let Some(&(i, ch)) = chars.peek() {
                    chars.next();
                    e = i + ch.len_utf8();
                    if ch == '\'' {
                        break;
                    }
                }
                (e, theme.sql_string)
            } else if c == '-' && text[start..].starts_with("--") {
                // line comment — consume through end of line (newline excluded,
                // so it keeps the default color)
                let e = text[start..].find('\n').map_or(text.len(), |i| start + i);
                while chars.peek().is_some_and(|&(i, _)| i < e) {
                    chars.next();
                }
                (e, theme.sql_comment)
            } else if c.is_ascii_digit() {
                let mut e = start + c.len_utf8();
                while let Some(&(i, ch)) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' {
                        chars.next();
                        e = i + ch.len_utf8();
                    } else {
                        break;
                    }
                }
                (e, theme.sql_number)
            } else if c.is_alphabetic() || c == '_' {
                let mut e = start + c.len_utf8();
                while let Some(&(i, ch)) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        chars.next();
                        e = i + ch.len_utf8();
                    } else {
                        break;
                    }
                }
                let word = &text[start..e];
                let color = if KEYWORDS.contains(&word.to_uppercase().as_str()) {
                    theme.sql_keyword
                } else {
                    theme.sql_identifier
                };
                (e, color)
            } else {
                (start + c.len_utf8(), theme.text)
            };

            job.append(
                &text[start..end],
                0.0,
                egui::TextFormat {
                    font_id: egui::FontId::monospace(13.0),
                    color,
                    ..Default::default()
                },
            );
            // skip the characters already consumed by this token
            while chars.peek().is_some_and(|&(i, _)| i < end) {
                chars.next();
            }
        }

        ui.painter().layout_job(job)
    }
}
