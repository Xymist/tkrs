use clap::Parser;
use ticket::{
    cli::{
        Command, TicketCli, cmd_add_note, cmd_blocked, cmd_close, cmd_closed, cmd_create, cmd_dep,
        cmd_edit, cmd_link, cmd_ls, cmd_query, cmd_ready, cmd_reopen, cmd_show, cmd_start,
        cmd_status, cmd_undep, cmd_unlink, cmd_update,
    },
    fs::{lock_tickets, refresh_ticket_cache},
    tui::TuiApp,
};

fn main() -> color_eyre::Result<()> {
    let cli = TicketCli::parse();

    // `update` is a self-management command that must run from anywhere
    // without reading (or creating) a .tickets/ directory.
    if matches!(cli.command, Command::Update(_)) {
        let Command::Update(args) = cli.command else {
            unreachable!()
        };
        return cmd_update(args);
    }

    refresh_ticket_cache()?;

    match cli.command {
        Command::Tui => {
            color_eyre::install()?;
            ratatui::run(|terminal| TuiApp::new(&lock_tickets()?).run(terminal))?
        }
        Command::Create(args) => cmd_create(args)?,
        Command::Start(args) => cmd_start(args)?,
        Command::Close(args) => cmd_close(args)?,
        Command::Reopen(args) => cmd_reopen(args)?,
        Command::Status(args) => cmd_status(args)?,
        Command::Dep(args) => cmd_dep(args)?,
        Command::Undep(args) => cmd_undep(args)?,
        Command::Link(args) => cmd_link(args)?,
        Command::Unlink(args) => cmd_unlink(args)?,
        Command::Ls(args) => cmd_ls(args)?,
        Command::Ready(args) => cmd_ready(args)?,
        Command::Blocked(args) => cmd_blocked(args)?,
        Command::Closed(args) => cmd_closed(args)?,
        Command::Show(args) => cmd_show(args)?,
        Command::Edit(args) => cmd_edit(args)?,
        Command::AddNote(args) => cmd_add_note(args)?,
        Command::Query(args) => cmd_query(args)?,
        Command::Update(_) => unreachable!("handled before refresh_ticket_cache"),
    };

    Ok(())
}
