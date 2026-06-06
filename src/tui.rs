use std::{collections::HashMap, io};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    DefaultTerminal, Frame,
    buffer::Buffer,
    crossterm,
    layout::Rect,
    widgets::{Block, ScrollbarOrientation, StatefulWidget, Widget},
};
use tui_tree_widget::{Scrollbar, ScrollbarState, Tree, TreeItem, TreeState};

use crate::Ticket;

#[derive(Debug, Default)]
pub struct TuiApp {
    tickets: Vec<Ticket>,
    exit: bool,
    tree_state: TreeState<String>,
    vertical_scroll: u16,
}

impl TuiApp {
    pub fn new(tickets: Vec<Ticket>) -> Self {
        Self {
            tickets,
            exit: false,
            tree_state: TreeState::default(),
            vertical_scroll: 0,
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
        if key_event.code == KeyCode::Esc {
            self.exit()
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

impl Widget for &mut TuiApp {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let items: Vec<TreeItem<String>> = assemble_ticket_tree(&self.tickets).unwrap_or_default();

        let tree_widget = Tree::new(&items)
            .expect("all item identifiers are unique")
            .block(Block::bordered().title("Tickets"));

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state =
            ScrollbarState::new(items.len()).position(self.vertical_scroll.into());

        <Tree<String> as StatefulWidget>::render(
            tree_widget,
            Rect::new(1, 1, area.width - 2, area.height - 2),
            buf,
            &mut self.tree_state,
        );

        <Scrollbar as StatefulWidget>::render(
            scrollbar,
            Rect::new(area.width - 1, 1, 1, area.height - 2),
            buf,
            &mut scrollbar_state,
        );
    }
}

fn assemble_ticket_tree(tickets: &'_ [Ticket]) -> color_eyre::Result<Vec<TreeItem<'_, String>>> {
    let mut nodes = Vec::new();
    let lookup: HashMap<String, Ticket> = tickets
        .iter()
        .map(|t| (t.id().to_string(), t.clone()))
        .collect();

    for ticket in tickets.iter().cloned() {
        let mut item = TreeItem::new_leaf(ticket.id().to_string(), ticket.summary());

        if !ticket.deps().is_empty() {
            add_deps_recursively(&mut item, ticket, &lookup)?;
        }

        nodes.push(item);
    }

    Ok(nodes)
}

fn add_deps_recursively(
    item: &mut TreeItem<String>,
    ticket: Ticket,
    lookup: &HashMap<String, Ticket>,
) -> color_eyre::Result<()> {
    for dep_id in ticket.deps() {
        if let Some(dep_ticket) = lookup.get(dep_id).cloned() {
            let mut dep_item =
                TreeItem::new_leaf(dep_ticket.id().to_string(), dep_ticket.summary());
            add_deps_recursively(&mut dep_item, dep_ticket, lookup)?;
            item.add_child(dep_item)?;
        }
    }
    Ok(())
}
