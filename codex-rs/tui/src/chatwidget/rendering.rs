//! Render composition for the main chat widget surface.

use super::*;

impl ChatWidget {
    pub(crate) fn as_renderable(&self) -> RenderableItem<'_> {
        let active_cell_right_reserve = self.ambient_pet_wrap_reserved_cols();
        let active_cell_renderable = match &self.transcript.active_cell {
            Some(cell) => RenderableItem::Owned(Box::new(TranscriptAreaRenderable {
                child: cell.as_ref(),
                top: 1,
                right: active_cell_right_reserve,
            })),
            None => RenderableItem::Owned(Box::new(())),
        };
        let active_hook_cell_renderable = match &self.active_hook_cell {
            Some(cell) if cell.should_render() => {
                RenderableItem::Owned(Box::new(TranscriptAreaRenderable {
                    child: cell,
                    top: 1,
                    right: active_cell_right_reserve,
                }))
            }
            _ => RenderableItem::Owned(Box::new(())),
        };
        let mut flex = FlexRenderable::new();
        flex.push(/*flex*/ 1, active_cell_renderable);
        flex.push(/*flex*/ 0, active_hook_cell_renderable);
        if let Some(cell) = self.pending_token_activity_output() {
            flex.push(
                /*flex*/ 1,
                RenderableItem::Owned(Box::new(TranscriptAreaRenderable {
                    child: cell,
                    top: 1,
                    right: active_cell_right_reserve,
                })),
            );
        }
        if let Some(cell) = self.pending_rate_limit_reset_hint() {
            flex.push(
                /*flex*/ 1,
                RenderableItem::Owned(Box::new(TranscriptAreaRenderable {
                    child: cell,
                    top: 1,
                    right: active_cell_right_reserve,
                })),
            );
        }
        flex.push(
            /*flex*/ 0,
            self.bottom_pane
                .as_renderable_with_composer_right_reserve(active_cell_right_reserve)
                .inset(Insets::tlbr(
                    /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                )),
        );
        RenderableItem::Owned(Box::new(flex))
    }

    pub(crate) fn note_rendered_width(&self, width: u16) {
        self.last_rendered_width.set(Some(width as usize));
    }

    pub(crate) fn last_rendered_width(&self) -> Option<u16> {
        self.last_rendered_width
            .get()
            .and_then(|width| u16::try_from(width).ok())
    }

    /// Returns transient history cells in the same order as the traditional shared viewport.
    pub(crate) fn live_history_cells(&self) -> Vec<&dyn HistoryCell> {
        let mut cells = Vec::new();
        if let Some(cell) = self.transcript.active_cell.as_deref() {
            cells.push(cell);
        }
        if let Some(cell) = self
            .active_hook_cell
            .as_ref()
            .filter(|cell| cell.should_render())
        {
            cells.push(cell);
        }
        if let Some(cell) = self.pending_token_activity_output() {
            cells.push(cell);
        }
        if let Some(cell) = self.pending_rate_limit_reset_hint() {
            cells.push(cell);
        }
        cells
    }

    /// Returns the composer/status surface without the active transcript cell.
    pub(crate) fn composer_renderable(&self, right_reserve: u16) -> RenderableItem<'_> {
        self.bottom_pane
            .as_renderable_with_composer_right_reserve(right_reserve)
            .inset(Insets::tlbr(
                /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
            ))
    }

    pub(crate) fn ambient_pet_right_reserve(&self) -> u16 {
        self.ambient_pet_wrap_reserved_cols()
    }
}

struct TranscriptAreaRenderable<'a> {
    child: &'a dyn HistoryCell,
    top: u16,
    right: u16,
}

impl Renderable for TranscriptAreaRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let area = self.child_area(area);
        let lines = self.child.display_lines(area.width);
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
        let y = if area.height == 0 {
            0
        } else {
            let overflow = paragraph
                .line_count(area.width)
                .saturating_sub(usize::from(area.height));
            u16::try_from(overflow).unwrap_or(u16::MAX)
        };
        Clear.render(area, buf);
        paragraph.scroll((y, 0)).render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        let child_width = width.saturating_sub(self.right).max(1);
        HistoryCell::desired_height(self.child, child_width) + self.top
    }
}

impl TranscriptAreaRenderable<'_> {
    fn child_area(&self, area: Rect) -> Rect {
        let y = area.y.saturating_add(self.top);
        let height = area.height.saturating_sub(self.top);
        Rect::new(
            area.x,
            y,
            area.width.saturating_sub(self.right).max(1),
            height,
        )
    }
}

impl Renderable for ChatWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_renderable().render(area, buf);
        self.note_rendered_width(area.width);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.as_renderable().desired_height(width)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_renderable().cursor_pos(area)
    }

    fn cursor_style(&self, area: Rect) -> crossterm::cursor::SetCursorStyle {
        self.as_renderable().cursor_style(area)
    }
}
