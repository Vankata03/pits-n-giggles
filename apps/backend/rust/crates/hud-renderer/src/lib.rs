use std::error::Error;
use std::fmt;

use serde_json::Value;

const MAX_PERCENTAGE: f32 = 100.0;
const STEERING_RANGE: f32 = 100.0;

#[derive(Debug, Clone, PartialEq)]
pub struct InputTelemetrySnapshot {
    pub throttle: f32,
    pub brake: f32,
    pub steering: f32,
    pub rev_lights_percentage: u8,
}

impl InputTelemetrySnapshot {
    pub fn sample() -> Self {
        Self {
            throttle: 73.5,
            brake: 14.0,
            steering: -11.25,
            rev_lights_percentage: 91,
        }
    }

    pub fn from_stream_overlay_value(value: &Value) -> Result<Self, TelemetryParseError> {
        let telemetry = value
            .get("car-telemetry")
            .or_else(|| value.get("carTelemetry"))
            .ok_or(TelemetryParseError::MissingField("car-telemetry"))?;

        Ok(Self {
            throttle: telemetry_f32(telemetry, &["throttle"])?,
            brake: telemetry_f32(telemetry, &["brake"])?,
            steering: telemetry_f32(telemetry, &["steering", "steer"])?,
            rev_lights_percentage: telemetry_u8(
                telemetry,
                &["rev-lights-percent", "revLightsPercent"],
            )?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HudDisplayMetrics {
    pub throttle_fill: f32,
    pub brake_fill: f32,
    pub steering_fill: f32,
    pub steering_percentage: f32,
    pub rev_lights_fill: f32,
}

impl HudDisplayMetrics {
    pub fn from_snapshot(snapshot: &InputTelemetrySnapshot) -> Self {
        let clamped_steering = snapshot.steering.clamp(-STEERING_RANGE, STEERING_RANGE);
        Self {
            throttle_fill: normalize_percentage(snapshot.throttle),
            brake_fill: normalize_percentage(snapshot.brake),
            steering_fill: (clamped_steering + STEERING_RANGE) / (STEERING_RANGE * 2.0),
            steering_percentage: clamped_steering,
            rev_lights_fill: normalize_percentage(snapshot.rev_lights_percentage as f32),
        }
    }
}

pub fn fetch_input_telemetry(
    base_url: &str,
) -> Result<InputTelemetrySnapshot, FetchTelemetryError> {
    let base_url = base_url.trim_end_matches('/');
    let url = format!("{base_url}/stream-overlay-info");
    let response = reqwest::blocking::get(url).map_err(FetchTelemetryError::Http)?;
    let response = response
        .error_for_status()
        .map_err(FetchTelemetryError::Http)?;
    let value = response
        .json::<Value>()
        .map_err(FetchTelemetryError::Http)?;
    InputTelemetrySnapshot::from_stream_overlay_value(&value).map_err(FetchTelemetryError::Parse)
}

fn normalize_percentage(value: f32) -> f32 {
    (value / MAX_PERCENTAGE).clamp(0.0, 1.0)
}

fn telemetry_f32(
    telemetry: &Value,
    keys: &'static [&'static str],
) -> Result<f32, TelemetryParseError> {
    keys.iter()
        .find_map(|key| {
            telemetry
                .get(key)
                .and_then(Value::as_f64)
                .map(|value| value as f32)
        })
        .ok_or(TelemetryParseError::MissingAnyField(keys))
}

fn telemetry_u8(
    telemetry: &Value,
    keys: &'static [&'static str],
) -> Result<u8, TelemetryParseError> {
    keys.iter()
        .find_map(|key| telemetry.get(key).and_then(Value::as_u64))
        .and_then(|value| u8::try_from(value).ok())
        .ok_or(TelemetryParseError::MissingAnyField(keys))
}

#[derive(Debug, Clone, PartialEq)]
pub enum TelemetryParseError {
    MissingField(&'static str),
    MissingAnyField(&'static [&'static str]),
}

impl fmt::Display for TelemetryParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing telemetry field '{field}'"),
            Self::MissingAnyField(fields) => write!(
                f,
                "missing telemetry field, expected one of: {}",
                fields.join(", ")
            ),
        }
    }
}

impl Error for TelemetryParseError {}

#[derive(Debug)]
pub enum FetchTelemetryError {
    Http(reqwest::Error),
    Parse(TelemetryParseError),
}

impl fmt::Display for FetchTelemetryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(f, "failed to fetch stream overlay data: {error}"),
            Self::Parse(error) => write!(f, "failed to parse stream overlay data: {error}"),
        }
    }
}

impl Error for FetchTelemetryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{HudDisplayMetrics, InputTelemetrySnapshot, TelemetryParseError};

    #[test]
    fn parses_backend_stream_overlay_shape() {
        let value = json!({
            "car-telemetry": {
                "throttle": 73.5,
                "brake": 14.0,
                "steering": -11.25,
                "rev-lights-percent": 91
            }
        });

        let telemetry =
            InputTelemetrySnapshot::from_stream_overlay_value(&value).expect("parse telemetry");

        assert_eq!(
            telemetry,
            InputTelemetrySnapshot {
                throttle: 73.5,
                brake: 14.0,
                steering: -11.25,
                rev_lights_percentage: 91,
            }
        );
    }

    #[test]
    fn parses_fallback_field_names_for_future_prototypes() {
        let value = json!({
            "carTelemetry": {
                "throttle": 10.0,
                "brake": 20.0,
                "steer": 30.0,
                "revLightsPercent": 40
            }
        });

        let telemetry =
            InputTelemetrySnapshot::from_stream_overlay_value(&value).expect("parse telemetry");

        assert_eq!(telemetry.throttle, 10.0);
        assert_eq!(telemetry.brake, 20.0);
        assert_eq!(telemetry.steering, 30.0);
        assert_eq!(telemetry.rev_lights_percentage, 40);
    }

    #[test]
    fn errors_when_car_telemetry_block_is_missing() {
        let error = InputTelemetrySnapshot::from_stream_overlay_value(&json!({}))
            .expect_err("missing telemetry block");

        assert_eq!(error, TelemetryParseError::MissingField("car-telemetry"));
    }

    #[test]
    fn hud_display_metrics_clamp_expected_ranges() {
        let metrics = HudDisplayMetrics::from_snapshot(&InputTelemetrySnapshot {
            throttle: 135.0,
            brake: -20.0,
            steering: -150.0,
            rev_lights_percentage: 255,
        });

        assert_eq!(metrics.throttle_fill, 1.0);
        assert_eq!(metrics.brake_fill, 0.0);
        assert_eq!(metrics.steering_fill, 0.0);
        assert_eq!(metrics.steering_percentage, -100.0);
        assert_eq!(metrics.rev_lights_fill, 1.0);
    }
}
