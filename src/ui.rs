use std::f64::consts::PI;

use chrono::Local;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::{self, Marker},
    text::{Line, Span},
    widgets::{
        Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Chart, Dataset, GraphType,
        Paragraph, Sparkline, Tabs,
        canvas::{Canvas, Circle, Line as CanvasLine},
    },
};

use crate::app::{App, DAILY_TARGET, Tab};

// ANSI colors deliberately inherit the active terminal theme.
const BG: Color = Color::Reset;
const PANEL: Color = Color::Reset;
const PANEL_BRIGHT: Color = Color::Reset;
const ODOMETER_BG: Color = Color::Black;
const INK: Color = Color::Black;
const BORDER: Color = Color::DarkGray;
const TEXT: Color = Color::White;
const MUTED: Color = Color::Gray;
const TRACK: Color = Color::DarkGray;
const LIME: Color = Color::LightGreen;
const GOLD: Color = Color::Yellow;
const AMBER: Color = Color::LightYellow;
const REDLINE: Color = Color::Red;
const RED: Color = Color::LightRed;

struct Stat<'a> {
    label: &'a str,
    value: String,
    color: Color,
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(
            Style::default()
                .fg(TEXT)
                .bg(BG)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );

    if area.width < 44 || area.height < 18 {
        render_too_small(frame, area);
        return;
    }

    let page = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(12),
        Constraint::Length(1),
    ])
    .split(horizontal_inset(area, 1));

    render_header(frame, page[0], app);
    render_tabs(frame, page[1], app.tab);
    match app.tab {
        Tab::Today => render_today(frame, page[2], app),
        Tab::Week => render_week(frame, page[2], app),
        Tab::Report => render_report(frame, page[2], app),
    }
    render_footer(frame, page[3], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    frame.render_widget(
        Block::new().style(
            Style::default()
                .fg(TEXT)
                .bg(PANEL)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
    let columns =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).split(area);
    let now = Local::now();

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " KP",
                    Style::default()
                        .fg(INK)
                        .bg(GOLD)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  KEYPULSE / ODOMETER",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                format!(" {}", now.format("%a  %b %-d  %H:%M")).to_uppercase(),
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
        ]),
        columns[0],
    );

    let (status, color, detail) = if app.save_error.is_some() {
        ("SERVICE", RED, "COUNTS IN MEMORY")
    } else if app.devices.is_empty() {
        ("IGNITION LOCKED", RED, "NO INPUT ACCESS")
    } else {
        ("ENGINE ON", LIME, "LOCAL TRIP LOG")
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                status,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                detail,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
        ])
        .alignment(Alignment::Right),
        columns[1],
    );
}

fn render_tabs(frame: &mut Frame, area: Rect, selected: Tab) {
    let tabs = Tabs::new([
        Line::from(" 01 DASH "),
        Line::from(" 02 LAPS "),
        Line::from(" 03 LOGBOOK "),
    ])
    .select(selected.index())
    .divider(Span::styled(" ", Style::default().fg(BORDER)))
    .style(
        Style::default()
            .fg(MUTED)
            .bg(BG)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_style(
        Style::default()
            .fg(INK)
            .bg(GOLD)
            .add_modifier(Modifier::BOLD),
    )
    .block(Block::new().borders(Borders::BOTTOM).border_style(BORDER));
    frame.render_widget(tabs, area);
}

fn render_today(frame: &mut Frame, area: Rect, app: &mut App) {
    let today = app.today();
    let total = app.stats.total_on(today);
    let hours = app.stats.hours_on(today);
    let pace = app.keys_per_minute();
    let peak = hours
        .iter()
        .enumerate()
        .filter(|(_, count)| **count > 0)
        .max_by_key(|(_, count)| *count)
        .map(|(hour, _)| hour);

    let narrow = area.width < 110;
    let sections = if narrow {
        Layout::vertical([Constraint::Min(10), Constraint::Length(7)]).split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)]).split(area)
    };

    render_speedometer(frame, sections[0], total);
    let device_value = if app.devices.is_empty() {
        "LOCKED".to_owned()
    } else {
        app.devices.len().to_string()
    };
    render_stat_strip(
        frame,
        sections[1],
        " PIT TELEMETRY ",
        &[
            Stat {
                label: "KPM",
                value: format_count(pace as u64),
                color: GOLD,
            },
            Stat {
                label: "GOAL",
                value: format!("{:.0}%", total as f64 / DAILY_TARGET as f64 * 100.0),
                color: LIME,
            },
            Stat {
                label: "PEAK",
                value: peak.map_or_else(|| "--".to_owned(), |hour| format!("{hour:02}:00")),
                color: AMBER,
            },
            Stat {
                label: "INPUTS",
                value: device_value,
                color: if app.devices.is_empty() { RED } else { REDLINE },
            },
        ],
        narrow,
    );
}

