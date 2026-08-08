use std::f64::consts::PI;

use chrono::Local;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Bar, BarChart, BarGroup, Block, BorderType, Borders, Chart, Dataset, GraphType,
        Paragraph, Sparkline, Tabs,
        canvas::{Canvas, Circle, Context, Line as CanvasLine},
    },
};

use crate::app::{App, DAILY_TARGET, Tab};

const BG: Color = Color::Reset;
const TEXT: Color = Color::Rgb(174, 176, 190);
const MUTED: Color = Color::Rgb(102, 105, 117);
const FAINT: Color = Color::Rgb(55, 58, 67);
const BORDER: Color = Color::Rgb(64, 67, 76);
const SILVER: Color = Color::Rgb(202, 205, 211);
const BLUE: Color = Color::Rgb(112, 145, 224);
const GREEN: Color = Color::Rgb(147, 190, 91);
const ORANGE: Color = Color::Rgb(222, 145, 94);
const RED: Color = Color::Rgb(218, 103, 111);

struct Stat<'a> {
    label: &'a str,
    value: String,
    color: Color,
}

pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::default().fg(TEXT).bg(BG)), area);

    if area.width < 60 || area.height < 32 {
        render_too_small(frame, area);
        return;
    }

    let page = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(23),
        Constraint::Length(1),
    ])
    .split(inset(area, 2, 1));

    render_header(frame, page[0], app);
    render_tabs(frame, page[2], app.tab);
    match app.tab {
        Tab::Live => render_live(frame, page[4], app),
        Tab::Daily => render_daily(frame, page[4], app),
        Tab::Hourly => render_hourly(frame, page[4], app),
        Tab::Records => render_records(frame, page[4], app),
    }
    render_footer(frame, page[5]);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let now = Local::now();
    let username = std::env::var("USER").unwrap_or_else(|_| "local".to_owned());
    let (status, color) = if app.refresh_error.is_some() {
        ("stale", RED)
    } else if !app.recorder_active {
        ("off", RED)
    } else if app.device_count == 0 {
        ("locked", ORANGE)
    } else {
        ("live", GREEN)
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "keycount",
                Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
            ),
            separator(),
            Span::styled(
                now.format("%Y-%m-%d").to_string(),
                Style::default().fg(MUTED),
            ),
            separator(),
            Span::styled(username, Style::default().fg(TEXT)),
            Span::raw("  "),
            Span::styled("● ", Style::default().fg(color)),
            Span::styled(status, Style::default().fg(TEXT)),
        ])),
        area,
    );
}

