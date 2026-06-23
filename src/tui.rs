use std::{
    collections::{HashMap, HashSet},
    io,
};

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm,
    layout::{Constraint, Layout, Margin, Position, Rect, Spacing},
    style::{Color, Style},
    symbols::merge::MergeStrategy,
    widgets::{Block, Paragraph, ScrollbarOrientation, StatefulWidget, Widget, Wrap},
};
use tui_tree_widget::{Scrollbar, ScrollbarState, Tree, TreeItem, TreeState};

use crate::{Ticket, cli::StatusValue};

#[derive(Debug, Default, PartialEq)]
enum Focus {
    #[default]
    Tickets,
    Content,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
enum TicketSelection {
    All,
    #[default]
    Open,
    InProgress,
    Closed,
}

#[derive(Debug, Default)]
pub struct TuiApp<'a> {
    tickets: &'a [Ticket],
    exit: bool,
    tree_state: TreeState<String>,
    focus: Focus,
    ticket_selection: TicketSelection,
    content_scroll: u16,
    tree_area: Rect,
    content_area: Rect,
}

impl<'a> TuiApp<'a> {
    pub fn new(tickets: &'a [Ticket]) -> Self {
        let mut tree_state = TreeState::default();
        tree_state.select_first();
        Self {
            tickets,
            exit: false,
            tree_state,
            focus: Focus::Tickets,
            content_scroll: 0,
            ..Default::default()
        }
    }
    /// runs the application's main loop until the user quits
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        frame.render_widget(self, frame.area());
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            // it's important to check that the event is a key press event as
            // crossterm also emits key release and repeat events on Windows.
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            Event::Mouse(mouse_event) => self.handle_mouse_event(mouse_event),
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => self.exit(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Tickets => Focus::Content,
                    Focus::Content => Focus::Tickets,
                };
            }
            _ => match self.focus {
                Focus::Tickets => self.handle_tree_key(key_event),
                Focus::Content => self.handle_content_key(key_event),
            },
        }
    }

    fn handle_tree_key(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Down => {
                self.tree_state.key_down();
            }
            KeyCode::Up => {
                self.tree_state.key_up();
            }
            KeyCode::Char(' ') => {
                self.tree_state.toggle_selected();
            }
            KeyCode::Right => {
                self.tree_state.key_right();
            }
            KeyCode::Left => {
                self.tree_state.key_left();
            }
            KeyCode::Char('s') => {
                self.ticket_selection = match self.ticket_selection {
                    TicketSelection::All => TicketSelection::Open,
                    TicketSelection::Open => TicketSelection::InProgress,
                    TicketSelection::InProgress => TicketSelection::Closed,
                    TicketSelection::Closed => TicketSelection::All,
                };
            }
            _ => {}
        }
        // reset scroll when selection changes
        self.content_scroll = 0;
    }

    fn handle_content_key(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.content_scroll = self.content_scroll.saturating_add(1);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.content_scroll = self.content_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.content_scroll = self.content_scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.content_scroll = self.content_scroll.saturating_sub(10);
            }
            _ => {}
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.tree_area.contains(pos) {
                    self.focus = Focus::Tickets;
                    // `click_at` selects the row rendered at this position (if any);
                    // only reset the body scroll when the selection actually changed.
                    if self.tree_state.click_at(pos) {
                        self.content_scroll = 0;
                    }
                } else if self.content_area.contains(pos) {
                    self.focus = Focus::Content;
                }
            }
            MouseEventKind::ScrollDown => {
                if self.content_area.contains(pos) {
                    self.content_scroll = self.content_scroll.saturating_add(1);
                } else if self.tree_area.contains(pos) {
                    self.tree_state.scroll_down(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if self.content_area.contains(pos) {
                    self.content_scroll = self.content_scroll.saturating_sub(1);
                } else if self.tree_area.contains(pos) {
                    self.tree_state.scroll_up(1);
                }
            }
            _ => {}
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl<'a> Widget for &mut TuiApp<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [ticket_window, content_window] =
            Layout::horizontal([Constraint::Fill(1); 2]).areas(area);
        self.tree_area = ticket_window;
        self.content_area = content_window;
        let items: Vec<TreeItem<String>> =
            assemble_ticket_tree(self.tickets, self.ticket_selection).unwrap_or_default();
        let (ticket_border, content_border) = if self.focus == Focus::Tickets {
            (Style::default().fg(Color::Blue), Style::default())
        } else {
            (Style::default(), Style::default().fg(Color::Blue))
        };

        let tree_widget = Tree::new(&items)
            .expect("all item identifiers are unique")
            .block(
                Block::bordered()
                    .border_style(ticket_border)
                    .title(format!(
                        "{} Tickets [↑/↓ or click to select, →/← expand/collapse, Tab/click to switch focus, scroll wheel, S to filter]",
                        match self.ticket_selection {
                            TicketSelection::All => "All",
                            TicketSelection::Open => "Open",
                            TicketSelection::InProgress => "In Progress",
                            TicketSelection::Closed => "Closed",
                        }
                    )),
            )
            .highlight_style(Style::default().bg(Color::Blue))
            .experimental_scrollbar(Some(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("↑"))
                    .end_symbol(Some("↓")),
            ));
        <Tree<String> as StatefulWidget>::render(
            tree_widget,
            ticket_window,
            buf,
            &mut self.tree_state,
        );

        let selected_ticket = self
            .tickets
            .iter()
            .find(|t| Some(&t.id().to_string()) == self.tree_state.selected().last());

        let [meta_window, body_window] =
            Layout::vertical([Constraint::Length(3), Constraint::Fill(1)])
                .spacing(Spacing::Overlap(1))
                .areas(content_window);

        let [status_window, priority_window, assignee_window] =
            Layout::horizontal([Constraint::Fill(1); 3])
                .spacing(Spacing::Overlap(1))
                .areas(meta_window);

        let (status_text, priority_text, assignee_text) = match selected_ticket {
            Some(t) => (
                t.status().to_string(),
                format!("P{}", t.priority()),
                t.assignee().unwrap_or("unassigned").to_string(),
            ),
            None => ("-".to_string(), "-".to_string(), "-".to_string()),
        };

        // Adjacent blocks overlap by one cell (Spacing::Overlap above) and
        // merge_borders draws the shared edges as proper junctions.
        let meta_block = |title: &'static str| {
            Block::bordered()
                .border_style(content_border)
                .merge_borders(MergeStrategy::Exact)
                .title(title)
        };
        Paragraph::new(status_text)
            .block(meta_block("Status"))
            .render(status_window, buf);
        Paragraph::new(priority_text)
            .block(meta_block("Priority"))
            .render(priority_window, buf);
        Paragraph::new(assignee_text)
            .block(meta_block("Assignee"))
            .render(assignee_window, buf);

        let content = Paragraph::new(
            selected_ticket
                .map(|t| format!("{}", t.body))
                .unwrap_or("No ticket selected".to_string()),
        )
        .block(
            Block::bordered()
                .border_style(content_border)
                .merge_borders(MergeStrategy::Exact),
        )
        .wrap(Wrap { trim: false })
        .scroll((self.content_scroll, 0));

        let inner_width = body_window.width.saturating_sub(2);
        let inner_height = body_window.height.saturating_sub(2);
        let total_lines = content.line_count(inner_width) as u16;
        let max_scroll = total_lines.saturating_sub(inner_height);
        self.content_scroll = self.content_scroll.min(max_scroll);

        content
            .scroll((self.content_scroll, 0))
            .render(body_window, buf);

        let mut scrollbar_state = ScrollbarState::new(max_scroll as usize + 1)
            .position(self.content_scroll as usize)
            .viewport_content_length(inner_height as usize);

        if total_lines > inner_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            StatefulWidget::render(
                scrollbar,
                body_window.inner(Margin {
                    vertical: 1,
                    horizontal: 0,
                }),
                buf,
                &mut scrollbar_state,
            );
        }
    }
}