fn render_speedometer(frame: &mut Frame, area: Rect, total: u64) {
    let progress = (total as f64 / DAILY_TARGET as f64).clamp(0.0, 1.0);
    let odometer = format_odometer(total);
    let show_large_odometer = area.width >= 32 && area.height >= 18 && total < 1_000_000;
    let target = format!(
        "TRIP  {:.0}%  /  {}",
        progress * 100.0,
        format_count(DAILY_TARGET)
    );
    let x_per_cell = 2.5 / f64::from(area.width.saturating_sub(2).max(1));

    let canvas = Canvas::default()
        .block(panel(" KEYSTROKE SPEEDOMETER "))
        .marker(Marker::Braille)
        .x_bounds([-1.25, 1.25])
        .y_bounds([-1.02, 1.02])
        .paint(move |context| {
            let center_y = -0.04;
            let segments = 72;
            for segment in 0..segments {
                let from = segment as f64 / segments as f64;
                let to = (segment + 1) as f64 / segments as f64;
                let a1 = angle_for(from);
                let a2 = angle_for(to);
                let bezel_color = if to > 0.82 { REDLINE } else { GOLD };
                context.draw(&CanvasLine {
                    x1: 0.94 * a1.cos(),
                    y1: center_y + 0.94 * a1.sin(),
                    x2: 0.94 * a2.cos(),
                    y2: center_y + 0.94 * a2.sin(),
                    color: bezel_color,
                });

                let progress_color = if to <= progress {
                    if to > 0.82 { REDLINE } else { LIME }
                } else {
                    TRACK
                };
                context.draw(&CanvasLine {
                    x1: 0.86 * a1.cos(),
                    y1: center_y + 0.86 * a1.sin(),
                    x2: 0.86 * a2.cos(),
                    y2: center_y + 0.86 * a2.sin(),
                    color: progress_color,
                });
            }

            for tick in 0..=10 {
                let fraction = tick as f64 / 10.0;
                let angle = angle_for(fraction);
                let inner = if tick % 5 == 0 { 0.66 } else { 0.72 };
                context.draw(&CanvasLine {
                    x1: inner * angle.cos(),
                    y1: center_y + inner * angle.sin(),
                    x2: 0.81 * angle.cos(),
                    y2: center_y + 0.81 * angle.sin(),
                    color: if tick % 5 == 0 { TEXT } else { MUTED },
                });
                if tick % 2 == 0 {
                    let label = tick.to_string();
                    context.print(
                        0.58 * angle.cos() - label.len() as f64 * x_per_cell / 2.0,
                        center_y + 0.58 * angle.sin(),
                        Span::styled(
                            label,
                            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
                        ),
                    );
                }
            }

            let needle_angle = angle_for(progress);
            context.draw(&CanvasLine {
                x1: -0.10 * needle_angle.cos(),
                y1: center_y - 0.10 * needle_angle.sin(),
                x2: 0.62 * needle_angle.cos(),
                y2: center_y + 0.62 * needle_angle.sin(),
                color: AMBER,
            });
            context.draw(&Circle {
                x: 0.0,
                y: center_y,
                radius: 0.07,
                color: GOLD,
            });

            if !show_large_odometer {
                context.print(
                    -(odometer.len() as f64) * x_per_cell / 2.0,
                    -0.27,
                    Span::styled(
                        odometer.clone(),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                );
                context.print(
                    -5.0 * x_per_cell,
                    -0.42,
                    Span::styled(
                        "KEYS TODAY",
                        Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
                    ),
                );
            }
            context.print(
                -(target.len() as f64) * x_per_cell / 2.0,
                -0.70,
                Span::styled(target.clone(), Style::default().fg(LIME)),
            );
        });
    frame.render_widget(canvas, area);
    if show_large_odometer {
        render_big_odometer(frame, area, total);
    }
}

fn render_big_odometer(frame: &mut Frame, area: Rect, total: u64) {
    let width = 27.min(area.width.saturating_sub(2));
    let height = 7;
    let y = area
        .y
        .saturating_add(area.height.saturating_mul(54) / 100)
        .min(area.bottom().saturating_sub(height + 1));
    let display = Rect {
        x: area.x.saturating_add(area.width.saturating_sub(width) / 2),
        y,
        width,
        height,
    };
    let block = Block::new()
        .title(Span::styled(
            " TRIP / TODAY ",
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(GOLD)
        .style(Style::default().bg(ODOMETER_BG));
    frame.render_widget(
        Paragraph::new(odometer_art(total))
            .alignment(Alignment::Center)
            .style(Style::default().fg(TEXT).bg(ODOMETER_BG))
            .block(block),
        display,
    );
}

fn render_week(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.today();
    let days = app.stats.last_days(today, 7);
    let previous = app.stats.last_days(today - chrono::Duration::days(7), 7);
    let total: u64 = days.iter().map(|(_, value)| value).sum();
    let previous_total: u64 = previous.iter().map(|(_, value)| value).sum();
    let average = total / 7;
    let (_, best_value) = days
        .iter()
        .max_by_key(|(_, value)| *value)
        .copied()
        .unwrap_or((today, 0));
    let change = percent_change(total, previous_total);
    let stats_height = if area.height >= 18 { 5 } else { 4 };
    let pulse_height = if area.height >= 18 { 5 } else { 3 };
    let sections = Layout::vertical([
        Constraint::Length(stats_height),
        Constraint::Min(5),
        Constraint::Length(pulse_height),
    ])
    .split(area);

    render_stat_strip(
        frame,
        sections[0],
        " 7-DAY LAP DATA ",
        &[
            Stat {
                label: "TOTAL",
                value: compact_count(total),
                color: LIME,
            },
            Stat {
                label: "DAILY AVG",
                value: compact_count(average),
                color: TEXT,
            },
            Stat {
                label: "BEST",
                value: compact_count(best_value),
                color: AMBER,
            },
            Stat {
                label: "DELTA",
                value: format!("{change:+.0}%"),
                color: if change >= 0.0 { LIME } else { RED },
            },
        ],
        false,
    );

    let bars: Vec<Bar> = days
        .iter()
        .enumerate()
        .map(|(index, (date, value))| {
            let color = if index == 6 { LIME } else { GOLD };
            let bar = Bar::default()
                .value(*value)
                .label(Line::from(date.format("%a").to_string()))
                .style(Style::default().fg(color))
                .value_style(Style::default().fg(INK).bg(color));
            if sections[1].height >= 9 {
                bar.text_value(compact_count(*value))
            } else {
                bar.text_value(String::new())
            }
        })
        .collect();
    let bar_width = ((sections[1].width.saturating_sub(8) / 7).saturating_sub(1)).clamp(2, 8);
    frame.render_widget(
        BarChart::default()
            .block(panel(" DAILY DISTANCE "))
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_width)
            .bar_gap(1)
            .max(
                days.iter()
                    .map(|(_, value)| *value)
                    .max()
                    .unwrap_or(1)
                    .max(1),
            )
            .label_style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD)),
        sections[1],
    );

    let hours = app.stats.hours_on(today);
    frame.render_widget(
        Sparkline::default()
            .block(panel(" TODAY RPM  //  00 > 23 "))
            .data(hours)
            .max(hours.iter().copied().max().unwrap_or(1).max(1))
            .style(Style::default().fg(REDLINE)),
        sections[2],
    );
}

