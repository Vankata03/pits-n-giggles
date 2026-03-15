use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender, TryRecvError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Rounding, Sense, Stroke, Vec2, pos2, vec2,
};
use hud_renderer::{TimingTowerDisplay, TimingTowerEntry, TimingTowerSnapshot, fetch_timing_tower};

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:4768";
const DEFAULT_RENDER_INTERVAL_MS: u64 = 16;
const DEFAULT_FETCH_INTERVAL_MS: u64 = 250;
const DEFAULT_NUM_ADJACENT_CARS: usize = 2;
const MAX_SUPPORTED_CARS: usize = 22;

const ROW_HEIGHT: f32 = 32.0;
const HEADER_HEIGHT: f32 = 40.0;
const MARGINS: f32 = 25.0;
const POS_WIDTH: f32 = 40.0;
const TEAM_WIDTH: f32 = 30.0;
const NAME_WIDTH: f32 = 160.0;
const DELTA_WIDTH: f32 = 90.0;
const TYRE_WIDTH: f32 = 75.0;
const ERS_WIDTH: f32 = 75.0;
const PENS_WIDTH: f32 = 80.0;

const TEXT_WHITE: Color32 = Color32::from_rgb(255, 255, 255);
const TEXT_DIM: Color32 = Color32::from_rgb(221, 221, 221);
const ERROR_COLOR: Color32 = Color32::from_rgb(255, 184, 107);
const ERROR_FETCH_COLOR: Color32 = Color32::from_rgb(255, 120, 120);
const BORDER_COLOR: Color32 = Color32::from_rgb(0, 0, 0);
const HEADER_BG_RGBA: (u8, u8, u8, u8) = (15, 15, 15, 204);
const HEADER_INNER_BG_RGBA: (u8, u8, u8, u8) = (41, 41, 41, 204);
const ROW_BG_EVEN_RGBA: (u8, u8, u8, u8) = (26, 26, 26, 178);
const ROW_BG_ODD_RGBA: (u8, u8, u8, u8) = (20, 20, 20, 178);
const CELL_BG_RGBA: (u8, u8, u8, u8) = (26, 26, 26, 178);

fn main() -> Result<(), eframe::Error> {
    let config = AppConfig::from_env();
    let title = if config.sample_mode {
        format!("{} (sample)", config.title)
    } else {
        config.title.clone()
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(config.initial_window_size)
        .with_min_inner_size(vec2(config.base_width(), config.base_height()))
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
        Box::new(move |creation_context| {
            let mut style = (*creation_context.egui_ctx.style()).clone();
            style.visuals.window_fill = Color32::TRANSPARENT;
            style.visuals.panel_fill = Color32::TRANSPARENT;
            style.visuals.extreme_bg_color = Color32::TRANSPARENT;
            creation_context.egui_ctx.set_style(style);
            Box::new(TimingTowerApp::new(config))
        }),
    )
}

