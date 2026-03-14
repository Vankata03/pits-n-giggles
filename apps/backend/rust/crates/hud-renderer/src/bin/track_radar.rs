use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Rounding, Sense, Shape, Stroke, Vec2, pos2,
    vec2,
};
use hud_renderer::{
    RadarDisplayState, RadarDriver, RadarDriverMotion, RadarVector3, TrackRadarSnapshot,
    fetch_track_radar,
};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:4768";
const DEFAULT_WINDOW_SIZE: Vec2 = Vec2::new(300.0, 300.0);
const TARGET_CANVAS_SIZE: Vec2 = DEFAULT_WINDOW_SIZE;
const MIN_WINDOW_SIZE: Vec2 = Vec2::new(180.0, 180.0);
const DEFAULT_RENDER_INTERVAL_MS: u64 = 16;
const DEFAULT_FETCH_INTERVAL_MS: u64 = 100;
const DEFAULT_RADAR_RANGE_METERS: f32 = 25.0;
const DEFAULT_IDLE_OPACITY: f32 = 0.3;

const GRID_RGBA: (u8, u8, u8, u8) = (255, 255, 255, 56);
const CROSSHAIR_RGBA: (u8, u8, u8, u8) = (255, 255, 255, 72);
const REF_CAR_COLOR: Color32 = Color32::from_rgb(0, 255, 0);
const TARGET_CAR_COLOR: Color32 = Color32::from_rgb(255, 255, 255);
const TARGET_CAR_BORDER: Color32 = Color32::from_rgb(136, 136, 136);
const ALERT_RGBA: (u8, u8, u8, u8) = (255, 0, 0, 88);
const TOOLTIP_BG_RGBA: (u8, u8, u8, u8) = (0, 0, 0, 220);

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
        Box::new(move |_creation_context| Box::new(TrackRadarApp::new(config))),
    )
}

#[derive(Clone, Debug)]
struct AppConfig {
    sample_mode: bool,
    base_url: String,
    title: String,
    render_interval: Duration,
    fetch_interval: Duration,
    radar_range_meters: f32,
    idle_opacity: f32,
    initial_position: Option<Pos2>,
    initial_window_size: Vec2,
}

impl AppConfig {
    fn from_env() -> Self {
        let mut sample_mode = false;
        let mut positional_base_url = None;
        let mut title = None;
        let mut render_interval_ms = None;
        let mut fetch_interval_ms = None;
        let mut radar_range_meters = None;
        let mut idle_opacity = None;
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
                "--range-meters" => {
                    if let Some(value) = args.next() {
                        radar_range_meters = value.parse::<f32>().ok().filter(|value| *value > 0.0);
                    }
                }
                "--idle-opacity" => {
                    if let Some(value) = args.next() {
                        idle_opacity = value.parse::<f32>().ok().map(|value| value.clamp(0.0, 1.0));
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
            title: title
                .unwrap_or_else(|| "pits n giggles - Rust Track Radar prototype".to_string()),
            render_interval: Duration::from_millis(
                render_interval_ms.unwrap_or(DEFAULT_RENDER_INTERVAL_MS),
            ),
            fetch_interval: Duration::from_millis(
                fetch_interval_ms.unwrap_or(DEFAULT_FETCH_INTERVAL_MS),
            ),
            radar_range_meters: radar_range_meters.unwrap_or(DEFAULT_RADAR_RANGE_METERS),
            idle_opacity: idle_opacity.unwrap_or(DEFAULT_IDLE_OPACITY),
            initial_position: initial_x.zip(initial_y).map(|(x, y)| pos2(x, y)),
            initial_window_size: vec2(
                initial_width.unwrap_or(DEFAULT_WINDOW_SIZE.x),
                initial_height.unwrap_or(DEFAULT_WINDOW_SIZE.y),
            ),
        }
    }
}

struct TrackRadarApp {
    config: AppConfig,
    snapshot: TrackRadarSnapshot,
    display: RadarDisplayState,
    telemetry_worker: Option<TelemetryWorker>,
    started_at: Instant,
    last_error: Option<String>,
}

impl TrackRadarApp {
    fn new(config: AppConfig) -> Self {
        let snapshot = sample_snapshot(0.0);
        let display = snapshot
            .display_state(config.radar_range_meters)
            .expect("sample display");
        let telemetry_worker = (!config.sample_mode)
            .then(|| TelemetryWorker::spawn(config.base_url.clone(), config.fetch_interval));

        Self {
            config,
            snapshot,
            display,
            telemetry_worker,
            started_at: Instant::now(),
            last_error: None,
        }
    }