fn render_report(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.today();
    let last_30 = app.stats.last_days(today, 30);
    let all_time: u64 = app.stats.days.values().map(|day| day.total()).sum();
    let (_, best_value) = app.stats.best_day().unwrap_or((today, 0));
    let average = app.stats.average_for_days(today, 30);
    let stats_height = if area.height >= 18 { 5 } else { 4 };
    let pulse_height = if area.height >= 18 { 5 } else { 3 };
    let sections = Layout::vertical([
        Constraint::Length(stats_height),
        Constraint::Min(5),
        Constraint::Length(pulse_height),
    ])
    .split(area);

    render_stat_strip(
        frame,
        sections[0],
        " SEASON LOG ",
        &[
            Stat {
                label: "ALL TIME",
                value: compact_count(all_time),
                color: LIME,
            },
            Stat {
                label: "30D AVG",
                value: compact_count(average),
                color: TEXT,
            },
            Stat {
                label: "RECORD",
                value: compact_count(best_value),
                color: AMBER,
            },
            Stat {
                label: "STREAK",
                value: format!("{}D", app.current_streak()),
                color: REDLINE,
            },
        ],
        false,
    );

    let trend: Vec<(f64, f64)> = last_30
        .iter()
        .enumerate()
        .map(|(index, (_, count))| (index as f64, *count as f64))
        .collect();
    let max_trend = last_30
        .iter()
        .map(|(_, value)| *value)
        .max()
        .unwrap_or(1)
        .max(1);
    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(LIME))
        .data(&trend);
    frame.render_widget(
        Chart::new(vec![dataset])
            .block(panel(" 30-DAY PACE "))
            .x_axis(
                Axis::default()
                    .bounds([0.0, 29.0])
                    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
                    .labels(["-30D", "-15D", "NOW"]),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, max_trend as f64])
                    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD))
                    .labels([
                        "0".to_owned(),
                        compact_count(max_trend / 2),
                        compact_count(max_trend),
                    ]),
            ),
        sections[1],
    );

    let hours = app.stats.aggregate_hours(today, 30);
    frame.render_widget(
        Sparkline::default()
            .block(panel(" SHIFT MAP  //  00 > 23 "))
            .data(hours)
            .max(hours.iter().copied().max().unwrap_or(1).max(1))
            .style(Style::default().fg(AMBER)),
        sections[2],
    );
}

