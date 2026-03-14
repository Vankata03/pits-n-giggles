use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::Value;

use crate::FetchTelemetryError;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(100);
const FETCH_TIMEOUT: Duration = Duration::from_millis(250);

pub fn fetch_input_telemetry_value(base_url: &str) -> Result<Value, FetchTelemetryError> {
    fetch_json_endpoint(base_url, "hud/input-telemetry")
}

pub fn fetch_track_radar_value(base_url: &str) -> Result<Value, FetchTelemetryError> {
    fetch_json_endpoint(base_url, "hud/track-radar")
}

pub fn fetch_stream_overlay_value(base_url: &str) -> Result<Value, FetchTelemetryError> {
    fetch_json_endpoint(base_url, "stream-overlay-info")
}

pub fn fetch_telemetry_info_value(base_url: &str) -> Result<Value, FetchTelemetryError> {
    fetch_json_endpoint(base_url, "telemetry-info")
}

fn fetch_json_endpoint(base_url: &str, endpoint: &str) -> Result<Value, FetchTelemetryError> {
    let base_url = base_url.trim_end_matches('/');
    let endpoint = endpoint.trim_start_matches('/');
    let url = format!("{base_url}/{endpoint}");
    let response = telemetry_client()
        .get(url)
        .send()
        .map_err(FetchTelemetryError::Http)?;
    let response = response
        .error_for_status()
        .map_err(FetchTelemetryError::Http)?;
    response.json::<Value>().map_err(FetchTelemetryError::Http)
}

fn telemetry_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("pits-n-giggles-hud-renderer")
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(FETCH_TIMEOUT)
            .build()
            .expect("build telemetry client")
    })
}
