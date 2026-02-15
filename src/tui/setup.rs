//! Terminal setup and utilities

/// Calculate terminal dimensions based on total size and chat width percentage
pub fn calculate_terminal_dimensions(
    total_width: u16,
    total_height: u16,
    chat_width_percent: u16,
) -> (u16, u16) {
    // Terminal pane is (100% - chat_width_percent) of width, minus borders
    let term_width =
        ((total_width as f32 * (1.0 - chat_width_percent as f32 / 100.0)) as u16).saturating_sub(2);
    let term_height = total_height.saturating_sub(4);
    (term_width, term_height)
}
