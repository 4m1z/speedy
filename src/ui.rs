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

use crate::app::{App, Tab};

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
    if app.editing_target {
        render_target_editor(frame, area, app);
    }
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

    let mut spans = vec![
        Span::styled(
            "speedy",
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
    ];
    if app.recorder_active && app.device_count > 0 {
        spans.push(Span::styled(
            format!("  {} KPM", format_count(app.keys_per_minute as u64)),
            Style::default().fg(speed_color(app.keys_per_minute as u64)),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
    let target = app.effective_target();
    let title = format!(" KEYBOARD  ·  goal {} ", format_count(target));
    let block = panel(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let kpm = app.keys_per_minute as u64;
    let max_kpm = 400.0;
    let progress = (kpm as f64 / max_kpm).clamp(0.0, 1.0);
    let today = app.stats.total_on(app.today());
    let goal = today as f64 / target as f64 * 100.0;
    let speed = speed_color(kpm);
    let x_per_cell = 2.4 / f64::from(inner.width.max(1));
    let value_label = format!(" {} KPM ", format_count(kpm));
    let goal_label = format!("goal {goal:.0}%");

    let canvas = Canvas::default()
        .marker(symbols::Marker::Braille)
        .x_bounds([-1.2, 1.2])
        .y_bounds([-1.0, 1.0])
        .paint(move |context| {
            const SEGMENTS: usize = 120;
            // Dim full track first so the active arc pops.
            for segment in 0..SEGMENTS {
                let from = segment as f64 / SEGMENTS as f64;
                let to = (segment as f64 + 0.78) / SEGMENTS as f64;
                let a1 = dial_angle(from);
                let a2 = dial_angle(to);
                let lit = from <= progress;
                let color = if lit { speed } else { FAINT };
                // Double-stroke the lit arc for a bolder, glowing sweep.
                let outer = if lit { 1.03 } else { 1.02 };
                context.draw(&CanvasLine {
                    x1: outer * a1.cos(),
                    y1: 0.94 * a1.sin(),
                    x2: outer * a2.cos(),
                    y2: 0.94 * a2.sin(),
                    color,
                });
                if lit {
                    context.draw(&CanvasLine {
                        x1: 0.985 * a1.cos(),
                        y1: 0.905 * a1.sin(),
                        x2: 0.985 * a2.cos(),
                        y2: 0.905 * a2.sin(),
                        color,
                    });
                }
            }

            // Glowing tip dot at the leading edge.
            if progress > 0.005 {
                let tip = dial_angle(progress);
                context.draw(&Circle {
                    x: 1.01 * tip.cos(),
                    y: 0.93 * tip.sin(),
                    radius: 0.032,
                    color: speed,
                });
            }

            for tick in 0..=50 {
                let fraction = tick as f64 / 50.0;
                let angle = dial_angle(fraction);
                let major = tick % 10 == 0;
                let medium = tick % 5 == 0;
                let inner_radius = if major {
                    0.70
                } else if medium {
                    0.78
                } else {
                    0.845
                };
                let passed = fraction <= progress + f64::EPSILON;
                context.draw(&CanvasLine {
                    x1: inner_radius * angle.cos(),
                    y1: 0.86 * inner_radius * angle.sin(),
                    x2: 0.93 * angle.cos(),
                    y2: 0.86 * 0.93 * angle.sin(),
                    color: if passed {
                        speed
                    } else if major {
                        TEXT
                    } else {
                        MUTED
                    },
                });
            }

            for (label, x, y, threshold) in [
                ("0", -0.84, -0.53, 0.0),
                ("100", -0.84, 0.18, 0.25),
                ("200", -0.06, 0.60, 0.50),
                ("300", 0.70, 0.18, 0.75),
                ("400", 0.82, -0.53, 1.0),
            ] {
                let reached = progress + 0.02 >= threshold;
                context.print(
                    x,
                    y,
                    Span::styled(
                        label,
                        Style::default().fg(if reached { SILVER } else { MUTED }),
                    ),
                );
            }

            let angle = dial_angle(progress);
            let tip_x = 0.78 * angle.cos();
            let tip_y = 0.70 * angle.sin();
            // Soft shadow under the needle for depth, then the needle itself.
            context.draw(&CanvasLine {
                x1: -0.14 * angle.cos(),
                y1: -0.04 - 0.13 * angle.sin(),
                x2: tip_x,
                y2: tip_y - 0.04,
                color: FAINT,
            });
            context.draw(&CanvasLine {
                x1: -0.16 * angle.cos(),
                y1: -0.04 - 0.14 * angle.sin(),
                x2: tip_x,
                y2: tip_y - 0.035,
                color: SILVER,
            });
            context.draw(&CanvasLine {
                x1: 0.0,
                y1: -0.04,
                x2: tip_x,
                y2: tip_y - 0.035,
                color: RED,
            });
            context.draw(&Circle {
                x: 0.0,
                y: -0.04,
                radius: 0.075,
                color: FAINT,
            });
            context.draw(&Circle {
                x: 0.0,
                y: -0.04,
                radius: 0.058,
                color: speed,
            });
            context.draw(&Circle {
                x: 0.0,
                y: -0.04,
                radius: 0.036,
                color: SILVER,
            });

            context.print(
                -0.29,
                -0.28,
                Span::styled("GROUND SPEED", Style::default().fg(MUTED)),
            );
            draw_boxed_label(context, &value_label, speed, x_per_cell, -0.48);
            context.print(
                -0.20,
                -0.78,
                Span::styled(
                    "KEYS / MIN",
                    Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
                ),
            );
            context.print(
                -(goal_label.len() as f64) * x_per_cell / 2.0,
                -0.91,
                Span::styled(
                    goal_label.clone(),
                    Style::default().fg(if goal >= 100.0 { GREEN } else { MUTED }),
                ),
            );
        });
    frame.render_widget(canvas, inner);
}

fn render_today(frame: &mut Frame, area: Rect, app: &App) {
    let target = app.effective_target();
    let total = app.stats.total_on(app.today());
    let goal_frac = (total as f64 / target as f64).clamp(0.0, 1.0);
    let number_color = if total >= target { GREEN } else { BLUE };
    let block = panel(format!(
        " Today  ·  {}% of {} ",
        (total as f64 / target as f64 * 100.0) as u64,
        format_count(target)
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let bar = goal_bar(goal_frac, inner.width.saturating_sub(4) as usize);
    let (art, art_width) = thin_number_art(total);
    if inner.height >= 6 && art_width <= inner.width {
        let mut lines = art;
        lines.push(Line::from(vec![
            Span::styled("keys today  ", Style::default().fg(MUTED)),
            Span::styled(bar, Style::default().fg(number_color)),
        ]));
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .style(Style::default().fg(number_color)),
            inner,
        );
    } else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format_count(total),
                    Style::default()
                        .fg(number_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(vec![
                    Span::styled("keys today  ", Style::default().fg(MUTED)),
                    Span::styled(bar, Style::default().fg(number_color)),
                ]),
            ])
            .alignment(Alignment::Center),
            inner,
        );
    }
}

fn render_comparison(frame: &mut Frame, area: Rect, app: &App) {
    let today = app.today();
    let today_total = app.stats.total_on(today);
    let yesterday = app.stats.total_on(today.pred_opt().unwrap_or(today));
    let average = app.stats.average_for_days(today, 7);
    let streak = app.current_streak();
    let (arrow, arrow_color) = if today_total > yesterday {
        (" ▲", GREEN)
    } else if today_total < yesterday {
        (" ▼", ORANGE)
    } else {
        (" ━", MUTED)
    };
    let block = panel(" vs. ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Yesterday ", Style::default().fg(MUTED)),
            Span::styled(format_count(yesterday), Style::default().fg(TEXT)),
            Span::styled(arrow, Style::default().fg(arrow_color)),
            separator(),
            Span::styled("7-day avg ", Style::default().fg(MUTED)),
            Span::styled(format_count(average), Style::default().fg(TEXT)),
            separator(),
            Span::styled("Streak ", Style::default().fg(MUTED)),
            Span::styled(format!("{streak} d"), Style::default().fg(ORANGE)),
        ]))
        .alignment(Alignment::Center),
        inner,
    );
}

