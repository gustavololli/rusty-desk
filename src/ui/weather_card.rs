use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use super::retro::{self, BORDER, BUTTON, CYAN, FIELD, GOLD_LIGHT, IVORY, MUTED, NIGHT};
use crate::weather::{describe_wmo_code, WeatherHour, WeatherReport};

pub fn show(ui: &mut egui::Ui, message_index: usize, report: &WeatherReport) {
    ui.push_id(("weather", message_index), |ui| {
        egui::Frame::none()
            .fill(NIGHT)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 5.0);

                let city = report.location_label.split(',').next().unwrap_or_default();
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(city).strong().size(16.0).color(GOLD_LIGHT))
                            .truncate(),
                    )
                    .on_hover_text(&report.location_label);
                    if let Some(time) = local_hour(&report.current_time) {
                        ui.label(small(time, MUTED));
                    }
                });
                if report.stale {
                    ui.label(small("Datos guardados · sin actualizar", GOLD_LIGHT));
                }

                ui.horizontal_wrapped(|ui| {
                    weather_icon(ui, Some(report.weather_code), report.is_day == Some(0), 5.0);
                    ui.label(
                        RichText::new(format!("{:.0}°C", report.temperature_c))
                            .size(30.0)
                            .strong()
                            .color(IVORY),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(describe_wmo_code(report.weather_code).1)
                                .strong()
                                .color(IVORY),
                        );
                        let mut range = Vec::new();
                        if let Some(max) = report.temp_max_c {
                            range.push(format!("Máx {max:.0}°"));
                        }
                        if let Some(min) = report.temp_min_c {
                            range.push(format!("Mín {min:.0}°"));
                        }
                        if !range.is_empty() {
                            ui.label(small(range.join("  "), MUTED));
                        }
                    });
                });

                ui.horizontal_wrapped(|ui| {
                    if let Some(feels) = report.apparent_temp_c {
                        metric(ui, "Sensación", format!("{feels:.0}°C"));
                    }
                    metric(ui, "Viento", format!("{:.0} km/h", report.wind_kmh));
                    if let Some(humidity) = report.humidity_pct {
                        metric(ui, "Humedad", format!("{humidity}%"));
                    }
                });

                let hours: Vec<_> = report.upcoming_hours().take(6).collect();
                if !hours.is_empty() {
                    ui.add_space(3.0);
                    ui.label(RichText::new("Próximas horas").strong().color(IVORY));
                    let gap = ui.spacing().item_spacing.x;
                    let columns = ((ui.available_width() + gap) / (HOUR_TILE_WIDTH + gap))
                        .floor()
                        .clamp(1.0, 6.0) as usize;
                    for row in hours.chunks(columns) {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            for hour in row {
                                hour_tile(ui, hour);
                            }
                        });
                    }
                    ui.label(small("% de precipitación por hora", MUTED));
                }

                if report.gust_kmh.is_some() || report.air_measurements().next().is_some() {
                    egui::CollapsingHeader::new(small("Ráfagas y calidad del aire", IVORY)).show(
                        ui,
                        |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if let Some(gusts) = report.gust_kmh {
                                    metric(ui, "Ráfagas", format!("{gusts:.0} km/h"));
                                }
                                for (label, value) in report.air_measurements() {
                                    metric(ui, label, format!("{value:.1} µg/m³"));
                                }
                            });
                        },
                    );
                }
                ui.label(small("Open‑Meteo", MUTED));
            });
    });
}

fn local_hour(time: &str) -> Option<&str> {
    time.split_once('T')?.1.get(..5)
}

fn small(text: impl Into<String>, color: Color32) -> RichText {
    RichText::new(text).size(11.0).color(color)
}

fn metric(ui: &mut egui::Ui, label: &str, value: String) {
    egui::Frame::none()
        .fill(FIELD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .inner_margin(egui::Margin::symmetric(6.0, 4.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(small(label, MUTED));
                ui.label(RichText::new(value).size(12.0).strong().color(IVORY));
            });
        });
}