#[derive(Clone, Debug)]
struct AppConfig {
    sample_mode: bool,
    base_url: String,
    title: String,
    render_interval: Duration,
    fetch_interval: Duration,
    num_adjacent_cars: usize,
    show_team_logos: bool,
    show_tyre_info: bool,
    show_deltas: bool,
    show_ers_drs_info: bool,
    show_pens: bool,
    show_tl_warns: bool,
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
        let mut num_adjacent_cars = None;
        let mut show_team_logos = None;
        let mut show_tyre_info = None;
        let mut show_deltas = None;
        let mut show_ers_drs_info = None;
        let mut show_pens = None;
        let mut show_tl_warns = None;
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
                "--num-adjacent-cars" => {
                    if let Some(value) = args.next() {
                        num_adjacent_cars = value
                            .parse::<usize>()
                            .ok()
                            .filter(|value| *value <= MAX_SUPPORTED_CARS);
                    }
                }
                "--show-team-logos" => {
                    if let Some(value) = args.next() {
                        show_team_logos = parse_bool_arg(&value);
                    }
                }
                "--show-tyre-info" => {
                    if let Some(value) = args.next() {
                        show_tyre_info = parse_bool_arg(&value);
                    }
                }
                "--show-deltas" => {
                    if let Some(value) = args.next() {
                        show_deltas = parse_bool_arg(&value);
                    }
                }
                "--show-ers-drs-info" => {
                    if let Some(value) = args.next() {
                        show_ers_drs_info = parse_bool_arg(&value);
                    }
                }
                "--show-pens" => {
                    if let Some(value) = args.next() {
                        show_pens = parse_bool_arg(&value);
                    }
                }
                "--show-tl-warns" => {
                    if let Some(value) = args.next() {
                        show_tl_warns = parse_bool_arg(&value);
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

        let num_adjacent_cars = num_adjacent_cars.unwrap_or(DEFAULT_NUM_ADJACENT_CARS);
        let show_team_logos = show_team_logos.unwrap_or(true);
        let show_tyre_info = show_tyre_info.unwrap_or(true);
        let show_deltas = show_deltas.unwrap_or(true);
        let show_ers_drs_info = show_ers_drs_info.unwrap_or(true);
        let show_pens = show_pens.unwrap_or(true);
        let show_tl_warns = show_tl_warns.unwrap_or(false);
        let base_width = base_width(
            show_team_logos,
            show_tyre_info,
            show_deltas,
            show_ers_drs_info,
            show_pens,
        );
        let base_height = base_height(num_adjacent_cars);

        Self {
            sample_mode,
            base_url: positional_base_url.unwrap_or(base_url),
            title: title
                .unwrap_or_else(|| "pits n giggles - Rust Timing Tower prototype".to_string()),
            render_interval: Duration::from_millis(
                render_interval_ms.unwrap_or(DEFAULT_RENDER_INTERVAL_MS),
            ),
            fetch_interval: Duration::from_millis(
                fetch_interval_ms.unwrap_or(DEFAULT_FETCH_INTERVAL_MS),
            ),
            num_adjacent_cars,
            show_team_logos,
            show_tyre_info,
            show_deltas,
            show_ers_drs_info,
            show_pens,
            show_tl_warns,
            initial_position: initial_x.zip(initial_y).map(|(x, y)| pos2(x, y)),
            initial_window_size: vec2(
                initial_width.unwrap_or(base_width),
                initial_height.unwrap_or(base_height),
            ),
        }
    }

    fn total_rows(&self) -> usize {
        ((self.num_adjacent_cars * 2) + 1).min(MAX_SUPPORTED_CARS)
    }

    fn base_width(&self) -> f32 {
        base_width(
            self.show_team_logos,
            self.show_tyre_info,
            self.show_deltas,
            self.show_ers_drs_info,
            self.show_pens,
        )
    }

    fn base_height(&self) -> f32 {
        base_height(self.num_adjacent_cars)
    }
}

struct TimingTowerApp {
    config: AppConfig,
    display: TimingTowerDisplay,
    telemetry_worker: Option<TelemetryWorker>,
    started_at: Instant,
    last_error: Option<String>,
}

impl TimingTowerApp {
    fn new(config: AppConfig) -> Self {
        let display = sample_snapshot(0.0).display_state(config.num_adjacent_cars);
        let telemetry_worker = (!config.sample_mode)
            .then(|| TelemetryWorker::spawn(config.base_url.clone(), config.fetch_interval));

        Self {
            config,
            display,
            telemetry_worker,
            started_at: Instant::now(),
            last_error: None,
        }
    }

    fn refresh_if_needed(&mut self) {
        if self.config.sample_mode {
            self.display = sample_snapshot(self.started_at.elapsed().as_secs_f32())
                .display_state(self.config.num_adjacent_cars);
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
                self.display = snapshot.display_state(self.config.num_adjacent_cars);
                self.last_error = None;
            }
            TelemetryUpdate::Error(error) => {
                self.last_error = Some(error);
            }
        }
    }
}

enum TelemetryUpdate {
    Snapshot(TimingTowerSnapshot),
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
            .name("timing-tower-fetch".to_string())
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
        let update = match fetch_timing_tower(&base_url) {
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

impl eframe::App for TimingTowerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.refresh_if_needed();
        ctx.request_repaint_after(self.config.render_interval);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                if response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                paint_overlay(ui.painter(), rect, &self.display, &self.config);

                if let Some(error) = &self.last_error {
                    ui.painter().text(
                        rect.left_top() + vec2(8.0, 8.0),
                        Align2::LEFT_TOP,
                        error,
                        FontId::proportional(11.0),
                        ERROR_FETCH_COLOR,
                    );
                }
            });
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