fn render_tabs(frame: &mut Frame, area: Rect, selected: Tab) {
    let tabs = Tabs::new([
        Line::from("  Live  "),
        Line::from("  Daily  "),
        Line::from("  Hourly  "),
        Line::from("  Records  "),
    ])
    .select(selected.index())
    .padding("", "")
    .divider(Span::styled("  │  ", Style::default().fg(FAINT)))
    .style(Style::default().fg(MUTED))
    .highlight_style(
        Style::default()
            .fg(BLUE)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
    .block(
        Block::new()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(BORDER),
    );
    frame.render_widget(tabs, area);
}

fn render_live(frame: &mut Frame, area: Rect, app: &App) {
    if area.height < 34 {
        let sections = Layout::vertical([
            Constraint::Min(12),
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(area);
        render_gauge(frame, sections[0], app);
        render_today(frame, sections[2], app);
        render_comparison(frame, sections[4], app);
        return;
    }

    let sections = Layout::vertical([
        Constraint::Min(16),
        Constraint::Length(1),
        Constraint::Length(8),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(6),
    ])
    .split(area);
    render_gauge(frame, sections[0], app);
    render_today(frame, sections[2], app);
    render_comparison(frame, sections[4], app);
    render_recent_kpm(frame, sections[6], app);
}

fn render_gauge(frame: &mut Frame, area: Rect, app: &App) {
    let title = format!(" KEYBOARD  ·  goal {} ", format_count(DAILY_TARGET));
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let kpm = app.keys_per_minute as u64;
    let max_kpm = 400.0;
    let progress = (kpm as f64 / max_kpm).clamp(0.0, 1.0);
    let today = app.stats.total_on(app.today());
    let goal = today as f64 / DAILY_TARGET as f64 * 100.0;
    let x_per_cell = 2.4 / f64::from(inner.width.max(1));
    let value_label = format!(" {} KPM ", format_count(kpm));
    let goal_label = format!("goal {goal:.0}%");

    let canvas = Canvas::default()
        .marker(symbols::Marker::Braille)
        .x_bounds([-1.2, 1.2])
        .y_bounds([-1.0, 1.0])
        .paint(move |context| {
            const SEGMENTS: usize = 120;
            for segment in 0..SEGMENTS {
                let from = segment as f64 / SEGMENTS as f64;
                let to = (segment as f64 + 0.78) / SEGMENTS as f64;
                let a1 = dial_angle(from);
                let a2 = dial_angle(to);
                context.draw(&CanvasLine {
                    x1: 1.02 * a1.cos(),
                    y1: 0.94 * a1.sin(),
                    x2: 1.02 * a2.cos(),
                    y2: 0.94 * a2.sin(),
                    color: SILVER,
                });
            }

            for tick in 0..=50 {
                let fraction = tick as f64 / 50.0;
                let angle = dial_angle(fraction);
                let major = tick % 10 == 0;
                let medium = tick % 5 == 0;
                let inner_radius = if major {
                    0.72
                } else if medium {
                    0.79
                } else {
                    0.85
                };
                context.draw(&CanvasLine {
                    x1: inner_radius * angle.cos(),
                    y1: 0.86 * inner_radius * angle.sin(),
                    x2: 0.93 * angle.cos(),
                    y2: 0.86 * 0.93 * angle.sin(),
                    color: if major { TEXT } else { MUTED },
                });
            }

            for (label, x, y) in [
                ("0", -0.84, -0.53),
                ("100", -0.84, 0.18),
                ("200", -0.06, 0.60),
                ("300", 0.70, 0.18),
                ("400", 0.82, -0.53),
            ] {
                context.print(x, y, Span::styled(label, Style::default().fg(MUTED)));
            }

            let angle = dial_angle(progress);
            context.draw(&CanvasLine {
                x1: 0.0,
                y1: -0.04,
                x2: 0.70 * angle.cos(),
                y2: 0.63 * angle.sin(),
                color: RED,
            });
            context.draw(&Circle {
                x: 0.0,
                y: -0.04,
                radius: 0.070,
                color: FAINT,
            });
            context.draw(&Circle {
                x: 0.0,
                y: -0.04,
                radius: 0.045,
                color: SILVER,
            });

            context.print(
                -0.29,
                -0.28,
                Span::styled("GROUND SPEED", Style::default().fg(FAINT)),
            );
            draw_boxed_label(context, &value_label, x_per_cell, -0.48);
            context.print(
                -0.20,
                -0.78,
                Span::styled(
                    "KEYS",
                    Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
                ),
            );
            context.print(
                -(goal_label.len() as f64) * x_per_cell / 2.0,
                -0.91,
                Span::styled(goal_label.clone(), Style::default().fg(MUTED)),
            );
        });
    frame.render_widget(canvas, inner);
}

fn render_today(frame: &mut Frame, area: Rect, app: &App) {
    let total = app.stats.total_on(app.today());
    let block = panel(" Today ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (art, art_width) = thin_number_art(total);
    if inner.height >= 6 && art_width <= inner.width {
        let mut lines = art;
        lines.push(Line::from(Span::styled(
            "keys today",
            Style::default().fg(MUTED),
        )));
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .style(Style::default().fg(BLUE)),
            inner,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format_count(total),
                    Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled("keys today", Style::default().fg(MUTED))),
            ])
            .alignment(Alignment::Center),
            inner,
        );
    }
}