fn render_stat_strip(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    stats: &[Stat<'_>],
    stack_on_narrow: bool,
) {
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if stack_on_narrow {
        let rows =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(inner);
        let top = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]);
        let bottom = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);
        for (stat, target) in stats.iter().zip([top[0], top[1], bottom[0], bottom[1]]) {
            render_stat(frame, target, stat);
        }
    } else {
        let columns =
            Layout::horizontal(vec![Constraint::Ratio(1, stats.len() as u32); stats.len()])
                .split(inner);
        for (stat, target) in stats.iter().zip(columns.iter()) {
            render_stat(frame, *target, stat);
        }
    }
}

fn render_stat(frame: &mut Frame, area: Rect, stat: &Stat<'_>) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                stat.label,
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                &stat.value,
                Style::default().fg(stat.color).add_modifier(Modifier::BOLD),
            )),
        ])
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(TEXT)
                .bg(PANEL_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let columns =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" 1-3", Style::default().fg(TEXT)),
            Span::styled(
                " MODE   ",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Q", Style::default().fg(TEXT)),
            Span::styled(
                " PARK",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            ),
        ])),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            if app.devices.is_empty() {
                "INPUT LOCKED"
            } else {
                "TRIP LOG / SQLITE"
            },
            Style::default()
                .fg(if app.devices.is_empty() { RED } else { MUTED })
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Right),
        columns[1],
    );
}