fn paint_overlay(painter: &Painter, rect: Rect, display: &TimingTowerDisplay, config: &AppConfig) {
    let scale = (rect.width() / config.base_width())
        .min(rect.height() / config.base_height())
        .clamp(0.6, 4.0);
    let content = Rect::from_center_size(
        rect.center(),
        vec2(config.base_width() * scale, config.base_height() * scale),
    );
    let inner = content.shrink(5.0 * scale);
    let header_rect = Rect::from_min_size(inner.min, vec2(inner.width(), 35.0 * scale));
    let header_inner_rect = Rect::from_center_size(
        header_rect.center(),
        vec2((header_rect.width() - 6.0 * scale).max(0.0), 25.0 * scale),
    );
    let table_rect = Rect::from_min_size(
        pos2(inner.left(), header_rect.bottom() + 5.0 * scale),
        vec2(
            inner.width(),
            (inner.height() - header_rect.height() - 5.0 * scale).max(0.0),
        ),
    );

    painter.rect_filled(
        header_rect,
        Rounding::same(5.0 * scale),
        rgba(HEADER_BG_RGBA),
    );
    painter.rect_filled(
        header_inner_rect,
        Rounding::same(3.0 * scale),
        rgba(HEADER_INNER_BG_RGBA),
    );
    painter.text(
        header_inner_rect.center(),
        Align2::CENTER_CENTER,
        display.session_info.as_str(),
        FontId::proportional(15.0 * scale),
        TEXT_WHITE,
    );

    if let Some(error_message) = &display.error_message {
        painter.text(
            table_rect.center(),
            Align2::CENTER_CENTER,
            error_message.as_str(),
            FontId::proportional(12.0 * scale),
            ERROR_COLOR,
        );
        return;
    }

    for (row_index, row) in display.rows.iter().take(config.total_rows()).enumerate() {
        let row_rect = Rect::from_min_size(
            pos2(
                table_rect.left(),
                table_rect.top() + (row_index as f32 * ROW_HEIGHT * scale),
            ),
            vec2(table_rect.width(), ROW_HEIGHT * scale),
        );
        paint_row(
            painter,
            row_rect,
            row,
            display.ref_driver_index,
            config,
            scale,
            row_index,
        );
    }
}

fn paint_row(
    painter: &Painter,
    row_rect: Rect,
    row: &TimingTowerEntry,
    ref_driver_index: usize,
    config: &AppConfig,
    scale: f32,
    row_index: usize,
) {
    let row_bg = if row_index % 2 == 0 {
        rgba(ROW_BG_EVEN_RGBA)
    } else {
        rgba(ROW_BG_ODD_RGBA)
    };
    painter.rect_filled(row_rect, Rounding::same(2.0 * scale), row_bg);

    if row.index == ref_driver_index {
        painter.rect_stroke(
            row_rect,
            Rounding::same(2.0 * scale),
            Stroke::new(2.0 * scale, TEXT_WHITE),
        );
    }

    let mut x = row_rect.left() + 4.0 * scale;

    let pos_rect = Rect::from_min_size(
        pos2(x, row_rect.top()),
        vec2(POS_WIDTH * scale, row_rect.height()),
    );
    paint_center_text(
        painter,
        pos_rect,
        &format!("{:>2}", row.position),
        FontId::proportional(12.0 * scale),
        TEXT_DIM,
    );
    x += POS_WIDTH * scale;

    if config.show_team_logos {
        let team_rect = Rect::from_min_size(
            pos2(x, row_rect.top()),
            vec2(TEAM_WIDTH * scale, row_rect.height()),
        );
        paint_team_marker(painter, team_rect, row.team.as_str(), scale);
        x += TEAM_WIDTH * scale;
    }

    let name_rect = Rect::from_min_size(
        pos2(x, row_rect.top()),
        vec2(NAME_WIDTH * scale, row_rect.height()),
    );
    painter.text(
        pos2(name_rect.left() + 4.0 * scale, name_rect.center().y),
        Align2::LEFT_CENTER,
        elide_text(row.name.as_str(), 18),
        FontId::proportional(13.0 * scale),
        TEXT_WHITE,
    );
    x += NAME_WIDTH * scale;

    if config.show_deltas {
        let delta_rect = Rect::from_min_size(
            pos2(x, row_rect.top()),
            vec2(DELTA_WIDTH * scale, row_rect.height()),
        );
        paint_center_text(
            painter,
            delta_rect,
            &format_delta(row, ref_driver_index),
            FontId::proportional(13.0 * scale),
            TEXT_WHITE,
        );
        x += DELTA_WIDTH * scale;
    }

    if config.show_tyre_info {
        let tyre_rect = Rect::from_min_size(
            pos2(x, row_rect.top()),
            vec2(TYRE_WIDTH * scale, row_rect.height()),
        );
        paint_tyre_cell(painter, tyre_rect, row, scale);
        x += TYRE_WIDTH * scale;
    }

    if config.show_ers_drs_info {
        let ers_width = if config.show_pens {
            ERS_WIDTH
        } else {
            ERS_WIDTH + 10.0
        };
        let ers_rect = Rect::from_min_size(
            pos2(x, row_rect.top()),
            vec2(ers_width * scale, row_rect.height()),
        );
        paint_ers_cell(painter, ers_rect, row, scale);
        x += ers_width * scale;
    }

    if config.show_pens {
        let pens_rect = Rect::from_min_size(
            pos2(x, row_rect.top()),
            vec2(PENS_WIDTH * scale, row_rect.height()),
        );
        paint_center_text(
            painter,
            pens_rect,
            &format_penalties(row, config.show_tl_warns),
            FontId::proportional(11.0 * scale),
            TEXT_WHITE,
        );
    }
}