fn render_comparison(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.today();
    let yesterday = app.stats.total_on(today.pred_opt().unwrap_or(today));
    let average = app.stats.average_for_days(today, 7);
    let streak = app.current_streak();
    let block = panel(" vs. ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Yesterday ", Style::default().fg(MUTED)),
            Span::styled(format_count(yesterday), Style::default().fg(TEXT)),
            separator(),
            Span::styled("7-day avg ", Style::default().fg(MUTED)),
            Span::styled(format_count(average), Style::default().fg(TEXT)),
            separator(),
            Span::styled("Streak ", Style::default().fg(MUTED)),
            Span::styled(format!("{streak} d"), Style::default().fg(ORANGE)),
            separator(),
        ]))
        .alignment(Alignment::Center),
        inner,
    );
}

fn render_recent_kpm(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel(" KPM (recent) ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let slots = usize::from(inner.width / 4).max(1);
    let visible = app.kpm_history.len().min(slots);
    let mut values = vec![0; slots.saturating_sub(visible)];
    values.extend(app.kpm_history.iter().skip(app.kpm_history.len() - visible));
    let bars: Vec<Bar> = values
        .iter()
        .map(|value| {
            Bar::default()
                .value(*value)
                .style(Style::default().fg(BLUE))
                .value_style(Style::default().fg(BLUE))
                .text_value(String::new())
        })
        .collect();
    let maximum = values.iter().copied().max().unwrap_or(1).max(60);
    frame.render_widget(
        BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(1)
            .bar_gap(3)
            .max(maximum),
        inner,
    );
}

fn render_daily(frame: &mut Frame, area: Rect, app: &App) {
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
    let sections = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(1),
        Constraint::Length(6),
    ])
    .split(area);

    render_stat_strip(
        frame,
        sections[0],
        " 7-day summary ",
        &[
            Stat {
                label: "total",
                value: compact_count(total),
                color: BLUE,
            },
            Stat {
                label: "daily avg",
                value: compact_count(average),
                color: TEXT,
            },
            Stat {
                label: "best",
                value: compact_count(best_value),
                color: GREEN,
            },
            Stat {
                label: "change",
                value: format!("{:+.0}%", percent_change(total, previous_total)),
                color: ORANGE,
            },
        ],
    );

    let bars: Vec<Bar> = days
        .iter()
        .enumerate()
        .map(|(index, (date, value))| {
            Bar::default()
                .value(*value)
                .label(Line::from(date.format("%a").to_string()))
                .style(Style::default().fg(if index == 6 { GREEN } else { BLUE }))
                .value_style(Style::default().fg(TEXT))
                .text_value(if sections[2].height >= 9 {
                    compact_count(*value)
                } else {
                    String::new()
                })
        })
        .collect();
    frame.render_widget(
        BarChart::default()
            .block(panel(" Daily keys "))
            .data(BarGroup::default().bars(&bars))
            .bar_width(((sections[2].width.saturating_sub(9) / 7).saturating_sub(1)).clamp(2, 8))
            .bar_gap(1)
            .max(
                days.iter()
                    .map(|(_, value)| *value)
                    .max()
                    .unwrap_or(1)
                    .max(1),
            )
            .label_style(Style::default().fg(MUTED)),
        sections[2],
    );

    let hours = app.stats.hours_on(today);
    let hourly_plot = stretch_values(&hours, sections[4].width.saturating_sub(2) as usize);
    frame.render_widget(
        Sparkline::default()
            .block(panel(" Today · 00—23 "))
            .data(hourly_plot)
            .max(hours.iter().copied().max().unwrap_or(1).max(1))
            .style(Style::default().fg(BLUE)),
        sections[4],
    );
}

