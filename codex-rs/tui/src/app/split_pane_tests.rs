use super::*;

#[derive(Debug)]
struct TestCell {
    stream: HistoryCellStream,
    lines: Vec<&'static str>,
}

impl TestCell {
    fn new(stream: HistoryCellStream, lines: Vec<&'static str>) -> Self {
        Self { stream, lines }
    }
}

impl HistoryCell for TestCell {
    fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
        self.lines.iter().copied().map(Line::from).collect()
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.display_lines(/*width*/ u16::MAX)
    }

    fn stream(&self) -> HistoryCellStream {
        self.stream
    }
}

struct TestComposer;

type CommittedCells = Vec<Arc<dyn HistoryCell>>;
type LiveCells = Vec<Box<dyn HistoryCell>>;

impl Renderable for TestComposer {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Paragraph::new(vec![
            Line::from(""),
            Line::from("> Type a message"),
            Line::from("  100% context left").dim(),
        ])
        .render(area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        3
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        Some((area.x.saturating_add(2), area.y.saturating_add(1)))
    }
}

fn fixture_cells() -> (CommittedCells, LiveCells) {
    (
        vec![
            Arc::new(TestCell::new(
                HistoryCellStream::Conversation,
                vec!["> split the chat from the tool stream"],
            )),
            Arc::new(TestCell::new(
                HistoryCellStream::Activity,
                vec!["Ran cargo test", "  42 tests passed"],
            )),
            Arc::new(TestCell::new(
                HistoryCellStream::Conversation,
                vec!["* I will keep the conversation readable."],
            )),
            Arc::new(TestCell::new(
                HistoryCellStream::Activity,
                vec!["Edited tui/src/app.rs", "  + split layout"],
            )),
        ],
        vec![Box::new(TestCell::new(
            HistoryCellStream::Activity,
            vec!["Running targeted checks..."],
        ))],
    )
}

fn render_fixture(width: u16, height: u16) -> String {
    let (committed, live) = fixture_cells();
    let live = live
        .iter()
        .map(|cell| cell.as_ref() as &dyn HistoryCell)
        .collect::<Vec<_>>();
    let area = Rect::new(/*x*/ 0, /*y*/ 0, width, height);
    let mut buffer = Buffer::empty(area);
    render_stream_surface(
        area,
        &mut buffer,
        &committed,
        &live,
        &TestComposer,
        StreamSurfaceOptions {
            history_mode: HistoryRenderMode::Rich,
            ambient_pet_right_reserve: 0,
            scroll_state: SplitPaneScrollState::default(),
        },
    );
    buffer_to_text(&buffer, area)
}

fn buffer_to_text(buffer: &Buffer, area: Rect) -> String {
    let mut rows = Vec::new();
    for y in area.y..area.bottom() {
        let row = (area.x..area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        rows.push(row.trim_end().to_string());
    }
    rows.join("\n")
}

#[test]
fn wide_layout_separates_conversation_and_activity_snapshot() {
    insta::assert_snapshot!(render_fixture(/*width*/ 120, /*height*/ 14));
}

#[test]
fn narrow_layout_combines_streams_snapshot() {
    insta::assert_snapshot!(render_fixture(/*width*/ 80, /*height*/ 12));
}

#[test]
fn mouse_scroll_targets_each_wide_pane_independently() {
    let mut state = SplitPaneScrollState::default();
    let (conversation_width, _) = split_widths(/*width*/ 120).expect("wide split");
    let event = |kind, column| MouseEvent {
        kind,
        column,
        row: 4,
        modifiers: KeyModifiers::NONE,
    };

    assert!(apply_mouse_scroll(
        &mut state,
        /*terminal_width*/ 120,
        event(MouseEventKind::ScrollUp, conversation_width - 1),
    ));
    assert_eq!(
        state.offset(SplitPaneRegion::Conversation),
        SCROLL_ROWS_PER_EVENT
    );
    assert_eq!(state.offset(SplitPaneRegion::Activity), 0);

    assert!(apply_mouse_scroll(
        &mut state,
        /*terminal_width*/ 120,
        event(MouseEventKind::ScrollUp, conversation_width + 1),
    ));
    assert_eq!(
        state.offset(SplitPaneRegion::Activity),
        SCROLL_ROWS_PER_EVENT
    );

    assert!(apply_mouse_scroll(
        &mut state,
        /*terminal_width*/ 120,
        event(MouseEventKind::ScrollDown, conversation_width - 1),
    ));
    assert_eq!(state.offset(SplitPaneRegion::Conversation), 0);
}

#[test]
fn scroll_offset_moves_viewport_away_from_tail() {
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 12, /*height*/ 3,
    );
    let lines = ["line 0", "line 1", "line 2", "line 3", "line 4", "line 5"]
        .into_iter()
        .map(HyperlinkLine::from)
        .collect::<Vec<_>>();
    let mut tail = Buffer::empty(area);
    render_lines_tail(area, &mut tail, &lines, /*from_bottom*/ 0);
    let mut scrolled = Buffer::empty(area);
    render_lines_tail(area, &mut scrolled, &lines, /*from_bottom*/ 2);

    assert_eq!(buffer_to_text(&tail, area), "line 3\nline 4\nline 5");
    assert_eq!(buffer_to_text(&scrolled, area), "line 1\nline 2\nline 3");
}
