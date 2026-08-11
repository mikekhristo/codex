//! Retained conversation/activity layout for wide alternate-screen terminals.

use super::*;
use crate::history_cell::HistoryCellStream;
use crate::history_cell::HistoryRenderMode;
use crate::render::renderable::Renderable;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::mark_buffer_hyperlinks;
use crate::terminal_hyperlinks::visible_lines_ref;
use codex_config::types::TuiLayout;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Text;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

const MIN_SPLIT_WIDTH: u16 = 120;
const MIN_CONVERSATION_WIDTH: u16 = 48;
const MIN_ACTIVITY_WIDTH: u16 = 56;
const CONVERSATION_PERCENT: u32 = 45;
const SCROLL_ROWS_PER_EVENT: usize = 3;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct SplitPaneScrollState {
    conversation_from_bottom: usize,
    activity_from_bottom: usize,
    combined_from_bottom: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SplitPaneRegion {
    Conversation,
    Activity,
    Combined,
}

impl SplitPaneScrollState {
    fn offset(self, region: SplitPaneRegion) -> usize {
        match region {
            SplitPaneRegion::Conversation => self.conversation_from_bottom,
            SplitPaneRegion::Activity => self.activity_from_bottom,
            SplitPaneRegion::Combined => self.combined_from_bottom,
        }
    }

    fn offset_mut(&mut self, region: SplitPaneRegion) -> &mut usize {
        match region {
            SplitPaneRegion::Conversation => &mut self.conversation_from_bottom,
            SplitPaneRegion::Activity => &mut self.activity_from_bottom,
            SplitPaneRegion::Combined => &mut self.combined_from_bottom,
        }
    }
}

struct StreamSurfaceResult {
    conversation_width: u16,
    cursor: Option<((u16, u16), SetCursorStyle)>,
}

#[derive(Clone, Copy)]
struct StreamSurfaceOptions {
    history_mode: HistoryRenderMode,
    ambient_pet_right_reserve: u16,
    scroll_state: SplitPaneScrollState,
}

impl App {
    /// Keep the split transcript on the terminal's full-screen buffer. `enter_alt_screen()` is a
    /// no-op when the user selected `--no-alt-screen` or `tui.alternate_screen = "never"`, so
    /// those explicit opt-outs still fall back to the traditional inline transcript.
    pub(super) fn ensure_split_pane_screen(&self, tui: &mut tui::Tui) -> Result<()> {
        if self.config.tui_layout == TuiLayout::Split {
            tui.set_mouse_capture_enabled(/*enabled*/ true);
            tui.enter_alt_screen()?;
        }
        Ok(())
    }

    /// The retained stream layout relies on alternate-screen ownership so it does not overwrite
    /// shell scrollback. Explicit `--no-alt-screen` and `tui.alternate_screen = "never"` keep the
    /// traditional inline transcript even when `tui.layout = "split"` is configured.
    pub(super) fn split_pane_active(&self, tui: &tui::Tui) -> bool {
        self.config.tui_layout == TuiLayout::Split && tui.is_alt_screen_active()
    }

    pub(super) fn split_conversation_width(&self, terminal_width: u16) -> u16 {
        split_widths(terminal_width)
            .map(|(conversation, _activity)| conversation)
            .unwrap_or(terminal_width)
    }

    /// Route a wheel gesture to the pane underneath the pointer. Returning true keeps the
    /// synthesized wheel movement away from composer history navigation.
    pub(super) fn handle_split_pane_mouse(
        &mut self,
        tui: &mut tui::Tui,
        terminal_width: u16,
        event: MouseEvent,
    ) -> bool {
        if !apply_mouse_scroll(&mut self.split_pane_scroll, terminal_width, event) {
            return false;
        }
        tui.frame_requester().schedule_frame();
        true
    }

    pub(super) fn render_split_pane_frame(
        &self,
        tui: &mut tui::Tui,
        screen_size: Size,
    ) -> Result<Rect> {
        let mut rendered_area = Rect::default();
        let history_mode = self.chat_widget.history_render_mode();
        let live_cells = self.chat_widget.live_history_cells();
        let ambient_pet_right_reserve = self.chat_widget.ambient_pet_right_reserve();
        let composer_right_reserve = if split_widths(screen_size.width).is_some() {
            0
        } else {
            ambient_pet_right_reserve
        };
        let composer = self.chat_widget.composer_renderable(composer_right_reserve);

        tui.draw_with_resize_reflow(screen_size.height, screen_size, |frame| {
            let area = frame.area();
            rendered_area = area;
            let result = render_stream_surface(
                area,
                frame.buffer,
                &self.transcript_cells,
                &live_cells,
                &composer,
                StreamSurfaceOptions {
                    history_mode,
                    ambient_pet_right_reserve,
                    scroll_state: self.split_pane_scroll,
                },
            );
            self.chat_widget
                .note_rendered_width(result.conversation_width);
            if let Some(((x, y), style)) = result.cursor {
                frame.set_cursor_style(style);
                frame.set_cursor_position((x, y));
            }
        })?;

        Ok(rendered_area)
    }
}

fn render_stream_surface(
    area: Rect,
    buf: &mut Buffer,
    committed_cells: &[Arc<dyn HistoryCell>],
    live_cells: &[&dyn HistoryCell],
    composer: &dyn Renderable,
    options: StreamSurfaceOptions,
) -> StreamSurfaceResult {
    Clear.render(area, buf);
    if let Some((conversation_width, activity_width)) = split_widths(area.width) {
        let conversation_area = Rect::new(area.x, area.y, conversation_width, area.height);
        let divider_x = conversation_area.right();
        let activity_area = Rect::new(
            divider_x.saturating_add(1),
            area.y,
            activity_width,
            area.height,
        );
        render_divider(divider_x, area, buf);

        let (conversation_header, conversation_content, composer_area) =
            column_with_composer(conversation_area, composer);
        let (activity_header, activity_content) = column_without_composer(activity_area);
        let activity_content = reserve_right(activity_content, options.ambient_pet_right_reserve);
        render_header("CHAT", conversation_header, buf);
        render_header("ACTIVITY", activity_header, buf);

        let conversation_lines = collect_stream_lines(
            committed_cells,
            live_cells,
            conversation_content.width,
            options.history_mode,
            Some(HistoryCellStream::Conversation),
        );
        render_lines_tail(
            conversation_content,
            buf,
            &conversation_lines,
            options.scroll_state.offset(SplitPaneRegion::Conversation),
        );

        let activity_lines = collect_stream_lines(
            committed_cells,
            live_cells,
            activity_content.width,
            options.history_mode,
            Some(HistoryCellStream::Activity),
        );
        render_lines_tail(
            activity_content,
            buf,
            &activity_lines,
            options.scroll_state.offset(SplitPaneRegion::Activity),
        );
        composer.render(composer_area, buf);

        StreamSurfaceResult {
            conversation_width,
            cursor: composer
                .cursor_pos(composer_area)
                .map(|position| (position, composer.cursor_style(composer_area))),
        }
    } else {
        let (header, content, composer_area) = column_with_composer(area, composer);
        let content = reserve_right(content, options.ambient_pet_right_reserve);
        render_header("TRANSCRIPT", header, buf);
        let lines = collect_stream_lines(
            committed_cells,
            live_cells,
            content.width,
            options.history_mode,
            /*stream*/ None,
        );
        render_lines_tail(
            content,
            buf,
            &lines,
            options.scroll_state.offset(SplitPaneRegion::Combined),
        );
        composer.render(composer_area, buf);

        StreamSurfaceResult {
            conversation_width: area.width,
            cursor: composer
                .cursor_pos(composer_area)
                .map(|position| (position, composer.cursor_style(composer_area))),
        }
    }
}

fn split_widths(width: u16) -> Option<(u16, u16)> {
    let available = width.checked_sub(/*rhs*/ 1)?;
    if width < MIN_SPLIT_WIDTH {
        return None;
    }
    let conversation =
        u16::try_from(u32::from(available).saturating_mul(CONVERSATION_PERCENT) / /*rhs*/ 100)
            .unwrap_or(available);
    let activity = available.saturating_sub(conversation);
    (conversation >= MIN_CONVERSATION_WIDTH && activity >= MIN_ACTIVITY_WIDTH)
        .then_some((conversation, activity))
}

fn split_region_at(width: u16, column: u16) -> SplitPaneRegion {
    match split_widths(width) {
        Some((conversation_width, _activity_width)) if column < conversation_width => {
            SplitPaneRegion::Conversation
        }
        Some(_) => SplitPaneRegion::Activity,
        None => SplitPaneRegion::Combined,
    }
}

fn apply_mouse_scroll(
    state: &mut SplitPaneScrollState,
    terminal_width: u16,
    event: MouseEvent,
) -> bool {
    let scroll_up = match event.kind {
        MouseEventKind::ScrollUp => true,
        MouseEventKind::ScrollDown => false,
        _ => return false,
    };
    let region = split_region_at(terminal_width, event.column);
    let offset = state.offset_mut(region);
    if scroll_up {
        *offset = offset.saturating_add(SCROLL_ROWS_PER_EVENT);
    } else {
        *offset = offset.saturating_sub(SCROLL_ROWS_PER_EVENT);
    }
    true
}

fn column_with_composer(area: Rect, composer: &dyn Renderable) -> (Rect, Rect, Rect) {
    let header = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let remaining_height = area.height.saturating_sub(header.height);
    let composer_height = composer
        .desired_height(area.width)
        .min(remaining_height.saturating_sub(/*rhs*/ 1));
    let content_height = remaining_height.saturating_sub(composer_height);
    let content = Rect::new(area.x, header.bottom(), area.width, content_height);
    let composer_area = Rect::new(area.x, content.bottom(), area.width, composer_height);
    (header, content, composer_area)
}

fn column_without_composer(area: Rect) -> (Rect, Rect) {
    let header = Rect::new(area.x, area.y, area.width, area.height.min(1));
    let content = Rect::new(
        area.x,
        header.bottom(),
        area.width,
        area.height.saturating_sub(header.height),
    );
    (header, content)
}

fn reserve_right(mut area: Rect, columns: u16) -> Rect {
    area.width = area.width.saturating_sub(columns).max(1).min(area.width);
    area
}

fn render_header(label: &str, area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    let label = format!(" {label} ");
    let rule_width = usize::from(area.width).saturating_sub(label.len());
    Line::from(vec![label.bold(), "─".repeat(rule_width).dim()]).render(area, buf);
}

fn render_divider(x: u16, area: Rect, buf: &mut Buffer) {
    if x >= area.right() {
        return;
    }
    for y in area.y..area.bottom() {
        buf[(x, y)].set_char('│').set_style(Style::default().dim());
    }
}

fn collect_stream_lines(
    committed_cells: &[Arc<dyn HistoryCell>],
    live_cells: &[&dyn HistoryCell],
    width: u16,
    history_mode: HistoryRenderMode,
    stream: Option<HistoryCellStream>,
) -> Vec<HyperlinkLine> {
    let mut lines = Vec::new();
    let mut has_visible_cell = false;
    for cell in committed_cells
        .iter()
        .map(Arc::as_ref)
        .chain(live_cells.iter().copied())
    {
        if stream.is_some_and(|stream| stream != cell.stream()) {
            continue;
        }
        let mut cell_lines = cell.display_hyperlink_lines_for_mode(width, history_mode);
        if cell_lines.is_empty() {
            continue;
        }
        if has_visible_cell && !cell.is_stream_continuation() {
            lines.push(HyperlinkLine::from(""));
        }
        lines.append(&mut cell_lines);
        has_visible_cell = true;
    }
    lines
}

fn render_lines_tail(area: Rect, buf: &mut Buffer, lines: &[HyperlinkLine], from_bottom: usize) {
    if area.is_empty() {
        return;
    }
    Clear.render(area, buf);
    let paragraph = Paragraph::new(Text::from(visible_lines_ref(lines))).wrap(Wrap { trim: false });
    let max_scroll = paragraph
        .line_count(area.width)
        .saturating_sub(usize::from(area.height));
    let scroll = max_scroll.saturating_sub(from_bottom.min(max_scroll));
    let scroll = u16::try_from(scroll).unwrap_or(u16::MAX);
    paragraph.scroll((scroll, 0)).render(area, buf);
    mark_buffer_hyperlinks(buf, area, lines, usize::from(scroll));
}

#[cfg(test)]
#[path = "split_pane_tests.rs"]
mod tests;