fn render_recent_kpm(frame: &mut Frame, area: Rect, app: &App) {
    let peak = app.kpm_history.iter().copied().max().unwrap_or(0);
    let block = panel(format!(" KPM recent · peak {} ", format_count(peak)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let slots = usize::from(inner.width / 4).max(1);
    let visible = app.kpm_history.len().min(slots);
    let mut values = vec![0; slots.saturating_sub(visible)];
    values.extend(app.kpm_history.iter().skip(app.kpm_history.len() - visible));
    let maximum = values.iter().copied().max().unwrap_or(1).max(60);
    let bars: Vec<Bar> = values
        .iter()
        .map(|value| {
            let color = speed_color(*value);
            Bar::default()
                .value(*value)
                .style(Style::default().fg(color))
                .value_style(Style::default().fg(color))
                .text_value(String::new())
        })
        .collect();
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
            hint(" refresh   "),
            key("t"),
            hint(" target   "),
            hint("click tabs to jump"),
        ])),
        area,
    );
}

fn render_target_editor(frame: &mut Frame, area: Rect, app: &App) {
    use ratatui::widgets::Clear;
    let width = 46_u16;
    let height = if app.target_error.is_some() { 7 } else { 6 };
    let dialog = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width: width.min(area.width),
        height: height.min(area.height),
    };
    frame.render_widget(Clear, dialog);
    let block = panel(format!(
        " Daily target · current {} ",
        format_count(app.effective_target())
    ));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);
    let mut lines = vec![
        Line::from(Span::styled(
            "Enter a positive number of keypresses:",
            Style::default().fg(TEXT),
        )),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{}_", app.target_input),
                Style::default().fg(SILVER).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    if let Some(error) = &app.target_error {
        lines.push(Line::from(Span::styled(
            error.clone(),
            Style::default().fg(RED),
        )));
    }
    lines.push(Line::from(vec![
        key("Enter"),
        hint(" save   "),
        key("Esc"),
        hint(" cancel"),
    ]));
    frame.render_widget(Paragraph::new(lines), inner);
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
                "speedy",
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

fn speed_color(kpm: u64) -> Color {
    if kpm >= 250 {
        RED
    } else if kpm >= 150 {
        ORANGE
    } else if kpm >= 60 {
        BLUE
    } else if kpm > 0 {
        GREEN
    } else {
        MUTED
    }
}

fn goal_bar(fraction: f64, width: usize) -> String {
    let width = width.clamp(6, 18);
    let filled = (fraction * width as f64).round() as usize;
    let mut bar = String::with_capacity(width + 2);
    bar.push('[');
    for i in 0..width {
        bar.push(if i < filled { '━' } else { '─' });
    }
    bar.push(']');
    bar
}

fn draw_boxed_label(
    context: &mut Context<'_>,
    label: &str,
    accent: Color,
    x_per_cell: f64,
    y: f64,
) {
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
            color: accent,
        },
        CanvasLine {
            x1: left,
            y1: bottom,
            x2: right,
            y2: bottom,
            color: accent,
        },
        CanvasLine {
            x1: left,
            y1: bottom,
            x2: left,
            y2: top,
            color: accent,
        },
        CanvasLine {
            x1: right,
            y1: bottom,
            x2: right,
            y2: top,
            color: accent,
        },
    ] {
        context.draw(&line);
    }
    context.print(
        -(label.len() as f64) * x_per_cell / 2.0,
        y - 0.03,
        Span::styled(
            label.to_owned(),
            Style::default().fg(SILVER).add_modifier(Modifier::BOLD),
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

    use super::{format_count, goal_bar, percent_change, render, speed_color, thin_number_art};

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
            "speedy",
            "Live",
            "KEYBOARD",
            "GROUND SPEED",
            "Today",
            "Yesterday",
        ] {
            assert!(output.contains(label), "missing {label:?}\n{output}");
        }
    }

    #[test]
    fn speed_colors_follow_zones() {
        assert_eq!(speed_color(0), super::MUTED);
        assert_eq!(speed_color(10), super::GREEN);
        assert_eq!(speed_color(100), super::BLUE);
        assert_eq!(speed_color(200), super::ORANGE);
        assert_eq!(speed_color(300), super::RED);
    }

    #[test]
    fn goal_bar_fills_proportionally() {
        assert_eq!(goal_bar(0.0, 8), "[────────]");
        assert_eq!(goal_bar(1.0, 8), "[━━━━━━━━]");
        assert_eq!(goal_bar(0.5, 8), "[━━━━────]");
    }

    #[test]
    fn live_page_uses_custom_target_for_progress() {
        use chrono::Local;
        let database = Database::in_memory().unwrap();
        database.set_daily_target(2_000).unwrap();
        let mut app = App::new(Stats::default(), database);
        assert_eq!(app.daily_target, 2_000);
        // Simulate 1,000 keys today => 50% of the custom 2,000 target.
        let today = Local::now().date_naive();
        app.stats.days.entry(today).or_default().hours[9] = 1_000;

        let mut terminal = Terminal::new(TestBackend::new(86, 48)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let output: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(output.contains("2,000"), "missing custom target\n{output}");
        assert!(output.contains("50%"), "missing custom progress\n{output}");
        assert!(!output.contains("10,000"), "stale default target\n{output}");
    }

    #[test]
    fn target_editor_overlays_prompt() {
        let mut terminal = Terminal::new(TestBackend::new(86, 48)).unwrap();
        let mut app = App::new(Stats::default(), Database::in_memory().unwrap());
        app.start_editing_target();

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let output: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(output.contains("Daily target"), "missing editor\n{output}");
        assert!(output.contains("positive number"), "missing hint\n{output}");
    }
}
