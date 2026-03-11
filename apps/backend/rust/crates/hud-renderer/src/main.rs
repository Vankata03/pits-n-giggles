use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Layout, ProgressBar, RichText, Vec2};
use hud_renderer::{HudDisplayMetrics, InputTelemetrySnapshot, fetch_input_telemetry};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:4768";
const FETCH_INTERVAL: Duration = Duration::from_millis(100);

fn main() -> Result<(), eframe::Error> {
    let config = AppConfig::from_env();
    let title = if config.sample_mode {
        "pits n giggles - Rust HUD prototype (sample)"
    } else {
        "pits n giggles - Rust HUD prototype"
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(360.0, 240.0))
            .with_min_inner_size(Vec2::new(300.0, 220.0))
            .with_title(title),
        ..Default::default()
    };

    eframe::run_native(
        title,
        options,
        Box::new(move |_creation_context| Box::new(HudApp::new(config))),
    )
}

#[derive(Clone, Debug)]
struct AppConfig {
    sample_mode: bool,
    base_url: String,
}

impl AppConfig {
    fn from_env() -> Self {
        let mut sample_mode = false;
        let mut positional_base_url = None;
        let base_url = std::env::var("PNG_BACKEND_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BACKEND_URL.to_string());

        for arg in std::env::args().skip(1) {
            if arg == "--sample" {
                sample_mode = true;
            } else {
                positional_base_url = Some(arg);
            }
        }

        Self {
            sample_mode,
            base_url: positional_base_url.unwrap_or(base_url),
        }
    }
}

struct HudApp {
    config: AppConfig,
    snapshot: InputTelemetrySnapshot,
    last_fetch_started_at: Option<Instant>,
    last_updated_at: Option<Instant>,
    last_error: Option<String>,
}

impl HudApp {
    fn new(config: AppConfig) -> Self {
        Self {
            snapshot: InputTelemetrySnapshot::sample(),
            config,
            last_fetch_started_at: None,
            last_updated_at: None,
            last_error: None,
        }
    }

    fn refresh_if_needed(&mut self) {
        if self.config.sample_mode {
            self.last_error = None;
            self.last_updated_at.get_or_insert_with(Instant::now);
            return;
        }

        let now = Instant::now();
        let should_fetch = self
            .last_fetch_started_at
            .map(|started_at| now.duration_since(started_at) >= FETCH_INTERVAL)
            .unwrap_or(true);

        if !should_fetch {
            return;
        }

        self.last_fetch_started_at = Some(now);
        match fetch_input_telemetry(&self.config.base_url) {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.last_updated_at = Some(now);
                self.last_error = None;
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
            }
        }
    }

    fn source_label(&self) -> String {
        if self.config.sample_mode {
            "sample telemetry".to_string()
        } else {
            format!(
                "{}/stream-overlay-info",
                self.config.base_url.trim_end_matches('/')
            )
        }
    }
}

impl eframe::App for HudApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_if_needed();
        ctx.request_repaint_after(Duration::from_millis(16));

        let metrics = HudDisplayMetrics::from_snapshot(&self.snapshot);

        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(Color32::from_rgb(14, 18, 24)))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(230, 236, 245));
                ui.spacing_mut().item_spacing.y = 10.0;

                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.heading(RichText::new("Rust HUD prototype").strong());
                    ui.label(
                        RichText::new(format!("Source: {}", self.source_label()))
                            .size(12.0)
                            .color(Color32::from_rgb(160, 171, 189)),
                    );

                    if let Some(updated_at) = self.last_updated_at {
                        let age_ms = Instant::now().duration_since(updated_at).as_millis();
                        ui.label(
                            RichText::new(format!("Last update: {age_ms} ms ago"))
                                .size(12.0)
                                .color(Color32::from_rgb(124, 205, 124)),
                        );
                    }

                    if let Some(error) = &self.last_error {
                        ui.colored_label(
                            Color32::from_rgb(255, 120, 120),
                            format!("Fetch error: {error}"),
                        );
                    }

                    labeled_bar(
                        ui,
                        "Throttle",
                        metrics.throttle_fill,
                        format!("{:.1}%", self.snapshot.throttle),
                        Color32::from_rgb(78, 201, 176),
                    );
                    labeled_bar(
                        ui,
                        "Brake",
                        metrics.brake_fill,
                        format!("{:.1}%", self.snapshot.brake),
                        Color32::from_rgb(229, 115, 115),
                    );
                    labeled_bar(
                        ui,
                        "Steering",
                        metrics.steering_fill,
                        format!("{:.1}%", metrics.steering_percentage),
                        Color32::from_rgb(100, 181, 246),
                    );
                    labeled_bar(
                        ui,
                        "Rev lights",
                        metrics.rev_lights_fill,
                        format!("{}%", self.snapshot.rev_lights_percentage),
                        Color32::from_rgb(255, 202, 40),
                    );
                });
            });
    }
}

fn labeled_bar(ui: &mut egui::Ui, label: &str, fill: f32, value: String, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(14.0).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).monospace());
        });
    });
    ui.add(
        ProgressBar::new(fill)
            .desired_width(f32::INFINITY)
            .fill(color)
            .show_percentage(),
    );
}
