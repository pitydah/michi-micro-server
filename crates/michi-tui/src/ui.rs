use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Row, Table},
    Frame,
};

use crate::app::{App, Screen};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    draw_main(f, chunks[0], app);
    draw_status(f, chunks[1], app);
}

fn draw_main(f: &mut Frame, area: Rect, app: &App) {
    match &app.screen {
        Screen::Search => draw_search(f, area, app),
        Screen::Albums => draw_albums(f, area, app),
        Screen::Artists => draw_artists(f, area, app),
        Screen::AlbumTracks(_) | Screen::ArtistTracks(_) | Screen::Tracks => {
            draw_tracks(f, area, app)
        }
        Screen::Quit => {}
    }
}

fn draw_tracks(f: &mut Frame, area: Rect, app: &App) {
    let title = match &app.screen {
        Screen::AlbumTracks(a) => format!(" Album: {a} "),
        Screen::ArtistTracks(a) => format!(" Artist: {a} "),
        _ => String::from(" Tracks "),
    };

    let header = vec![
        "Title".to_string(),
        "Artist".to_string(),
        "Album".to_string(),
        "Dur.".to_string(),
    ];

    let rows: Vec<Row> = app
        .current_tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Green)
            } else {
                Style::default()
            };
            let cells = vec![
                t.title.clone().unwrap_or_else(|| "—".to_string()),
                t.artist.clone().unwrap_or_else(|| "—".to_string()),
                t.album.clone().unwrap_or_else(|| "—".to_string()),
                fmt_dur(t.duration_ms),
            ];
            Row::new(cells).style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(35),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(15),
        ],
    )
    .header(
        Row::new(header).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(title)
            .title_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn draw_albums(f: &mut Frame, area: Rect, app: &App) {
    let header = vec![
        "Album".to_string(),
        "Artist".to_string(),
        "Tracks".to_string(),
    ];
    let rows: Vec<Row> = app
        .albums
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Green)
            } else {
                Style::default()
            };
            let cells = vec![
                a.album.clone(),
                a.album_artist.clone().unwrap_or_else(|| "—".to_string()),
                a.track_count.to_string(),
            ];
            Row::new(cells).style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ],
    )
    .header(
        Row::new(header).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Albums ")
            .title_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn draw_artists(f: &mut Frame, area: Rect, app: &App) {
    let header = vec!["Artist".to_string(), "Tracks".to_string()];
    let rows: Vec<Row> = app
        .artists
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let style = if i == app.selected {
                Style::default().fg(Color::Black).bg(Color::Green)
            } else {
                Style::default()
            };
            let cells = vec![
                a.artist.clone().unwrap_or_else(|| "Unknown".to_string()),
                a.track_count.to_string(),
            ];
            Row::new(cells).style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        [Constraint::Percentage(70), Constraint::Percentage(30)],
    )
    .header(
        Row::new(header).style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Artists ")
            .title_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, area);
}

fn draw_search(f: &mut Frame, area: Rect, app: &App) {
    let input = Paragraph::new(app.search_query.as_str())
        .style(Style::default().fg(Color::Green))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Search (Esc to cancel) "),
        );
    f.render_widget(input, area);

    if !app.current_tracks.is_empty() {
        let results: Vec<ListItem> = app
            .current_tracks
            .iter()
            .map(|t| {
                ListItem::new(format!(
                    "{} — {}",
                    t.title.as_deref().unwrap_or("Unknown"),
                    t.artist.as_deref().unwrap_or("Unknown")
                ))
            })
            .collect();
        let list =
            List::new(results).block(Block::default().title(" Results ").borders(Borders::ALL));
        f.render_widget(list, area);
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let mode = match &app.screen {
        Screen::Tracks => "Tracks[1]",
        Screen::Albums => "Albums[2]",
        Screen::Artists => "Artists[3]",
        Screen::Search => "Search[/]",
        Screen::AlbumTracks(_) => "Album",
        Screen::ArtistTracks(_) => "Artist",
        Screen::Quit => "",
    };

    let hint = "[1]Tracks [2]Albums [3]Artists [/]Search [↑↓]Nav [Enter]Select [Space]Play [q]Quit";

    let status_text = if let Some(ref e) = app.error {
        format!(" ERROR: {e} ")
    } else {
        format!(" {} | {mode} ", app.status)
    };

    let line = Line::from(vec![
        Span::styled(status_text, Style::default().fg(Color::Yellow)),
        Span::raw(" "),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);

    let paragraph = Paragraph::new(line).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(paragraph, area);
}

fn fmt_dur(ms: Option<u64>) -> String {
    match ms {
        Some(ms) => {
            let total = ms / 1000;
            format!("{}:{:02}", total / 60, total % 60)
        }
        None => "—".to_string(),
    }
}