fn paint_center_text(painter: &Painter, rect: Rect, text: &str, font: FontId, color: Color32) {
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

fn paint_team_marker(painter: &Painter, rect: Rect, team: &str, scale: f32) {
    let marker_rect = Rect::from_center_size(rect.center(), vec2(16.0 * scale, 20.0 * scale));
    painter.rect_filled(marker_rect, Rounding::same(2.0 * scale), team_color(team));
}

fn paint_tyre_cell(painter: &Painter, rect: Rect, row: &TimingTowerEntry, scale: f32) {
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + 14.0 * scale, rect.center().y),
        vec2(18.0 * scale, 18.0 * scale),
    );
    painter.circle_filled(
        icon_rect.center(),
        9.0 * scale,
        compound_color(&row.visual_tyre_compound),
    );
    painter.text(
        icon_rect.center(),
        Align2::CENTER_CENTER,
        compound_label(&row.visual_tyre_compound),
        FontId::proportional(9.0 * scale),
        Color32::BLACK,
    );
    painter.text(
        pos2(rect.left() + 28.0 * scale, rect.center().y),
        Align2::LEFT_CENTER,
        format_tyre_info(row),
        FontId::proportional(13.0 * scale),
        TEXT_WHITE,
    );
}

fn paint_ers_cell(painter: &Painter, rect: Rect, row: &TimingTowerEntry, scale: f32) {
    painter.rect_filled(rect, Rounding::ZERO, rgba(CELL_BG_RGBA));

    let ers_mode_rect = Rect::from_min_size(rect.min, vec2(rect.width() * 0.15, rect.height()));
    let ers_text_rect = Rect::from_min_size(
        pos2(ers_mode_rect.right(), rect.top()),
        vec2(rect.width() * 0.70, rect.height()),
    );
    let drs_rect = Rect::from_min_size(
        pos2(ers_text_rect.right(), rect.top()),
        vec2(
            (rect.right() - ers_text_rect.right()).max(0.0),
            rect.height(),
        ),
    );

    painter.rect_filled(
        ers_mode_rect,
        Rounding::ZERO,
        ers_mode_color(row.ers_mode.as_str()),
    );
    painter.rect_filled(
        drs_rect,
        Rounding::ZERO,
        if row.drs {
            Color32::from_rgb(0, 255, 0)
        } else {
            Color32::from_rgb(136, 136, 136)
        },
    );
    painter.rect_stroke(rect, Rounding::ZERO, Stroke::new(1.0 * scale, BORDER_COLOR));
    paint_center_text(
        painter,
        ers_text_rect,
        &format_ers(row),
        FontId::proportional(12.0 * scale),
        TEXT_WHITE,
    );
}

