use std::collections::VecDeque;
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
use hud_renderer::{InputTelemetrySnapshot, fetch_input_telemetry};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:4768";
const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(480.0, 160.0);
const TARGET_CANVAS_SIZE: Vec2 = DEFAULT_WINDOW_SIZE;
const MIN_WINDOW_SIZE: Vec2 = Vec2::new(160.0, 56.0);
const MAX_HISTORY_LENGTH: usize = 175;
const SMOOTHING_FACTOR: f32 = 0.2;
const DEFAULT_RENDER_INTERVAL_MS: u64 = 16;
const DEFAULT_FETCH_INTERVAL_MS: u64 = 100;

const PANEL_COLOR: Color32 = Color32::from_rgb(0, 0, 0);
const PANEL_EDGE_COLOR: Color32 = Color32::from_rgb(28, 28, 28);
const GRID_COLOR: Color32 = Color32::from_rgb(26, 26, 26);
const CENTER_LINE_COLOR: Color32 = Color32::from_rgb(42, 42, 42);
const BAR_BACKGROUND: Color32 = Color32::from_rgb(32, 32, 32);
const THROTTLE_COLOR: Color32 = Color32::from_rgb(118, 255, 3);
const BRAKE_COLOR: Color32 = Color32::from_rgb(255, 23, 68);
const STEERING_COLOR: Color32 = Color32::from_rgb(41, 121, 255);

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
        Box::new(move |_creation_context| Box::new(HudApp::new(config))),
    )
}

#[derive(Clone, Debug)]
struct AppConfig {
    sample_mode: bool,
    base_url: String,
    title: String,
    history_length: usize,
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
        let mut history_length = None;
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
                "--history-length" => {
                    if let Some(value) = args.next() {
                        history_length = value.parse::<usize>().ok().filter(|value| *value > 0);
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
            title: title.unwrap_or_else(|| "pits n giggles - Rust HUD prototype".to_string()),
            history_length: history_length.unwrap_or(MAX_HISTORY_LENGTH),
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

struct HudApp {
    config: AppConfig,
    snapshot: InputTelemetrySnapshot,
    display: SmoothedTelemetry,
    history: TelemetryHistory,
    telemetry_worker: Option<TelemetryWorker>,
    started_at: Instant,
    last_updated_at: Option<Instant>,
    last_error: Option<String>,
}

impl HudApp {
    fn new(config: AppConfig) -> Self {
        let snapshot = InputTelemetrySnapshot::sample();
        let display = SmoothedTelemetry::from_snapshot(&snapshot);
        let mut history = TelemetryHistory::new(config.history_length);
        history.push(&display);
        let telemetry_worker = (!config.sample_mode)
            .then(|| TelemetryWorker::spawn(config.base_url.clone(), config.fetch_interval));

        Self {
            config,
            snapshot,
            display,
            history,
            telemetry_worker,
            started_at: Instant::now(),
            last_updated_at: None,
            last_error: None,
        }
    }

    fn refresh_if_needed(&mut self) {
        if self.config.sample_mode {
            self.snapshot = self.sample_snapshot();
            self.display.update_from_snapshot(&self.snapshot);
            self.history.push(&self.display);
            self.last_updated_at = Some(Instant::now());
            self.last_error = None;
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
            TelemetryUpdate::Snapshot(snapshot, received_at) => {
                self.snapshot = snapshot;
                self.display.update_from_snapshot(&self.snapshot);
                self.history.push(&self.display);
                self.last_updated_at = Some(received_at);
                self.last_error = None;
            }
            TelemetryUpdate::Error(error) => {
                self.last_error = Some(error);
            }
        }
    }

    fn sample_snapshot(&self) -> InputTelemetrySnapshot {
        let elapsed = self.started_at.elapsed().as_secs_f32();
        let throttle = 42.0 + 54.0 * normalized_wave(elapsed * 1.35);
        let brake = 26.0 * positive_wave(elapsed * 2.1 + 0.9).powi(3);
        let steering = 72.0 * (elapsed * 0.95).sin();
        let rev_lights_percentage =
            (30.0 + 70.0 * normalized_wave(elapsed * 1.8 + 0.45)).round() as u8;

        InputTelemetrySnapshot {
            throttle,
            brake,
            steering,
            rev_lights_percentage,
        }
    }
}

enum TelemetryUpdate {
    Snapshot(InputTelemetrySnapshot, Instant),
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
            .name("hud-renderer-fetch".to_string())
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
    while !stop.load(Ordering::Relaxed) {
        let update = match fetch_input_telemetry(&base_url) {
            Ok(snapshot) => TelemetryUpdate::Snapshot(snapshot, Instant::now()),
            Err(error) => TelemetryUpdate::Error(error.to_string()),
        };

        if sender.send(update).is_err() {
            break;
        }

        sleep_until_stop(&stop, fetch_interval);
    }
}

fn sleep_until_stop(stop: &AtomicBool, duration: Duration) {
    let started_at = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let elapsed = started_at.elapsed();
        if elapsed >= duration {
            return;
        }

        thread::sleep((duration - elapsed).min(Duration::from_millis(10)));
    }
}

impl eframe::App for HudApp {
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
                paint_overlay(ui.painter(), rect, &self.history, &self.display);

                if let Some(error) = &self.last_error {
                    ui.painter().text(
                        rect.left_top() + vec2(10.0, 10.0),
                        Align2::LEFT_TOP,
                        error,
                        FontId::proportional(11.0),
                        Color32::from_rgb(255, 120, 120),
                    );
                }
            });
    }
}

