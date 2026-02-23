//! UI Utility functions

/// Format elapsed time in human-readable form
pub fn format_elapsed(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    if secs >= 60 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{}.{:01}s", secs, millis / 100)
    } else {
        format!("{}ms", millis)
    }
}

/// Format token count with K/M suffix
pub fn format_tokens(tokens: u32) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

/// Wrap text to fit within a given width
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = if width == 0 { 1 } else { width };
    let mut lines = Vec::new();

    // split('\n') returns at least one element even for empty string
    let paragraphs: Vec<&str> = text.split('\n').collect();

    for paragraph in paragraphs.iter() {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        let mut current_width = 0;

        let mut chars = paragraph.chars().peekable();
        while let Some(c) = chars.next() {
            if c == ' ' {
                if current_width < width {
                    current_line.push(' ');
                    current_width += 1;
                } else {
                    lines.push(current_line);
                    current_line = String::new();
                    current_width = 0;
                }
            } else {
                let mut word = String::from(c);
                while let Some(&nc) = chars.peek() {
                    if nc == ' ' {
                        break;
                    }
                    word.push(chars.next().unwrap());
                }

                let word_len = word.chars().count();
                if current_width + word_len <= width {
                    current_line.push_str(&word);
                    current_width += word_len;
                } else {
                    if !current_line.is_empty() {
                        lines.push(current_line);
                        current_line = String::new();
                        current_width = 0;
                    }

                    let mut remaining = word;
                    while !remaining.is_empty() {
                        let r_len = remaining.chars().count();
                        if r_len <= width {
                            current_line = remaining;
                            current_width = r_len;
                            remaining = String::new();
                        } else {
                            let split_idx = remaining
                                .char_indices()
                                .nth(width)
                                .map(|(i, _)| i)
                                .unwrap_or(remaining.len());
                            lines.push(remaining[..split_idx].to_string());
                            remaining = remaining[split_idx..].to_string();
                        }
                    }
                }
            }
        }
        lines.push(current_line);
    }
    lines
}

/// Calculate cursor position in wrapped text
pub fn calculate_input_cursor_pos(text: &str, cursor_idx: usize, width: usize) -> (u16, u16) {
    if width == 0 {
        return (0, 0);
    }

    let prefix: String = text.chars().take(cursor_idx).collect();
    let wrapped = wrap_text(&prefix, width);

    if wrapped.is_empty() {
        return (0, 0);
    }

    let row = wrapped.len().saturating_sub(1);
    let col = wrapped.last().map(|l| l.chars().count()).unwrap_or(0);

    (col as u16, row as u16)
}



/// Format a unix timestamp as a compact string for the list view
/// Shows date if not today, otherwise shows time
pub fn format_timestamp(ts: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    let dt = DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local))
        .unwrap_or_else(|| Local::now());

    let now = Local::now();
    let is_today = dt.date_naive() == now.date_naive();

    if is_today {
        // Today: show time only
        dt.format("%H:%M").to_string()
    } else {
        // Not today: show month/day
        dt.format("%m/%d").to_string()
    }
}

/// Format a unix timestamp as a full string for the detail view
pub fn format_timestamp_full(ts: i64) -> String {
    use chrono::{DateTime, Local, Utc};

    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}