fn render_hourly(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.today();
    let hours = app.stats.hours_on(today);
    let total = app.stats.total_on(today);
    let peak = hours
        .iter()
        .enumerate()
        .filter(|(_, value)| **value > 0)
        .max_by_key(|(_, value)| *value)
        .map(|(hour, value)| (hour, *value));
    let peak_hour = peak.map(|(hour, _)| hour);
    let peak_value = peak.map_or(0, |(_, value)| value);
    let active_hours = hours.iter().filter(|value| **value > 0).count();
    let sections = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(12),
    ])
    .split(area);

    render_stat_strip(
        frame,
        sections[0],
        " Today by hour ",
        &[
            Stat {
                label: "keys",
                value: format_count(total),
                color: BLUE,
            },
            Stat {
                label: "peak hour",
                value: peak_hour.map_or_else(|| "--".to_owned(), |hour| format!("{hour:02}:00")),
                color: GREEN,
            },
            Stat {
                label: "peak keys",
                value: format_count(peak_value),
                color: TEXT,
            },
            Stat {
                label: "active hours",
                value: active_hours.to_string(),
                color: ORANGE,
            },
        ],
    );

    let bars: Vec<Bar> = hours
        .iter()
        .enumerate()
        .map(|(hour, value)| {
            Bar::default()
                .value(*value)
                .style(Style::default().fg(if Some(hour) == peak_hour { GREEN } else { BLUE }))
                .value_style(Style::default().fg(TEXT))
                .text_value(String::new())
        })
        .collect();
    let block = panel(" Hourly distribution · 00—23 ");
    let inner = block.inner(sections[2]);
    frame.render_widget(block, sections[2]);
    let chart = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let bar_width = if chart[0].width >= 76 { 2 } else { 1 };
    let bar_gap = 1;
    let used_width = 24 * bar_width + 23 * bar_gap;
    let bar_area = Rect {
        x: chart[0].x + chart[0].width.saturating_sub(used_width) / 2,
        y: chart[0].y,
        width: used_width.min(chart[0].width),
        height: chart[0].height,
    };
    frame.render_widget(
        BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_width)
            .bar_gap(bar_gap)
            .max(hours.iter().copied().max().unwrap_or(1).max(1))
            .label_style(Style::default().fg(MUTED)),
        bar_area,
    );
    render_hour_axis(frame, chart[1], bar_area, bar_width, bar_gap);
}

fn render_records(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.today();
    let last_30 = app.stats.last_days(today, 30);
    let all_time: u64 = app.stats.days.values().map(|day| day.total()).sum();
    let (_, best_value) = app.stats.best_day().unwrap_or((today, 0));
    let average = app.stats.average_for_days(today, 30);
    let sections = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(1),
        Constraint::Length(6),
    ])
    .split(area);

    render_stat_strip(
        frame,
        sections[0],
        " Records ",
        &[
            Stat {
                label: "all time",
                value: compact_count(all_time),
                color: BLUE,
            },
            Stat {
                label: "30-day avg",
                value: compact_count(average),
                color: TEXT,
            },
            Stat {
                label: "best day",
                value: compact_count(best_value),
                color: GREEN,
            },
            Stat {
                label: "streak",
                value: format!("{} d", app.current_streak()),
                color: ORANGE,
            },
        ],
    );

    let trend: Vec<(f64, f64)> = last_30
        .iter()
        .enumerate()
        .map(|(index, (_, count))| (index as f64, *count as f64))
        .collect();
    let maximum = last_30
        .iter()
        .map(|(_, value)| *value)
        .max()
        .unwrap_or(1)
        .max(1);
    let dataset = Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(BLUE))
        .data(&trend);
    frame.render_widget(
        Chart::new(vec![dataset])
            .block(panel(" 30-day trend "))
            .x_axis(
                Axis::default()
                    .bounds([0.0, 29.0])
                    .style(Style::default().fg(MUTED))
                    .labels(["-30d", "-15d", "now"]),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, maximum as f64])
                    .style(Style::default().fg(MUTED))
                    .labels([
                        "0".to_owned(),
                        compact_count(maximum / 2),
                        compact_count(maximum),
                    ]),
            ),
        sections[2],
    );

    let aggregate = app.stats.aggregate_hours(today, 30);
    let aggregate_plot = stretch_values(&aggregate, sections[4].width.saturating_sub(2) as usize);
    frame.render_widget(
        Sparkline::default()
            .block(panel(" 30-day rhythm · 00—23 "))
            .data(aggregate_plot)
            .max(aggregate.iter().copied().max().unwrap_or(1).max(1))
            .style(Style::default().fg(GREEN)),
        sections[4],
    );
}