#[derive(Clone, Debug)]
struct SmoothedTelemetry {
    throttle: f32,
    brake: f32,
    steering: f32,
    rev_lights: f32,
}

impl SmoothedTelemetry {
    fn from_snapshot(snapshot: &InputTelemetrySnapshot) -> Self {
        Self {
            throttle: snapshot.throttle,
            brake: snapshot.brake,
            steering: snapshot.steering,
            rev_lights: snapshot.rev_lights_percentage as f32,
        }
    }

    fn update_from_snapshot(&mut self, snapshot: &InputTelemetrySnapshot) {
        self.throttle = smooth_value(self.throttle, snapshot.throttle);
        self.brake = smooth_value(self.brake, snapshot.brake);
        self.steering = smooth_value(self.steering, snapshot.steering);
        self.rev_lights = smooth_value(self.rev_lights, snapshot.rev_lights_percentage as f32);
    }
}

#[derive(Debug)]
struct TelemetryHistory {
    max_len: usize,
    throttle: VecDeque<f32>,
    brake: VecDeque<f32>,
    steering: VecDeque<f32>,
}

impl TelemetryHistory {
    fn new(max_len: usize) -> Self {
        Self {
            max_len,
            throttle: VecDeque::with_capacity(max_len),
            brake: VecDeque::with_capacity(max_len),
            steering: VecDeque::with_capacity(max_len),
        }
    }

    fn push(&mut self, telemetry: &SmoothedTelemetry) {
        push_capped(&mut self.throttle, telemetry.throttle, self.max_len);
        push_capped(&mut self.brake, telemetry.brake, self.max_len);
        push_capped(&mut self.steering, telemetry.steering, self.max_len);
    }
}

fn push_capped(history: &mut VecDeque<f32>, value: f32, max_len: usize) {
    history.push_back(value);
    while history.len() > max_len {
        history.pop_front();
    }
}

fn smooth_value(previous: f32, next: f32) -> f32 {
    previous * SMOOTHING_FACTOR + next * (1.0 - SMOOTHING_FACTOR)
}

fn normalized_wave(value: f32) -> f32 {
    (value.sin() + 1.0) * 0.5
}

fn positive_wave(value: f32) -> f32 {
    value.sin().max(0.0)
}

