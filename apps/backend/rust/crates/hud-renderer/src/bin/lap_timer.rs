use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Rect, Rounding, Sense, Stroke, Vec2, pos2, vec2,
};
use hud_renderer::{
    LapTimerCompletedLap, LapTimerController, LapTimerCurrentLap, LapTimerDeltaState,
    LapTimerDisplay, LapTimerEntry, LapTimerSnapshot, fetch_lap_timer,
};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:4768";
const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(280.0, 180.0);
const TARGET_CANVAS_SIZE: Vec2 = DEFAULT_WINDOW_SIZE;
const MIN_WINDOW_SIZE: Vec2 = Vec2::new(220.0, 140.0);
const DEFAULT_RENDER_INTERVAL_MS: u64 = 16;
const DEFAULT_FETCH_INTERVAL_MS: u64 = 250;

const PANEL_BG: Color32 = Color32::from_rgb(26, 26, 26);
const PANEL_BORDER: Color32 = Color32::from_rgb(51, 51, 51);
const LABEL_COLOR: Color32 = Color32::from_rgb(136, 136, 136);
const CURRENT_COLOR: Color32 = Color32::from_rgb(0, 255, 255);
const GOOD_COLOR: Color32 = Color32::from_rgb(0, 255, 0);
const BAD_COLOR: Color32 = Color32::from_rgb(255, 85, 85);
const WHITE_COLOR: Color32 = Color32::from_rgb(255, 255, 255);

fn main() -> Result<(), eframe::Error> {
    let config = AppConfig::from_env();
    let title = if config.sample_mode {
        format!("{} (sample)", config.title)
    } else {
        config.title.clone()
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(config.initial_window_size)
        .with_min_inner_size(MIN_WINDOW_SIZE)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_icon(egui::IconData::default())
        .with_title(title.clone());

    if let Some(position) = config.initial_position {
        viewport = viewport.with_position(position);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        &title,
        options,
        Box::new(move |_creation_context| Box::new(LapTimerApp::new(config))),
    )
}

#[derive(Clone, Debug)]
struct AppConfig {
    sample_mode: bool,
    base_url: String,
    title: String,
    render_interval: Duration,
    fetch_interval: Duration,
    initial_position: Option<egui::Pos2>,
    initial_window_size: Vec2,
}

impl AppConfig {
    fn from_env() -> Self {
        let mut sample_mode = false;
        let mut positional_base_url = None;
        let mut title = None;
        let mut render_interval_ms = None;
        let mut fetch_interval_ms = None;
        let mut initial_x = None;
        let mut initial_y = None;
        let mut initial_width = None;
        let mut initial_height = None;
        let base_url = std::env::var("PNG_BACKEND_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BACKEND_URL.to_string());

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--sample" => sample_mode = true,
                "--title" => {
                    if let Some(value) = args.next() {
                        title = Some(value);
                    }
                }
                "--render-interval-ms" | "--interval-ms" => {
                    if let Some(value) = args.next() {
                        render_interval_ms = value.parse::<u64>().ok().filter(|value| *value > 0);
                    }
                }
                "--fetch-interval-ms" => {
                    if let Some(value) = args.next() {
                        fetch_interval_ms = value.parse::<u64>().ok().filter(|value| *value > 0);
                    }
                }
                "--x" => {
                    if let Some(value) = args.next() {
                        initial_x = value.parse::<f32>().ok();
                    }
                }
                "--y" => {
                    if let Some(value) = args.next() {
                        initial_y = value.parse::<f32>().ok();
                    }
                }
                "--width" => {
                    if let Some(value) = args.next() {
                        initial_width = value.parse::<f32>().ok().filter(|value| *value > 0.0);
                    }
                }
                "--height" => {
                    if let Some(value) = args.next() {
                        initial_height = value.parse::<f32>().ok().filter(|value| *value > 0.0);
                    }
                }
                _ if arg.starts_with("--") => {}
                _ => {
                    if positional_base_url.is_none() {
                        positional_base_url = Some(arg);
                    }
                }
            }
        }

        Self {
            sample_mode,
            base_url: positional_base_url.unwrap_or(base_url),
            title: title.unwrap_or_else(|| "pits n giggles - Rust Lap Timer prototype".to_string()),
            render_interval: Duration::from_millis(
                render_interval_ms.unwrap_or(DEFAULT_RENDER_INTERVAL_MS),
            ),
            fetch_interval: Duration::from_millis(
                fetch_interval_ms.unwrap_or(DEFAULT_FETCH_INTERVAL_MS),
            ),
            initial_position: initial_x.zip(initial_y).map(|(x, y)| pos2(x, y)),
            initial_window_size: vec2(
                initial_width.unwrap_or(DEFAULT_WINDOW_SIZE.x),
                initial_height.unwrap_or(DEFAULT_WINDOW_SIZE.y),
            ),
        }
    }
}