fn assemble_ticket_tree(
    tickets: &'_ [Ticket],
    ticket_selection: TicketSelection,
) -> color_eyre::Result<Vec<TreeItem<'_, String>>> {
    let mut nodes = Vec::new();
    let lookup: HashMap<String, Ticket> = tickets
        .iter()
        .map(|t| (t.id().to_string(), t.clone()))
        .collect();

    // Any ticket that is referenced as a dependency (at any level) is not a
    // root and must only appear nested under its parent, never at the top
    // level.
    let dependency_ids: HashSet<String> = tickets
        .iter()
        .flat_map(|t| t.deps().iter().cloned())
        .collect();

    for ticket in tickets.iter().filter(|t| match ticket_selection {
        TicketSelection::All => true,
        TicketSelection::Open => t.status() != &StatusValue::Closed,
        TicketSelection::InProgress => t.status() == &StatusValue::InProgress,
        TicketSelection::Closed => t.status() == &StatusValue::Closed,
    }) {
        if dependency_ids.contains(ticket.id()) {
            continue;
        }

        let mut item = TreeItem::new_leaf(ticket.id().to_string(), ticket.summary());

        if !ticket.deps().is_empty() {
            let mut visited = HashSet::new();
            visited.insert(ticket.id().to_string());
            add_deps_recursively(&mut item, ticket, &lookup, &mut visited, ticket_selection)?;
        }

        nodes.push(item);
    }

    Ok(nodes)
}