fn paint_overlay(
    painter: &Painter,
    rect: Rect,
    history: &TelemetryHistory,
    display: &SmoothedTelemetry,
) {
    let scale = (rect.width() / TARGET_CANVAS_SIZE.x)
        .min(rect.height() / TARGET_CANVAS_SIZE.y)
        .clamp(0.35, 4.0);
    let outer_margin = 0.0;
    let inner_margin = 6.0 * scale;
    let panel_rounding = Rounding::same(6.0 * scale);
    let bars_width = 60.0 * scale;
    let gap = 10.0 * scale;

    let panel = rect.shrink(outer_margin);
    painter.rect_filled(panel, panel_rounding, PANEL_COLOR);
    painter.rect_stroke(
        panel,
        panel_rounding,
        Stroke::new(1.0 * scale, PANEL_EDGE_COLOR),
    );

    let inner = panel.shrink(inner_margin);
    let graph_width = (inner.width() - bars_width - gap).max(80.0 * scale);
    let graph = Rect::from_min_size(inner.min, vec2(graph_width, inner.height()));
    let bars = Rect::from_min_size(
        pos2(graph.right() + gap, inner.top()),
        vec2(
            (inner.right() - graph.right() - gap).max(0.0),
            inner.height(),
        ),
    );

    paint_rev_strip(painter, graph, clamp_percentage(display.rev_lights), scale);
    paint_graph_grid(painter, graph, scale);
    paint_history_fill(
        painter,
        graph,
        &history.throttle,
        history.max_len,
        THROTTLE_COLOR.gamma_multiply(0.14),
        scale,
        sample_to_bottom,
    );
    paint_history_fill(
        painter,
        graph,
        &history.brake,
        history.max_len,
        BRAKE_COLOR.gamma_multiply(0.14),
        scale,
        sample_to_bottom,
    );
    paint_history_line(
        painter,
        graph,
        &history.throttle,
        history.max_len,
        THROTTLE_COLOR,
        2.0 * scale,
        sample_to_bottom,
    );
    paint_history_line(
        painter,
        graph,
        &history.brake,
        history.max_len,
        BRAKE_COLOR,
        2.0 * scale,
        sample_to_bottom,
    );
    paint_history_line(
        painter,
        graph,
        &history.steering,
        history.max_len,
        STEERING_COLOR,
        2.0 * scale,
        sample_to_center,
    );

    let bar_gap = 6.0 * scale;
    let bar_width = 22.0 * scale;
    let total_bars_width = bar_width * 2.0 + bar_gap;
    let bars_left = bars.center().x - total_bars_width * 0.5;

    let brake_bar =
        Rect::from_min_size(pos2(bars_left, bars.top()), vec2(bar_width, bars.height()));
    let throttle_bar = Rect::from_min_size(
        pos2(bars_left + bar_width + bar_gap, bars.top()),
        vec2(bar_width, bars.height()),
    );

    paint_input_bar(
        painter,
        brake_bar,
        clamp_percentage(display.brake),
        BRAKE_COLOR,
        scale,
    );
    paint_input_bar(
        painter,
        throttle_bar,
        clamp_percentage(display.throttle),
        THROTTLE_COLOR,
        scale,
    );
}

fn paint_rev_strip(painter: &Painter, graph: Rect, rev_fill: f32, scale: f32) {
    let strip = Rect::from_min_size(graph.min, vec2(graph.width(), 3.0 * scale));
    let segments = 48usize;
    let segment_width = strip.width() / segments as f32;
    let alpha = rev_fill.clamp(0.0, 1.0);

    for index in 0..segments {
        let t0 = index as f32 / segments as f32;
        let t1 = (index + 1) as f32 / segments as f32;
        let color = strip_gradient_color((t0 + t1) * 0.5, alpha);
        let segment = Rect::from_min_max(
            pos2(strip.left() + segment_width * index as f32, strip.top()),
            pos2(
                strip.left() + segment_width * (index + 1) as f32,
                strip.bottom(),
            ),
        );
        painter.rect_filled(segment, Rounding::ZERO, color);
    }
}

fn strip_gradient_color(position: f32, alpha: f32) -> Color32 {
    let transparent = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
    let brake = BRAKE_COLOR.gamma_multiply(alpha);
    let throttle = THROTTLE_COLOR.gamma_multiply(alpha);

    if position <= 0.3 {
        lerp_color(transparent, brake, position / 0.3)
    } else if position <= 0.7 {
        lerp_color(brake, throttle, (position - 0.3) / 0.4)
    } else {
        lerp_color(throttle, transparent, (position - 0.7) / 0.3)
    }
}