fn render_stat_strip(frame: &mut Frame, area: Rect, title: impl Into<String>, stats: &[Stat<'_>]) {
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let columns = Layout::horizontal(vec![Constraint::Ratio(1, stats.len() as u32); stats.len()])
        .split(inner);
    for (stat, target) in stats.iter().zip(columns.iter()) {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(stat.label, Style::default().fg(MUTED))),
                Line::from(Span::styled(
                    &stat.value,
                    Style::default().fg(stat.color).add_modifier(Modifier::BOLD),
                )),
            ])
            .alignment(Alignment::Center),
            *target,
        );
    }
}

fn render_footer(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            key("q"),
            hint(" quit   "),
            key("Tab"),
            hint(" switch   "),
            key("1-4"),
            hint(" jump   "),
            key("r"),
            hint(" refresh"),
        ])),
        area,
    );
}

fn render_hour_axis(frame: &mut Frame, area: Rect, bars: Rect, bar_width: u16, bar_gap: u16) {
    for (hour, label) in [(0, "00"), (6, "06"), (12, "12"), (18, "18"), (23, "23")] {
        let center = bars.x + hour * (bar_width + bar_gap) + bar_width / 2;
        let target = Rect {
            x: center.saturating_sub(1),
            y: area.y,
            width: 2,
            height: area.height,
        };
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(MUTED)))
                .alignment(Alignment::Center),
            target,
        );
    }
}

fn panel(title: impl Into<String>) -> Block<'static> {
    Block::new()
        .title(Span::styled(title.into(), Style::default().fg(MUTED)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(BORDER)
        .style(Style::default().fg(TEXT).bg(BG))
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "keycount",
                Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "window too small",
                Style::default().fg(ORANGE),
            )),
            Line::from(Span::styled("minimum 60 × 32", Style::default().fg(MUTED))),
        ])
        .alignment(Alignment::Center)
        .block(panel(" live ")),
        area,
    );
}

fn separator() -> Span<'static> {
    Span::styled("  ·  ", Style::default().fg(FAINT))
}

fn key(value: &'static str) -> Span<'static> {
    Span::styled(
        value,
        Style::default().fg(BLUE).add_modifier(Modifier::BOLD),
    )
}

fn hint(value: &'static str) -> Span<'static> {
    Span::styled(value, Style::default().fg(MUTED))
}

fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(horizontal),
        y: area.y.saturating_add(vertical),
        width: area.width.saturating_sub(horizontal.saturating_mul(2)),
        height: area.height.saturating_sub(vertical.saturating_mul(2)),
    }
}

fn dial_angle(progress: f64) -> f64 {
    (220.0 - progress * 260.0) * PI / 180.0
}