struct LapTimerApp {
    config: AppConfig,
    controller: LapTimerController,
    display: LapTimerDisplay,
    telemetry_worker: Option<TelemetryWorker>,
    started_at: Instant,
    last_error: Option<String>,
}

impl LapTimerApp {
    fn new(config: AppConfig) -> Self {
        Self {
            config: config.clone(),
            controller: LapTimerController::new(),
            display: LapTimerDisplay::empty(),
            telemetry_worker: (!config.sample_mode)
                .then(|| TelemetryWorker::spawn(config.base_url.clone(), config.fetch_interval)),
            started_at: Instant::now(),
            last_error: None,
        }
    }

    fn refresh_if_needed(&mut self) {
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        if self.config.sample_mode {
            let snapshot = sample_snapshot(self.started_at.elapsed().as_secs_f32());
            match self.controller.update(&snapshot, now_ms) {
                Ok(display) => {
                    self.display = display;
                    self.last_error = None;
                }
                Err(error) => self.last_error = Some(error.to_string()),
            }
            return;
        }

        let Some(update) = self
            .telemetry_worker
            .as_ref()
            .and_then(TelemetryWorker::drain_latest)
        else {
            return;
        };

        match update {
            TelemetryUpdate::Snapshot(snapshot) => {
                match self.controller.update(&snapshot, now_ms) {
                    Ok(display) => {
                        self.display = display;
                        self.last_error = None;
                    }
                    Err(error) => self.last_error = Some(error.to_string()),
                }
            }
            TelemetryUpdate::Error(error) => {
                self.last_error = Some(error);
            }
        }
    }
}

enum TelemetryUpdate {
    Snapshot(LapTimerSnapshot),
    Error(String),
}

struct TelemetryWorker {
    receiver: Receiver<TelemetryUpdate>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TelemetryWorker {
    fn spawn(base_url: String, fetch_interval: Duration) -> Self {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("lap-timer-fetch".to_string())
            .spawn(move || telemetry_worker_loop(base_url, fetch_interval, sender, thread_stop))
            .expect("spawn telemetry worker");

        Self {
            receiver,
            stop,
            thread: Some(thread),
        }
    }

    fn drain_latest(&self) -> Option<TelemetryUpdate> {
        let mut latest = None;
        loop {
            match self.receiver.try_recv() {
                Ok(update) => latest = Some(update),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return latest,
            }
        }
    }
}

impl Drop for TelemetryWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn telemetry_worker_loop(
    base_url: String,
    fetch_interval: Duration,
    sender: Sender<TelemetryUpdate>,
    stop: Arc<AtomicBool>,
) {
    let mut next_fetch_at = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        let update = match fetch_lap_timer(&base_url) {
            Ok(snapshot) => TelemetryUpdate::Snapshot(snapshot),
            Err(error) => TelemetryUpdate::Error(error.to_string()),
        };

        if sender.send(update).is_err() {
            break;
        }

        next_fetch_at += fetch_interval;
        if next_fetch_at < Instant::now() {
            next_fetch_at = Instant::now();
        }
        sleep_until_stop(&stop, next_fetch_at);
    }
}

fn sleep_until_stop(stop: &AtomicBool, deadline: Instant) {
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= deadline {
            return;
        }

        thread::sleep((deadline - now).min(Duration::from_millis(10)));
    }
}

impl eframe::App for LapTimerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_if_needed();
        ctx.request_repaint_after(self.config.render_interval);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_black_alpha(0)))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                paint_overlay(ui.painter(), rect, &self.display);

                if let Some(error) = &self.last_error {
                    ui.painter().text(
                        rect.left_top() + vec2(8.0, 8.0),
                        Align2::LEFT_TOP,
                        error,
                        FontId::proportional(11.0),
                        BAD_COLOR,
                    );
                }
            });
    }
}