const HOUR_TILE_CONTENT_WIDTH: f32 = 40.0;
const HOUR_TILE_WIDTH: f32 = HOUR_TILE_CONTENT_WIDTH + 12.0;

fn hour_tile(ui: &mut egui::Ui, hour: &WeatherHour<'_>) {
    let time = local_hour(hour.time);
    let tile = egui::Frame::none()
        .fill(BUTTON)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .inner_margin(egui::Margin::symmetric(5.0, 4.0))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(HOUR_TILE_CONTENT_WIDTH, 66.0),
                egui::Layout::top_down(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(small(time.unwrap_or("—"), MUTED));
                    weather_icon(ui, hour.weather_code, hour.is_day == Some(0), 2.0);
                    ui.label(
                        RichText::new(
                            hour.temperature_c
                                .map_or_else(|| "—".into(), |temp| format!("{temp:.0}°")),
                        )
                        .strong()
                        .size(14.0)
                        .color(IVORY),
                    );
                    ui.label(small(
                        hour.precipitation_pct
                            .map_or_else(|| "—".into(), |pct| format!("{pct}%")),
                        CYAN,
                    ));
                },
            );
        });
    if let Some(code) = hour.weather_code {
        tile.response.on_hover_text(format!(
            "{} · {}",
            hour.time.replace('T', " "),
            describe_wmo_code(code).1,
        ));
    }
}

fn weather_icon(ui: &mut egui::Ui, code: Option<u64>, night: bool, pixel: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0 * pixel), Sense::hover());
    let rows = match code {
        Some(0 | 1) if night => [
            "...GG...", "..GG....", ".GG.....", ".GG.....", ".GGG....", "..GGGGG.", "...GGG..",
            "........",
        ],
        Some(0 | 1) => [
            "...G....", ".G...G..", "..GGG...", "G.GGG.G.", "..GGG...", ".G...G..", "...G....",
            "........",
        ],
        Some(2) => [
            ".GG.....", "GGGG....", ".GG.CCC.", "...CCCCC", "..CCCCCC", "........", "........",
            "........",
        ],
        Some(3) => [
            "........", "...CCC..", "..CCCCC.", ".CCCCCCC", "CCCCCCCC", "........", "........",
            "........",
        ],
        Some(45 | 48) => [
            "........", ".CCCCCC.", "........", "CCCCCC..", "........", "..CCCCCC", "........",
            "........",
        ],
        Some(95..=97 | 99) => [
            "...CCC..", "..CCCCC.", ".CCCCCCC", "CCCCCCCC", "...GG...", "..GG....", "...G....",
            "..G.....",
        ],
        Some(56 | 57 | 66 | 67 | 71 | 73 | 75 | 77 | 85 | 86) => [
            "...CCC..", "..CCCCC.", ".CCCCCCC", "CCCCCCCC", "........", ".W...W..", "...W...W",
            ".....W..",
        ],
        Some(51 | 53 | 55 | 61 | 63 | 65 | 80..=82) => [
            "...CCC..", "..CCCCC.", ".CCCCCCC", "CCCCCCCC", "..B.B.B.", ".B.B.B..", "..B.B.B.",
            "........",
        ],
        _ => [
            "........", "........", "........", "..CCCC..", "........", "........", "........",
            "........",
        ],
    };
    let origin = rect.min.round();
    for (y, row) in rows.iter().enumerate() {
        for (x, shade) in row.bytes().enumerate() {
            let color = match shade {
                b'G' => retro::GOLD_LIGHT,
                b'C' => MUTED,
                b'B' => CYAN,
                b'W' => IVORY,
                _ => continue,
            };
            let at = origin + Vec2::new(x as f32 * pixel, y as f32 * pixel);
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::new(at.x, at.y), Vec2::splat(pixel)),
                0.0,
                color,
            );
        }
    }
}