    fn refresh_if_needed(&mut self) {
        if self.config.sample_mode {
            self.snapshot = sample_snapshot(self.started_at.elapsed().as_secs_f32());
            self.display = self
                .snapshot
                .display_state(self.config.radar_range_meters)
                .expect("sample display");
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
            TelemetryUpdate::Snapshot(snapshot) => {
                match snapshot.display_state(self.config.radar_range_meters) {
                    Ok(display) => {
                        self.snapshot = snapshot;
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
    Snapshot(TrackRadarSnapshot),
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
            .name("track-radar-fetch".to_string())
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
        let update = match fetch_track_radar(&base_url) {
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

impl eframe::App for TrackRadarApp {
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

                paint_overlay(
                    ui.painter(),
                    rect,
                    &self.display,
                    self.config.radar_range_meters,
                    self.config.idle_opacity,
                    ctx.pointer_hover_pos(),
                );

                if let Some(error) = &self.last_error {
                    ui.painter().text(
                        rect.left_top() + vec2(8.0, 8.0),
                        Align2::LEFT_TOP,
                        error,
                        FontId::proportional(11.0),
                        Color32::from_rgb(255, 120, 120),
                    );
                }
            });
    }
}

fn sample_snapshot(elapsed: f32) -> TrackRadarSnapshot {
    let sweep = elapsed * 0.55;
    let along_left = 2.6 + 0.35 * (elapsed * 1.4).sin();
    let along_right = 2.8 + 0.25 * (elapsed * 1.2 + 1.1).sin();

    TrackRadarSnapshot {
        ref_index: 1,
        drivers: vec![
            sample_driver(1, "Player", "McLaren", 1, 0.0, 0.0, 0.0),
            sample_driver(
                2,
                "Ahead",
                "Ferrari",
                2,
                6.0 * (elapsed * 0.7).sin(),
                13.0 + 3.0 * (elapsed * 0.9).sin(),
                0.3 * (elapsed * 0.7).sin(),
            ),
            sample_driver(
                3,
                "Left",
                "Mercedes",
                3,
                -along_left,
                1.5 * (elapsed * 2.0).sin(),
                FRAC_PI_2 * 0.15,
            ),
            sample_driver(
                4,
                "Right",
                "Red Bull",
                4,
                along_right,
                1.2 * (elapsed * 1.8 + 0.8).sin(),
                PI - 0.25,
            ),
            sample_driver(
                5,
                "Behind",
                "Williams",
                5,
                -5.0 * (elapsed * 0.6 + 0.5).sin(),
                -15.0 - 4.0 * (elapsed * 0.6).sin(),
                PI,
            ),
            sample_driver(
                6,
                "Orbit",
                "Aston Martin",
                6,
                17.0 * sweep.cos(),
                17.0 * sweep.sin(),
                sweep + PI,
            ),
            sample_driver(7, "Far", "Haas", 7, 33.0, -6.0, 0.0),
        ],
    }
}

fn sample_driver(
    index: usize,
    name: &str,
    team: &str,
    track_position: u8,
    x: f32,
    z: f32,
    yaw: f32,
) -> RadarDriver {
    RadarDriver {
        name: name.to_string(),
        team: Some(team.to_string()),
        track_position: Some(track_position),
        index,
        motion: Some(RadarDriverMotion {
            world_position: RadarVector3 { x, y: 0.0, z },
            yaw,
        }),
    }
}

fn paint_overlay(
    painter: &Painter,
    rect: Rect,
    display: &RadarDisplayState,
    radar_range_meters: f32,
    idle_opacity: f32,
    pointer_pos: Option<Pos2>,
) {
    let scale = (rect.width() / TARGET_CANVAS_SIZE.x)
        .min(rect.height() / TARGET_CANVAS_SIZE.y)
        .clamp(0.5, 4.0);
    let overlay_alpha = if display.cars_nearby {
        1.0
    } else {
        idle_opacity
    };
    let radar_size = vec2(rect.width() * 0.85, rect.height() * 0.85);
    let radar = Rect::from_center_size(rect.center(), radar_size);
    let center = radar.center();

    paint_grid(painter, radar, scale, overlay_alpha);
    if display.left_alert {
        paint_alert_sector(painter, radar, scale, overlay_alpha, AlertSide::Left);
    }
    if display.right_alert {
        paint_alert_sector(painter, radar, scale, overlay_alpha, AlertSide::Right);
    }

    let ref_size = vec2(12.0 * scale, 34.0 * scale);
    paint_car(
        painter,
        center,
        ref_size,
        0.0,
        fade_color(REF_CAR_COLOR, overlay_alpha),
        Stroke::new(2.0 * scale, fade_color(Color32::WHITE, overlay_alpha)),
    );

    let mut hovered_driver = None;
    for driver in display.drivers.iter().filter(|driver| !driver.is_ref) {
        if driver.distance > radar_range_meters {
            continue;
        }

        let marker_center = pos2(
            center.x - (driver.relative_x / radar_range_meters) * (radar.width() * 0.5),
            center.y - (driver.relative_z / radar_range_meters) * (radar.height() * 0.5),
        );
        let car_size = vec2(12.0 * scale, 34.0 * scale);
        let marker_rect =
            Rect::from_center_size(marker_center, car_size + vec2(12.0 * scale, 12.0 * scale));
        if pointer_pos.is_some_and(|pointer| marker_rect.contains(pointer)) {
            hovered_driver = Some((driver, marker_center));
        }

        paint_car(
            painter,
            marker_center,
            car_size,
            -driver.relative_heading_deg.to_radians(),
            fade_color(TARGET_CAR_COLOR, overlay_alpha),
            Stroke::new(1.0 * scale, fade_color(TARGET_CAR_BORDER, overlay_alpha)),
        );
    }

    if let Some((driver, marker_center)) = hovered_driver {
        paint_tooltip(painter, driver.name.as_str(), marker_center, scale);
    }
}

fn paint_grid(painter: &Painter, radar: Rect, scale: f32, overlay_alpha: f32) {
    let radius_step = radar.width().min(radar.height()) * 0.125;
    for index in 1..=4 {
        painter.circle_stroke(
            radar.center(),
            radius_step * index as f32,
            Stroke::new(1.0 * scale, fade_color(rgba(GRID_RGBA), overlay_alpha)),
        );
    }

    painter.line_segment(
        [
            pos2(radar.center().x, radar.top()),
            pos2(radar.center().x, radar.bottom()),
        ],
        Stroke::new(1.0 * scale, fade_color(rgba(CROSSHAIR_RGBA), overlay_alpha)),
    );
    painter.line_segment(
        [
            pos2(radar.left(), radar.center().y),
            pos2(radar.right(), radar.center().y),
        ],
        Stroke::new(1.0 * scale, fade_color(rgba(CROSSHAIR_RGBA), overlay_alpha)),
    );
}

#[derive(Clone, Copy)]
enum AlertSide {
    Left,
    Right,
}

fn paint_alert_sector(
    painter: &Painter,
    radar: Rect,
    scale: f32,
    overlay_alpha: f32,
    side: AlertSide,
) {
    let center = radar.center();
    let radius = radar.width().min(radar.height()) * 0.5;
    let car_half_width = 6.0 * scale;
    let car_half_height = 17.0 * scale;

    let (start_angle, end_angle) = match side {
        AlertSide::Left => (
            (-car_half_height).atan2(car_half_width),
            car_half_height.atan2(car_half_width),
        ),
        AlertSide::Right => (
            car_half_height.atan2(-car_half_width),
            (-car_half_height).atan2(-car_half_width),
        ),
    };

    let mut points = vec![center];
    let segments = 24;
    for index in 0..=segments {
        let t = index as f32 / segments as f32;
        let angle = start_angle + (end_angle - start_angle) * t;
        points.push(pos2(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        ));
    }
    points.push(center);

    painter.add(Shape::convex_polygon(
        points,
        fade_color(rgba(ALERT_RGBA), overlay_alpha),
        Stroke::NONE,
    ));
}

fn paint_car(
    painter: &Painter,
    center: Pos2,
    size: Vec2,
    rotation_radians: f32,
    fill: Color32,
    stroke: Stroke,
) {
    let half_width = size.x * 0.5;
    let half_height = size.y * 0.5;
    let corners = [
        vec2(-half_width, -half_height),
        vec2(half_width, -half_height),
        vec2(half_width, half_height),
        vec2(-half_width, half_height),
    ];
    let rotated = corners
        .into_iter()
        .map(|corner| rotate_vec(corner, rotation_radians))
        .map(|corner| pos2(center.x + corner.x, center.y + corner.y))
        .collect::<Vec<_>>();
    painter.add(Shape::convex_polygon(rotated, fill, stroke));
}

fn rotate_vec(value: Vec2, angle: f32) -> Vec2 {
    vec2(
        value.x * angle.cos() - value.y * angle.sin(),
        value.x * angle.sin() + value.y * angle.cos(),
    )
}

fn paint_tooltip(painter: &Painter, label: &str, marker_center: Pos2, scale: f32) {
    let font = FontId::proportional(10.0 * scale);
    let text_color = Color32::WHITE;
    let galley = painter.layout_no_wrap(label.to_string(), font.clone(), text_color);
    let tooltip_size = galley.size() + vec2(8.0 * scale, 4.0 * scale);
    let tooltip_rect = Rect::from_center_size(
        pos2(marker_center.x, marker_center.y - 28.0 * scale),
        tooltip_size,
    );

    painter.rect_filled(
        tooltip_rect,
        Rounding::same(3.0 * scale),
        rgba(TOOLTIP_BG_RGBA),
    );
    painter.galley(
        tooltip_rect.center() - galley.size() * 0.5,
        galley,
        text_color,
    );
}

fn fade_color(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, a] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, ((a as f32) * alpha.clamp(0.0, 1.0)).round() as u8)
}

fn rgba((r, g, b, a): (u8, u8, u8, u8)) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}