fn paint_graph_grid(painter: &Painter, graph: Rect, scale: f32) {
    for step in 1..4 {
        let y = graph.top() + graph.height() * step as f32 / 4.0;
        painter.line_segment(
            [pos2(graph.left(), y), pos2(graph.right(), y)],
            Stroke::new(1.0 * scale, GRID_COLOR),
        );
    }

    let center_y = graph.center().y;
    painter.line_segment(
        [pos2(graph.left(), center_y), pos2(graph.right(), center_y)],
        Stroke::new(1.0 * scale, CENTER_LINE_COLOR),
    );
}

fn paint_history_fill(
    painter: &Painter,
    graph: Rect,
    history: &VecDeque<f32>,
    max_len: usize,
    color: Color32,
    scale: f32,
    mapper: fn(Rect, f32) -> f32,
) {
    if history.len() < 2 {
        return;
    }

    let spacing = graph.width() / (max_len.saturating_sub(1)) as f32;
    let thickness = spacing.max(1.0 * scale);

    for (index, sample) in history.iter().enumerate() {
        let x = graph.left() + spacing * index as f32;
        let y = mapper(graph, *sample);
        painter.line_segment(
            [pos2(x, y), pos2(x, graph.bottom())],
            Stroke::new(thickness, color),
        );
    }
}

fn paint_history_line(
    painter: &Painter,
    graph: Rect,
    history: &VecDeque<f32>,
    max_len: usize,
    color: Color32,
    width: f32,
    mapper: fn(Rect, f32) -> f32,
) {
    if history.len() < 2 {
        return;
    }

    let spacing = graph.width() / (max_len.saturating_sub(1)) as f32;
    let points: Vec<_> = history
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            pos2(
                graph.left() + spacing * index as f32,
                mapper(graph, *sample),
            )
        })
        .collect();

    painter.add(egui::Shape::line(points, Stroke::new(width, color)));
}

fn sample_to_bottom(graph: Rect, sample: f32) -> f32 {
    graph.bottom() - clamp_percentage(sample) * graph.height()
}

fn sample_to_center(graph: Rect, sample: f32) -> f32 {
    graph.center().y - clamp_steering(sample) * (graph.height() * 0.5)
}

fn clamp_percentage(value: f32) -> f32 {
    (value / 100.0).clamp(0.0, 1.0)
}

fn clamp_steering(value: f32) -> f32 {
    (value / 100.0).clamp(-1.0, 1.0)
}

fn paint_input_bar(painter: &Painter, rect: Rect, value_fill: f32, color: Color32, scale: f32) {
    let rounding = Rounding::same(3.0 * scale);
    painter.rect_filled(rect, rounding, BAR_BACKGROUND);
    painter.rect_stroke(rect, rounding, Stroke::new(1.0 * scale, color));

    let fill_height = rect.height() * value_fill.clamp(0.0, 1.0);
    if fill_height > 0.0 {
        let fill_rect = Rect::from_min_max(
            pos2(rect.left(), rect.bottom() - fill_height),
            pos2(rect.right(), rect.bottom()),
        );
        painter.rect_filled(fill_rect, rounding, color);
    }

    let value = (value_fill * 100.0).round();
    let value_color = if value > 10.0 { PANEL_COLOR } else { color };
    painter.text(
        pos2(rect.center().x, rect.bottom() - 2.0 * scale),
        Align2::CENTER_BOTTOM,
        format!("{value:.0}"),
        FontId::proportional(10.0 * scale),
        value_color,
    );
}

fn lerp_color(start: Color32, end: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let [sr, sg, sb, sa] = start.to_array();
    let [er, eg, eb, ea] = end.to_array();

    Color32::from_rgba_unmultiplied(
        lerp_channel(sr, er, t),
        lerp_channel(sg, eg, t),
        lerp_channel(sb, eb, t),
        lerp_channel(sa, ea, t),
    )
}

fn lerp_channel(start: u8, end: u8, t: f32) -> u8 {
    (start as f32 + (end as f32 - start as f32) * t).round() as u8
}