fn sample_snapshot(elapsed: f32) -> LapTimerSnapshot {
    let lap_duration_sec = 6.0;
    let lap_index = (elapsed / lap_duration_sec).floor() as u8;
    let lap_progress = elapsed % lap_duration_sec;
    let current_lap_number = 8 + lap_index;
    let current_lap_time_ms = (lap_progress * 1_000.0).round() as u32;
    let delta_ms = ((elapsed * 0.9).sin() * 380.0).round() as i32;
    let last_lap_ms = 91_450u32.saturating_sub(u32::from(lap_index.min(5)) * 120);
    let best_lap_ms = last_lap_ms.min(90_960);
    let current_sector_status = if lap_progress < 2.0 {
        [1, -2, -2]
    } else if lap_progress < 4.0 {
        [1, 0, -2]
    } else {
        [1, 0, 1]
    };

    LapTimerSnapshot {
        session_type: "Qualifying".to_string(),
        session_uid: 9001,
        safety_car_status: "NO_SAFETY_CAR".to_string(),
        ref_driver_index: 7,
        table_entries: vec![
            LapTimerEntry {
                index: 7,
                position: Some(3),
                is_player: true,
                current_lap_number: Some(current_lap_number),
                last_lap: LapTimerCompletedLap {
                    lap_time_ms: Some(last_lap_ms),
                    sector_status: [2, 1, 1],
                },
                best_lap: LapTimerCompletedLap {
                    lap_time_ms: Some(best_lap_ms),
                    sector_status: [2, 1, 1],
                },
                current_lap: LapTimerCurrentLap {
                    lap_time_ms: Some(current_lap_time_ms),
                    driver_status: "FLYING_LAP".to_string(),
                    sector_status: current_sector_status,
                    delta_ms: Some(delta_ms),
                    delta_sc_sec: None,
                },
            },
            LapTimerEntry {
                index: 3,
                position: Some(1),
                is_player: false,
                current_lap_number: Some(current_lap_number),
                last_lap: LapTimerCompletedLap {
                    lap_time_ms: Some(90_720),
                    sector_status: [1, 2, 1],
                },
                best_lap: LapTimerCompletedLap {
                    lap_time_ms: Some(90_720),
                    sector_status: [1, 2, 1],
                },
                current_lap: LapTimerCurrentLap {
                    lap_time_ms: Some(current_lap_time_ms.saturating_add(600)),
                    driver_status: "FLYING_LAP".to_string(),
                    sector_status: [1, -2, -2],
                    delta_ms: Some(120),
                    delta_sc_sec: None,
                },
            },
            LapTimerEntry {
                index: 9,
                position: Some(2),
                is_player: false,
                current_lap_number: Some(current_lap_number),
                last_lap: LapTimerCompletedLap {
                    lap_time_ms: Some(91_040),
                    sector_status: [1, 1, 1],
                },
                best_lap: LapTimerCompletedLap {
                    lap_time_ms: Some(91_040),
                    sector_status: [1, 1, 1],
                },
                current_lap: LapTimerCurrentLap {
                    lap_time_ms: Some(current_lap_time_ms.saturating_add(200)),
                    driver_status: "FLYING_LAP".to_string(),
                    sector_status: [1, -2, -2],
                    delta_ms: Some(60),
                    delta_sc_sec: None,
                },
            },
        ],
    }
}

fn paint_overlay(painter: &Painter, rect: Rect, display: &LapTimerDisplay) {
    let scale = (rect.width() / TARGET_CANVAS_SIZE.x)
        .min(rect.height() / TARGET_CANVAS_SIZE.y)
        .clamp(0.7, 4.0);
    let inner = rect.shrink(2.0 * scale);
    let gap = 2.0 * scale;
    let estimated_height = 36.0 * scale;
    let current_sector_height = 12.0 * scale;
    let grid_height = inner.height() - estimated_height - current_sector_height - gap * 2.0;
    let card_width = (inner.width() - gap) * 0.5;
    let top_card_height = (grid_height - gap) * 0.45;
    let bottom_card_height = grid_height - gap - top_card_height;

    let current_rect = Rect::from_min_size(inner.min, vec2(card_width, top_card_height));
    let delta_rect = Rect::from_min_size(
        pos2(current_rect.right() + gap, inner.top()),
        vec2(card_width, top_card_height),
    );
    let last_rect = Rect::from_min_size(
        pos2(inner.left(), current_rect.bottom() + gap),
        vec2(card_width, bottom_card_height),
    );
    let best_rect = Rect::from_min_size(
        pos2(delta_rect.left(), delta_rect.bottom() + gap),
        vec2(card_width, bottom_card_height),
    );
    let estimated_rect = Rect::from_min_size(
        pos2(inner.left(), last_rect.bottom() + gap),
        vec2(inner.width(), estimated_height),
    );
    let current_sector_rect = Rect::from_min_size(
        pos2(inner.left(), estimated_rect.bottom() + gap),
        vec2(inner.width(), current_sector_height),
    );

    paint_card(
        painter,
        current_rect,
        "CURRENT",
        display.current_time.as_str(),
        CURRENT_COLOR,
        scale,
    );
    paint_card(
        painter,
        delta_rect,
        "DELTA",
        display.delta_time.as_str(),
        match display.delta_state {
            LapTimerDeltaState::Neutral => WHITE_COLOR,
            LapTimerDeltaState::Good => GOOD_COLOR,
            LapTimerDeltaState::Bad => BAD_COLOR,
        },
        scale,
    );
    paint_card_with_sector_bar(
        painter,
        last_rect,
        "LAST",
        display.last_time.as_str(),
        WHITE_COLOR,
        display.last_sector_status,
        scale,
    );
    paint_card_with_sector_bar(
        painter,
        best_rect,
        "BEST",
        display.best_time.as_str(),
        GOOD_COLOR,
        display.best_sector_status,
        scale,
    );
    paint_estimated_bar(
        painter,
        estimated_rect,
        display.estimated_time.as_str(),
        scale,
    );
    paint_sector_bar(
        painter,
        current_sector_rect,
        display.current_sector_status,
        scale,
    );
}