fn format_delta(row: &TimingTowerEntry, ref_driver_index: usize) -> String {
    if row.is_pitting {
        return "PIT".to_string();
    }

    if let Some(status) = &row.dnf_status {
        if status == "DNF" || status == "DSQ" {
            return status.clone();
        }
    }

    let Some(relative_delta_ms) = row.relative_delta_ms else {
        return "---".to_string();
    };
    if row.index == ref_driver_index || relative_delta_ms == 0 {
        return "---".to_string();
    }

    format_signed_seconds(relative_delta_ms as f32 / 1000.0)
}

fn format_tyre_info(row: &TimingTowerEntry) -> String {
    if row.telemetry_public {
        row.max_tyre_wear_percent
            .map(|wear| format!("{wear}%"))
            .unwrap_or_else(|| "N/A".to_string())
    } else {
        format!("{}L", row.tyre_age_laps)
    }
}

fn format_ers(row: &TimingTowerEntry) -> String {
    if !row.telemetry_public {
        return "N/A".to_string();
    }

    row.ers_percent
        .map(|value| format!("{}%", value.round() as i32))
        .unwrap_or_else(|| "0%".to_string())
}

fn format_penalties(row: &TimingTowerEntry, show_tl_warns: bool) -> String {
    if row.num_dt > 0 {
        return format!("{}DT", row.num_dt);
    }

    let penalty_seconds = row.time_penalties_sec + (u32::from(row.num_sg) * 10);
    if penalty_seconds > 0 {
        return format!("+{penalty_seconds}sec");
    }

    if show_tl_warns {
        return format!("TL: {}", row.corner_cutting_warnings);
    }

    String::new()
}

fn format_signed_seconds(value: f32) -> String {
    if value >= 0.0 {
        format!("+{value:.3}")
    } else {
        format!("{value:.3}")
    }
}

fn team_color(team: &str) -> Color32 {
    const PALETTE: [Color32; 12] = [
        Color32::from_rgb(232, 65, 24),
        Color32::from_rgb(243, 156, 18),
        Color32::from_rgb(241, 196, 15),
        Color32::from_rgb(46, 204, 113),
        Color32::from_rgb(26, 188, 156),
        Color32::from_rgb(52, 152, 219),
        Color32::from_rgb(41, 128, 185),
        Color32::from_rgb(155, 89, 182),
        Color32::from_rgb(192, 57, 43),
        Color32::from_rgb(127, 140, 141),
        Color32::from_rgb(230, 126, 34),
        Color32::from_rgb(22, 160, 133),
    ];

    let hash = team.bytes().fold(0u32, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(u32::from(byte))
    });
    PALETTE[(hash as usize) % PALETTE.len()]
}

fn compound_color(compound: &str) -> Color32 {
    let normalized = compound.to_ascii_lowercase();
    if normalized.contains("soft") {
        Color32::from_rgb(255, 64, 64)
    } else if normalized.contains("medium") {
        Color32::from_rgb(255, 224, 64)
    } else if normalized.contains("hard") {
        Color32::from_rgb(224, 224, 224)
    } else if normalized.contains("inter") {
        Color32::from_rgb(80, 200, 120)
    } else if normalized.contains("wet") {
        Color32::from_rgb(64, 164, 223)
    } else {
        Color32::from_rgb(180, 180, 180)
    }
}

fn compound_label(compound: &str) -> String {
    let normalized = compound.to_ascii_lowercase();
    if normalized.contains("soft") {
        "S".to_string()
    } else if normalized.contains("medium") {
        "M".to_string()
    } else if normalized.contains("hard") {
        "H".to_string()
    } else if normalized.contains("inter") {
        "I".to_string()
    } else if normalized.contains("wet") {
        "W".to_string()
    } else {
        compound
            .chars()
            .next()
            .map(|value| value.to_uppercase().collect())
            .unwrap_or_else(|| "?".to_string())
    }
}

fn ers_mode_color(mode: &str) -> Color32 {
    match mode {
        "Medium" => Color32::from_rgb(255, 255, 0),
        "Hotlap" => Color32::from_rgb(0, 255, 0),
        "Overtake" => Color32::from_rgb(255, 0, 0),
        _ => Color32::from_rgb(136, 136, 136),
    }
}

