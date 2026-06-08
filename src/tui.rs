use std::{
    collections::{HashMap, HashSet},
    io,
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Style},
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
    #[default]
    All,
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
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit(),
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

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl<'a> Widget for &mut TuiApp<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [ticket_window, content_window] =
            Layout::horizontal([Constraint::Fill(1); 2]).areas(area);
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
                        "{} Tickets [↑/↓ to navigate, →/← to expand/collapse, space to select, Tab to switch focus, S to filter]",
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

        let content = Paragraph::new(
            self.tickets
                .iter()
                .find(|t| Some(&t.id().to_string()) == self.tree_state.selected().last())
                .map(|t| format!("{}", t.body))
                .unwrap_or("No ticket selected".to_string()),
        )
        .block(
            Block::bordered()
                .border_style(content_border)
                .title("Content"),
        )
        .wrap(Wrap { trim: false })
        .scroll((self.content_scroll, 0));

        let inner_width = content_window.width.saturating_sub(2);
        let inner_height = content_window.height.saturating_sub(2);
        let total_lines = content.line_count(inner_width) as u16;
        let max_scroll = total_lines.saturating_sub(inner_height);
        self.content_scroll = self.content_scroll.min(max_scroll);

        content
            .scroll((self.content_scroll, 0))
            .render(content_window, buf);

        let mut scrollbar_state = ScrollbarState::new(max_scroll as usize + 1)
            .position(self.content_scroll as usize)
            .viewport_content_length(inner_height as usize);

        if total_lines > inner_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            StatefulWidget::render(
                scrollbar,
                content_window.inner(Margin {
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