fn paint_card(
    painter: &Painter,
    rect: Rect,
    label: &str,
    value: &str,
    value_color: Color32,
    scale: f32,
) {
    let rounding = Rounding::ZERO;
    painter.rect_filled(rect, rounding, PANEL_BG);
    painter.rect_stroke(rect, rounding, Stroke::new(1.0 * scale, PANEL_BORDER));

    let label_font = FontId::proportional(12.0 * scale);
    let value_font = FontId::proportional(14.0 * scale);
    painter.text(
        pos2(rect.center().x, rect.top() + 12.0 * scale),
        Align2::CENTER_CENTER,
        label,
        label_font,
        LABEL_COLOR,
    );
    painter.text(
        pos2(rect.center().x, rect.center().y + 6.0 * scale),
        Align2::CENTER_CENTER,
        value,
        value_font,
        value_color,
    );
}

fn paint_card_with_sector_bar(
    painter: &Painter,
    rect: Rect,
    label: &str,
    value: &str,
    value_color: Color32,
    sector_status: [i32; 3],
    scale: f32,
) {
    paint_card(painter, rect, label, value, value_color, scale);
    let sector_rect = Rect::from_min_size(
        pos2(rect.left() + 2.0 * scale, rect.bottom() - 10.0 * scale),
        vec2(rect.width() - 4.0 * scale, 8.0 * scale),
    );
    paint_sector_bar(painter, sector_rect, sector_status, scale);
}

fn paint_estimated_bar(painter: &Painter, rect: Rect, value: &str, scale: f32) {
    painter.rect_filled(rect, Rounding::ZERO, PANEL_BG);
    painter.rect_stroke(rect, Rounding::ZERO, Stroke::new(1.0 * scale, PANEL_BORDER));

    painter.text(
        pos2(rect.center().x, rect.top() + 10.0 * scale),
        Align2::CENTER_CENTER,
        "ESTIMATED",
        FontId::proportional(12.0 * scale),
        LABEL_COLOR,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 11.0 * scale),
        Align2::CENTER_CENTER,
        value,
        FontId::proportional(14.0 * scale),
        WHITE_COLOR,
    );
}

fn paint_sector_bar(painter: &Painter, rect: Rect, sector_status: [i32; 3], scale: f32) {
    let width = rect.width() / 3.0;
    for (index, status) in sector_status.into_iter().enumerate() {
        let segment = Rect::from_min_size(
            pos2(rect.left() + width * index as f32, rect.top()),
            vec2(width, rect.height()),
        );
        painter.rect_filled(segment, Rounding::ZERO, sector_color(status));
        if index > 0 {
            painter.line_segment(
                [
                    pos2(segment.left(), segment.top()),
                    pos2(segment.left(), segment.bottom()),
                ],
                Stroke::new(1.0 * scale, Color32::BLACK),
            );
        }
    }
}

fn sector_color(status: i32) -> Color32 {
    match status {
        -2 => Color32::from_rgb(108, 117, 125),
        -1 => Color32::from_rgb(220, 53, 69),
        0 => Color32::from_rgb(255, 193, 7),
        1 => Color32::from_rgb(40, 167, 69),
        2 => Color32::from_rgb(128, 0, 128),
        _ => Color32::from_rgb(50, 50, 50),
    }
}