fn elide_text(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let head = max_chars.saturating_sub(3);
    let mut output = text.chars().take(head).collect::<String>();
    output.push_str("...");
    output
}

fn parse_bool_arg(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn base_width(
    show_team_logos: bool,
    show_tyre_info: bool,
    show_deltas: bool,
    show_ers_drs_info: bool,
    show_pens: bool,
) -> f32 {
    let mut width = POS_WIDTH + NAME_WIDTH;
    if show_team_logos {
        width += TEAM_WIDTH;
    }
    if show_deltas {
        width += DELTA_WIDTH;
    }
    if show_tyre_info {
        width += TYRE_WIDTH;
    }
    if show_ers_drs_info {
        width += if show_pens {
            ERS_WIDTH
        } else {
            ERS_WIDTH + 10.0
        };
    }
    if show_pens {
        width += PENS_WIDTH;
    }
    width + 20.0
}

fn base_height(num_adjacent_cars: usize) -> f32 {
    HEADER_HEIGHT
        + (ROW_HEIGHT * (((num_adjacent_cars * 2) + 1).min(MAX_SUPPORTED_CARS) as f32))
        + MARGINS
}

fn rgba((r, g, b, a): (u8, u8, u8, u8)) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn sample_snapshot(elapsed: f32) -> TimingTowerSnapshot {
    let mut entries = vec![
        sample_entry(
            3,
            1,
            false,
            "NORRIS",
            "McLaren",
            0,
            Some(91_204),
            0,
            14,
            false,
            None,
            "Hotlap",
            74.0,
        ),
        sample_entry(
            1,
            2,
            false,
            "LECLERC",
            "Ferrari",
            820,
            Some(91_430),
            0,
            18,
            true,
            None,
            "Medium",
            56.0,
        ),
        sample_entry(
            7,
            3,
            true,
            "PLAYER",
            "Red Bull",
            610,
            Some(91_880),
            0,
            16,
            true,
            None,
            "Overtake",
            48.0,
        ),
        sample_entry(
            11,
            4,
            false,
            "RUSSELL",
            "Mercedes",
            410,
            Some(92_010),
            5,
            21,
            false,
            None,
            "Hotlap",
            62.0,
        ),
        sample_entry(
            14,
            5,
            false,
            "ALONSO",
            "Aston Martin",
            1_120,
            Some(92_442),
            0,
            27,
            false,
            Some("DNF"),
            "None",
            0.0,
        ),
    ];

    if let Some(entry) = entries.get_mut(3) {
        entry.is_pitting = elapsed.sin() > 0.4;
    }

    TimingTowerSnapshot {
        session_type: "Race".to_string(),
        current_lap: Some(14),
        total_laps: Some(57),
        session_time_left_sec: Some(2_123),
        ref_driver_index: 7,
        table_entries: entries,
    }
}

fn sample_entry(
    index: usize,
    position: u8,
    is_player: bool,
    name: &str,
    team: &str,
    delta_to_car_in_front_ms: i32,
    best_lap_time_ms: Option<u32>,
    time_penalties_sec: u32,
    max_tyre_wear_percent: u8,
    drs: bool,
    dnf_status: Option<&str>,
    ers_mode: &str,
    ers_percent: f32,
) -> TimingTowerEntry {
    TimingTowerEntry {
        index,
        position,
        is_player,
        name: name.to_string(),
        team: team.to_string(),
        telemetry_public: true,
        drs,
        is_pitting: false,
        dnf_status: dnf_status.map(ToOwned::to_owned),
        delta_to_car_in_front_ms,
        best_lap_time_ms,
        visual_tyre_compound: if position % 2 == 0 {
            "Medium".to_string()
        } else {
            "Soft".to_string()
        },
        tyre_age_laps: 8,
        max_tyre_wear_percent: Some(max_tyre_wear_percent),
        ers_percent: Some(ers_percent),
        ers_mode: ers_mode.to_string(),
        num_dt: 0,
        num_sg: 0,
        time_penalties_sec,
        corner_cutting_warnings: position.saturating_sub(1),
        relative_delta_ms: None,
    }
}
