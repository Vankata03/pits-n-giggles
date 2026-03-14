use serde_json::Value;

use crate::{FetchTelemetryError, TelemetryParseError, fetch_track_radar_value};

const SIDE_ALERT_MIN_X: f32 = 1.5;
const SIDE_ALERT_MAX_X: f32 = 4.0;
const SIDE_ALERT_MAX_Z: f32 = 8.0;

#[derive(Debug, Clone, PartialEq)]
pub struct RadarVector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadarDriverMotion {
    pub world_position: RadarVector3,
    pub yaw: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadarDriver {
    pub name: String,
    pub team: Option<String>,
    pub track_position: Option<u8>,
    pub index: usize,
    pub motion: Option<RadarDriverMotion>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrackRadarSnapshot {
    pub ref_index: usize,
    pub drivers: Vec<RadarDriver>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadarDriverState {
    pub name: String,
    pub team: Option<String>,
    pub track_position: Option<u8>,
    pub index: usize,
    pub is_ref: bool,
    pub relative_x: f32,
    pub relative_z: f32,
    pub relative_heading_deg: f32,
    pub distance: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadarDisplayState {
    pub drivers: Vec<RadarDriverState>,
    pub cars_nearby: bool,
    pub left_alert: bool,
    pub right_alert: bool,
}

impl TrackRadarSnapshot {
    pub fn from_stream_overlay_value(value: &Value) -> Result<Self, TelemetryParseError> {
        let ref_index = value
            .get("ref-index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(TelemetryParseError::MissingField("ref-index"))?;
        let drivers = value
            .get("motion")
            .and_then(Value::as_array)
            .ok_or(TelemetryParseError::MissingField("motion"))?
            .iter()
            .map(parse_driver)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { ref_index, drivers })
    }

    pub fn display_state(
        &self,
        radar_range: f32,
    ) -> Result<RadarDisplayState, TelemetryParseError> {
        let reference_driver = self
            .drivers
            .iter()
            .find(|driver| driver.index == self.ref_index)
            .ok_or(TelemetryParseError::InvalidField("ref-index"))?;
        let reference_motion = reference_driver
            .motion
            .as_ref()
            .ok_or(TelemetryParseError::InvalidField("motion"))?;

        let ref_pos = &reference_motion.world_position;
        let ref_yaw = reference_motion.yaw;
        let cos_yaw = (-ref_yaw).cos();
        let sin_yaw = (-ref_yaw).sin();

        let mut cars_nearby = false;
        let mut left_alert = false;
        let mut right_alert = false;

        let drivers = self
            .drivers
            .iter()
            .filter_map(|driver| {
                let is_ref = driver.index == self.ref_index;
                let motion = driver.motion.as_ref()?;

                let dx = motion.world_position.x - ref_pos.x;
                let dz = motion.world_position.z - ref_pos.z;
                let relative_x = dz * sin_yaw + dx * cos_yaw;
                let relative_z = dz * cos_yaw - dx * sin_yaw;
                let distance = (relative_x.powi(2) + relative_z.powi(2)).sqrt();

                if !is_ref {
                    if distance <= radar_range {
                        cars_nearby = true;
                    }
                    if relative_x < -SIDE_ALERT_MIN_X
                        && relative_x > -SIDE_ALERT_MAX_X
                        && relative_z.abs() < SIDE_ALERT_MAX_Z
                    {
                        left_alert = true;
                    }
                    if relative_x > SIDE_ALERT_MIN_X
                        && relative_x < SIDE_ALERT_MAX_X
                        && relative_z.abs() < SIDE_ALERT_MAX_Z
                    {
                        right_alert = true;
                    }
                }

                Some(RadarDriverState {
                    name: driver.name.clone(),
                    team: driver.team.clone(),
                    track_position: driver.track_position,
                    index: driver.index,
                    is_ref,
                    relative_x,
                    relative_z,
                    relative_heading_deg: (motion.yaw - ref_yaw).to_degrees(),
                    distance,
                })
            })
            .collect::<Vec<_>>();

        Ok(RadarDisplayState {
            drivers,
            cars_nearby,
            left_alert,
            right_alert,
        })
    }
}

pub fn fetch_track_radar(base_url: &str) -> Result<TrackRadarSnapshot, FetchTelemetryError> {
    let value = fetch_track_radar_value(base_url)?;
    TrackRadarSnapshot::from_stream_overlay_value(&value).map_err(FetchTelemetryError::Parse)
}

fn parse_driver(value: &Value) -> Result<RadarDriver, TelemetryParseError> {
    Ok(RadarDriver {
        name: value
            .get("name")
            .and_then(Value::as_str)
            .ok_or(TelemetryParseError::MissingField("name"))?
            .to_string(),
        team: value
            .get("team")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        track_position: value
            .get("track-position")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok()),
        index: value
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or(TelemetryParseError::MissingField("index"))?,
        motion: parse_motion(value.get("motion"))?,
    })
}

fn parse_motion(value: Option<&Value>) -> Result<Option<RadarDriverMotion>, TelemetryParseError> {
    let Some(value) = value else {
        return Err(TelemetryParseError::MissingField("motion"));
    };
    if value.is_null() {
        return Ok(None);
    }

    let world_position = value
        .get("world-position")
        .ok_or(TelemetryParseError::MissingField("world-position"))?;
    let orientation = value
        .get("orientation")
        .ok_or(TelemetryParseError::MissingField("orientation"))?;

    Ok(Some(RadarDriverMotion {
        world_position: RadarVector3 {
            x: world_position
                .get("x")
                .and_then(Value::as_f64)
                .map(|value| value as f32)
                .ok_or(TelemetryParseError::MissingField("world-position.x"))?,
            y: world_position
                .get("y")
                .and_then(Value::as_f64)
                .map(|value| value as f32)
                .ok_or(TelemetryParseError::MissingField("world-position.y"))?,
            z: world_position
                .get("z")
                .and_then(Value::as_f64)
                .map(|value| value as f32)
                .ok_or(TelemetryParseError::MissingField("world-position.z"))?,
        },
        yaw: orientation
            .get("yaw")
            .and_then(Value::as_f64)
            .map(|value| value as f32)
            .ok_or(TelemetryParseError::MissingField("orientation.yaw"))?,
    }))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use serde_json::json;

    use super::TrackRadarSnapshot;

    #[test]
    fn parses_track_radar_shape_from_stream_overlay_payload() {
        let value = json!({
            "ref-index": 7,
            "motion": [
                {
                    "name": "Driver 7",
                    "team": "Ferrari",
                    "track-position": 3,
                    "index": 7,
                    "motion": {
                        "world-position": { "x": 10.0, "y": 0.0, "z": 100.0 },
                        "orientation": { "yaw": 0.5 }
                    }
                }
            ]
        });

        let snapshot = TrackRadarSnapshot::from_stream_overlay_value(&value).expect("parse");

        assert_eq!(snapshot.ref_index, 7);
        assert_eq!(snapshot.drivers.len(), 1);
        assert_eq!(snapshot.drivers[0].name, "Driver 7");
        assert_eq!(
            snapshot.drivers[0]
                .motion
                .as_ref()
                .expect("driver motion")
                .world_position
                .z,
            100.0
        );
    }

    #[test]
    fn computes_relative_positions_and_side_alerts_like_legacy_overlay() {
        let value = json!({
            "ref-index": 1,
            "motion": [
                {
                    "name": "Player",
                    "team": "McLaren",
                    "track-position": 2,
                    "index": 1,
                    "motion": {
                        "world-position": { "x": 0.0, "y": 0.0, "z": 0.0 },
                        "orientation": { "yaw": 0.0 }
                    }
                },
                {
                    "name": "Left",
                    "team": "Ferrari",
                    "track-position": 3,
                    "index": 2,
                    "motion": {
                        "world-position": { "x": -2.0, "y": 0.0, "z": 3.0 },
                        "orientation": { "yaw": FRAC_PI_2 }
                    }
                },
                {
                    "name": "Right",
                    "team": "Mercedes",
                    "track-position": 1,
                    "index": 3,
                    "motion": {
                        "world-position": { "x": 2.5, "y": 0.0, "z": 1.0 },
                        "orientation": { "yaw": PI }
                    }
                },
                {
                    "name": "Far",
                    "team": "Williams",
                    "track-position": 5,
                    "index": 4,
                    "motion": {
                        "world-position": { "x": 30.0, "y": 0.0, "z": 0.0 },
                        "orientation": { "yaw": 0.0 }
                    }
                }
            ]
        });

        let snapshot = TrackRadarSnapshot::from_stream_overlay_value(&value).expect("parse");
        let display = snapshot.display_state(25.0).expect("display state");

        assert_eq!(display.drivers.len(), 4);
        assert!(display.cars_nearby);
        assert!(display.left_alert);
        assert!(display.right_alert);

        let left_driver = display
            .drivers
            .iter()
            .find(|driver| driver.name == "Left")
            .expect("left driver");
        assert_eq!(left_driver.relative_x, -2.0);
        assert_eq!(left_driver.relative_z, 3.0);
        assert!((left_driver.relative_heading_deg - 90.0).abs() < 0.001);

        let far_driver = display
            .drivers
            .iter()
            .find(|driver| driver.name == "Far")
            .expect("far driver");
        assert!(far_driver.distance > 25.0);
    }

    #[test]
    fn rotates_world_positions_into_reference_driver_space() {
        let value = json!({
            "ref-index": 1,
            "motion": [
                {
                    "name": "Player",
                    "team": "McLaren",
                    "track-position": 2,
                    "index": 1,
                    "motion": {
                        "world-position": { "x": 10.0, "y": 0.0, "z": 10.0 },
                        "orientation": { "yaw": FRAC_PI_2 }
                    }
                },
                {
                    "name": "Ahead in world x",
                    "team": "Ferrari",
                    "track-position": 3,
                    "index": 2,
                    "motion": {
                        "world-position": { "x": 20.0, "y": 0.0, "z": 10.0 },
                        "orientation": { "yaw": FRAC_PI_2 }
                    }
                }
            ]
        });

        let snapshot = TrackRadarSnapshot::from_stream_overlay_value(&value).expect("parse");
        let display = snapshot.display_state(25.0).expect("display");
        let target = display
            .drivers
            .iter()
            .find(|driver| driver.index == 2)
            .expect("target");

        assert!(target.relative_x.abs() < 0.001);
        assert!((target.relative_z - 10.0).abs() < 0.001);
    }
}