fn add_deps_recursively(
    item: &mut TreeItem<String>,
    ticket: &Ticket,
    lookup: &HashMap<String, Ticket>,
    visited: &mut HashSet<String>,
    ticket_selection: TicketSelection,
) -> color_eyre::Result<()> {
    for dep_id in ticket.deps() {
        // Guard against cycles and repeated dependencies so a ticket is never
        // inserted more than once within the same branch.
        if !visited.insert(dep_id.clone()) {
            continue;
        }

        if let Some(dep_ticket) = lookup.get(dep_id) {
            if match ticket_selection {
                TicketSelection::All => false,
                TicketSelection::Open => dep_ticket.status() == &StatusValue::Closed,
                TicketSelection::InProgress => dep_ticket.status() != &StatusValue::InProgress,
                TicketSelection::Closed => dep_ticket.status() != &StatusValue::Closed,
            } {
                continue;
            }

            let mut dep_item =
                TreeItem::new_leaf(dep_ticket.id().to_string(), dep_ticket.summary());
            add_deps_recursively(&mut dep_item, dep_ticket, lookup, visited, ticket_selection)?;
            item.add_child(dep_item)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Focus, TuiApp};
    use ratatui::crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    // Build an app with a known layout: the left half (cols 0..40) is the
    // ticket tree, the right half (cols 40..80) is the content pane. These are
    // normally set during `render`; tests set them directly since they never
    // draw.
    fn test_app() -> TuiApp<'static> {
        let mut app = TuiApp::new(&[]);
        app.tree_area = Rect::new(0, 0, 40, 24);
        app.content_area = Rect::new(40, 0, 40, 24);
        app
    }

    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn scroll_down_over_content_increments_scroll() {
        let mut app = test_app();
        app.handle_mouse_event(mouse_at(MouseEventKind::ScrollDown, 50, 5));
        assert_eq!(app.content_scroll, 1);
    }

    #[test]
    fn scroll_up_over_content_decrements_scroll() {
        let mut app = test_app();
        app.content_scroll = 5;
        app.handle_mouse_event(mouse_at(MouseEventKind::ScrollUp, 50, 5));
        assert_eq!(app.content_scroll, 4);
    }

    #[test]
    fn scroll_up_over_content_saturates_at_zero() {
        let mut app = test_app();
        app.handle_mouse_event(mouse_at(MouseEventKind::ScrollUp, 50, 5));
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn scroll_over_tree_leaves_content_scroll_untouched() {
        let mut app = test_app();
        app.handle_mouse_event(mouse_at(MouseEventKind::ScrollDown, 5, 5));
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn scroll_outside_panes_leaves_content_scroll_untouched() {
        let mut app = test_app();
        app.handle_mouse_event(mouse_at(MouseEventKind::ScrollDown, 200, 200));
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn left_click_in_content_focuses_content() {
        let mut app = test_app();
        assert_eq!(app.focus, Focus::Tickets);
        app.handle_mouse_event(mouse_at(MouseEventKind::Down(MouseButton::Left), 50, 5));
        assert_eq!(app.focus, Focus::Content);
    }

    #[test]
    fn left_click_in_tree_focuses_tickets() {
        let mut app = test_app();
        app.focus = Focus::Content;
        app.handle_mouse_event(mouse_at(MouseEventKind::Down(MouseButton::Left), 5, 5));
        assert_eq!(app.focus, Focus::Tickets);
    }

    #[test]
    fn left_click_outside_panes_leaves_focus_untouched() {
        let mut app = test_app();
        app.focus = Focus::Content;
        app.handle_mouse_event(mouse_at(MouseEventKind::Down(MouseButton::Left), 200, 200));
        assert_eq!(app.focus, Focus::Content);
    }

    #[test]
    fn right_click_does_not_change_focus() {
        let mut app = test_app();
        assert_eq!(app.focus, Focus::Tickets);
        app.handle_mouse_event(mouse_at(MouseEventKind::Down(MouseButton::Right), 50, 5));
        assert_eq!(app.focus, Focus::Tickets);
    }
}