fn draw_boxed_label(context: &mut Context<'_>, label: &str, x_per_cell: f64, y: f64) {
    let half_width = (label.len() as f64 * x_per_cell + 0.12) / 2.0;
    let left = -half_width;
    let right = half_width;
    let top = y + 0.09;
    let bottom = y - 0.09;
    for line in [
        CanvasLine {
            x1: left,
            y1: top,
            x2: right,
            y2: top,
            color: MUTED,
        },
        CanvasLine {
            x1: left,
            y1: bottom,
            x2: right,
            y2: bottom,
            color: MUTED,
        },
        CanvasLine {
            x1: left,
            y1: bottom,
            x2: left,
            y2: top,
            color: MUTED,
        },
        CanvasLine {
            x1: right,
            y1: bottom,
            x2: right,
            y2: top,
            color: MUTED,
        },
    ] {
        context.draw(&line);
    }
    context.print(
        -(label.len() as f64) * x_per_cell / 2.0,
        y - 0.03,
        Span::styled(
            label.to_owned(),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        ),
    );
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

fn stretch_values(values: &[u64], width: usize) -> Vec<u64> {
    if values.is_empty() || width == 0 {
        return Vec::new();
    }
    (0..width)
        .map(|column| values[column * values.len() / width])
        .collect()
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

fn thin_number_art(value: u64) -> (Vec<Line<'static>>, u16) {
    const DIGITS: [[&str; 5]; 10] = [
        ["╭─╮", "│ │", "│ │", "│ │", "╰─╯"],
        ["  ╷", "  │", "  │", "  │", "  ╵"],
        ["╭─╮", "  │", "╭─╯", "│  ", "╰─╴"],
        ["╭─╮", "  │", " ─┤", "  │", "╰─╯"],
        ["╷ ╷", "│ │", "╰─┤", "  │", "  ╵"],
        ["╭─╴", "│  ", "╰─╮", "  │", "╰─╯"],
        ["╭─╴", "│  ", "├─╮", "│ │", "╰─╯"],
        ["╭─╮", "  │", "  │", "  │", "  ╵"],
        ["╭─╮", "│ │", "├─┤", "│ │", "╰─╯"],
        ["╭─╮", "│ │", "╰─┤", "  │", "  ╵"],
    ];
    const COMMA: [&str; 5] = [" ", " ", " ", " ", "╵"];

    let display = format_count(value);
    let mut rows = [
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
    ];
    let mut width = 0_u16;
    for (index, character) in display.chars().enumerate() {
        if index > 0 {
            for row in &mut rows {
                row.push(' ');
            }
            width += 1;
        }
        let glyph = if character == ',' {
            &COMMA
        } else {
            &DIGITS[character.to_digit(10).expect("count contains only digits") as usize]
        };
        for (row, segment) in rows.iter_mut().zip(glyph.iter()) {
            row.push_str(segment);
        }
        width += glyph[0].chars().count() as u16;
    }
    (rows.into_iter().map(Line::from).collect(), width)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use crate::{
        app::{App, Tab},
        store::{Database, Stats},
    };

    use super::{format_count, percent_change, render, thin_number_art};

    #[test]
    fn formats_counts_for_display() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(12_345_678), "12,345,678");
        let (art, width) = thin_number_art(12_356);
        assert_eq!(art.len(), 5);
        assert_eq!(width, 21);
    }

    #[test]
    fn calculates_week_change() {
        assert_eq!(percent_change(150, 100), 50.0);
        assert_eq!(percent_change(0, 0), 0.0);
    }

    #[test]
    fn renders_every_tab() {
        let mut terminal = Terminal::new(TestBackend::new(86, 48)).unwrap();
        for tab in [Tab::Live, Tab::Daily, Tab::Hourly, Tab::Records] {
            let mut app = App::new(Stats::default(), Database::in_memory().unwrap());
            app.tab = tab;
            terminal.draw(|frame| render(frame, &app)).unwrap();
        }
    }

    #[test]
    fn live_page_matches_reference_structure() {
        let mut terminal = Terminal::new(TestBackend::new(60, 32)).unwrap();
        let app = App::new(Stats::default(), Database::in_memory().unwrap());

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let output: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        for label in [
            "keycount",
            "Live",
            "KEYBOARD",
            "GROUND SPEED",
            "Today",
            "Yesterday",
        ] {
            assert!(output.contains(label), "missing {label:?}\n{output}");
        }
    }
}