fn panel(title: &'static str) -> Block<'static> {
    Block::new()
        .title(Span::styled(
            title,
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(BORDER)
        .style(
            Style::default()
                .fg(TEXT)
                .bg(PANEL)
                .add_modifier(Modifier::BOLD),
        )
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "KEYPULSE",
                Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "PANEL TOO SMALL",
                Style::default().fg(REDLINE),
            )),
            Line::from(Span::styled(
                "MINIMUM 44 x 18",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
        ])
        .alignment(Alignment::Center)
        .block(panel(" INPUT ODOMETER ")),
        area,
    );
}

fn horizontal_inset(area: Rect, amount: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(amount),
        y: area.y,
        width: area.width.saturating_sub(amount.saturating_mul(2)),
        height: area.height,
    }
}

fn angle_for(progress: f64) -> f64 {
    (220.0 - progress * 260.0) * PI / 180.0
}

fn percent_change(current: u64, previous: u64) -> f64 {
    if previous == 0 {
        if current == 0 { 0.0 } else { 100.0 }
    } else {
        (current as f64 - previous as f64) / previous as f64 * 100.0
    }
}

fn compact_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn format_odometer(value: u64) -> String {
    if value < 1_000_000 {
        format!("{value:06}")
    } else {
        format_count(value)
    }
}

fn odometer_art(value: u64) -> Vec<Line<'static>> {
    const DIGITS: [[&str; 5]; 10] = [
        ["███", "█ █", "█ █", "█ █", "███"],
        [" ██", "  █", "  █", "  █", " ███"],
        ["███", "  █", "███", "█  ", "███"],
        ["███", "  █", " ██", "  █", "███"],
        ["█ █", "█ █", "███", "  █", "  █"],
        ["███", "█  ", "███", "  █", "███"],
        ["███", "█  ", "███", "█ █", "███"],
        ["███", "  █", "  █", "  █", "  █"],
        ["███", "█ █", "███", "█ █", "███"],
        ["███", "█ █", "███", "  █", "███"],
    ];

    let digits = format!("{value:06}");
    (0..5)
        .map(|row| {
            let mut output = String::with_capacity(23);
            for (index, digit) in digits.bytes().enumerate() {
                if index > 0 {
                    output.push(' ');
                }
                output.push_str(DIGITS[usize::from(digit - b'0')][row]);
            }
            Line::from(output)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use crate::{
        app::{App, Tab},
        store::{Database, Stats},
    };

    use super::{format_count, format_odometer, odometer_art, percent_change, render};

    #[test]
    fn formats_counts_for_display() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(12_345_678), "12,345,678");
        assert_eq!(format_odometer(38), "000038");
        assert_eq!(odometer_art(38).len(), 5);
    }

    #[test]
    fn calculates_week_change() {
        assert_eq!(percent_change(150, 100), 50.0);
        assert_eq!(percent_change(0, 0), 0.0);
    }

    #[test]
    fn renders_every_tab() {
        let mut terminal = Terminal::new(TestBackend::new(72, 42)).unwrap();
        let mut app = App::new(Stats::default(), Database::in_memory().unwrap());
        for _ in 0..20 {
            app.record_press();
        }

        for tab in [Tab::Today, Tab::Week, Tab::Report] {
            app.tab = tab;
            terminal.draw(|frame| render(frame, &mut app)).unwrap();
        }
    }

    #[test]
    fn renders_at_minimum_supported_size() {
        let mut terminal = Terminal::new(TestBackend::new(44, 18)).unwrap();
        let mut app = App::new(Stats::default(), Database::in_memory().unwrap());

        for tab in [Tab::Today, Tab::Week, Tab::Report] {
            app.tab = tab;
            terminal.draw(|frame| render(frame, &mut app)).unwrap();
        }
    }
}
