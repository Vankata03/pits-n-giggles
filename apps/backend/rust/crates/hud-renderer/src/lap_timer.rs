use serde_json::Value;

use crate::{FetchTelemetryError, TelemetryParseError, fetch_telemetry_info_value};

const DEFAULT_TIME: &str = "--:--.---";
const DEFAULT_DELTA: &str = "---";
const DEFAULT_SECTOR_STATUS: [i32; 3] = [-2, -2, -2];
const LAST_LAP_SECTOR_BAR_DURATION_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq)]
pub struct LapTimerCompletedLap {
    pub lap_time_ms: Option<u32>,
    pub sector_status: [i32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct LapTimerCurrentLap {
    pub lap_time_ms: Option<u32>,
    pub driver_status: String,
    pub sector_status: [i32; 3],
    pub delta_ms: Option<i32>,
    pub delta_sc_sec: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LapTimerEntry {
    pub index: usize,
    pub position: Option<u8>,
    pub is_player: bool,
    pub current_lap_number: Option<u8>,
    pub last_lap: LapTimerCompletedLap,
    pub best_lap: LapTimerCompletedLap,
    pub current_lap: LapTimerCurrentLap,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LapTimerSnapshot {
    pub session_type: String,
    pub session_uid: u64,
    pub safety_car_status: String,
    pub ref_driver_index: usize,
    pub table_entries: Vec<LapTimerEntry>,
}

impl LapTimerSnapshot {
    pub fn from_telemetry_info_value(value: &Value) -> Result<Self, TelemetryParseError> {
        let session_type = value
            .get("event-type")
            .and_then(Value::as_str)
            .unwrap_or("None")
            .to_string();
        let session_uid = value
            .get("session-uid")
            .and_then(Value::as_u64)
            .ok_or(TelemetryParseError::MissingField("session-uid"))?;
        let safety_car_status = value
            .get("safety-car-status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let is_spectating = value
            .get("is-spectating")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let spectator_car_index = value
            .get("spectator-car-index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok());
        let table_entries = value
            .get("table-entries")
            .and_then(Value::as_array)
            .ok_or(TelemetryParseError::MissingField("table-entries"))?
            .iter()
            .map(parse_table_entry)
            .collect::<Result<Vec<_>, _>>()?;

        let ref_driver_index = if is_spectating {
            let spectator_index = spectator_car_index
                .ok_or(TelemetryParseError::MissingField("spectator-car-index"))?;
            table_entries
                .iter()
                .find(|entry| entry.index == spectator_index)
                .map(|entry| entry.index)
                .ok_or(TelemetryParseError::InvalidField("spectator-car-index"))?
        } else {
            table_entries
                .iter()
                .find(|entry| entry.is_player)
                .map(|entry| entry.index)
                .ok_or(TelemetryParseError::InvalidField("table-entries"))?
        };

        Ok(Self {
            session_type,
            session_uid,
            safety_car_status,
            ref_driver_index,
            table_entries,
        })
    }

    pub fn reference_entry(&self) -> Result<&LapTimerEntry, TelemetryParseError> {
        self.table_entries
            .iter()
            .find(|entry| entry.index == self.ref_driver_index)
            .ok_or(TelemetryParseError::InvalidField("table-entries"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LapTimerDeltaState {
    Neutral,
    Good,
    Bad,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LapTimerDisplay {
    pub current_time: String,
    pub delta_time: String,
    pub last_time: String,
    pub best_time: String,
    pub estimated_time: String,
    pub current_sector_status: [i32; 3],
    pub last_sector_status: [i32; 3],
    pub best_sector_status: [i32; 3],
    pub delta_state: LapTimerDeltaState,
}

impl LapTimerDisplay {
    pub fn empty() -> Self {
        Self {
            current_time: DEFAULT_TIME.to_string(),
            delta_time: DEFAULT_DELTA.to_string(),
            last_time: DEFAULT_TIME.to_string(),
            best_time: DEFAULT_TIME.to_string(),
            estimated_time: DEFAULT_TIME.to_string(),
            current_sector_status: DEFAULT_SECTOR_STATUS,
            last_sector_status: DEFAULT_SECTOR_STATUS,
            best_sector_status: DEFAULT_SECTOR_STATUS,
            delta_state: LapTimerDeltaState::Neutral,
        }
    }
}

#[derive(Debug, Default)]
pub struct LapTimerController {
    session_uid: Option<u64>,
    last_lap_num: Option<u8>,
    show_last_lap_sector_bar_until_ms: Option<u64>,
}

impl LapTimerController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(
        &mut self,
        snapshot: &LapTimerSnapshot,
        now_ms: u64,
    ) -> Result<LapTimerDisplay, TelemetryParseError> {
        if self.session_uid != Some(snapshot.session_uid) {
            self.session_uid = Some(snapshot.session_uid);
            self.last_lap_num = None;
            self.show_last_lap_sector_bar_until_ms = None;
        }

        if self
            .show_last_lap_sector_bar_until_ms
            .is_some_and(|deadline| now_ms >= deadline)
        {
            self.show_last_lap_sector_bar_until_ms = None;
        }

        let entry = snapshot.reference_entry()?;
        if let (Some(previous_lap), Some(current_lap)) =
            (self.last_lap_num, entry.current_lap_number)
        {
            if previous_lap != current_lap {
                self.show_last_lap_sector_bar_until_ms =
                    Some(now_ms + LAST_LAP_SECTOR_BAR_DURATION_MS);
            }
        }

        let show_last_lap_sector_bar = self
            .show_last_lap_sector_bar_until_ms
            .is_some_and(|deadline| now_ms < deadline);

        let delta_ms = calculate_delta_ms(snapshot, entry);
        let display = LapTimerDisplay {
            current_time: current_time_label(snapshot, entry),
            delta_time: format_delta(delta_ms),
            last_time: format_lap_time(entry.last_lap.lap_time_ms),
            best_time: format_lap_time(entry.best_lap.lap_time_ms),
            estimated_time: estimated_time_label(snapshot, entry, delta_ms),
            current_sector_status: if show_last_lap_sector_bar {
                entry.last_lap.sector_status
            } else {
                entry.current_lap.sector_status
            },
            last_sector_status: entry.last_lap.sector_status,
            best_sector_status: entry.best_lap.sector_status,
            delta_state: delta_state(snapshot, delta_ms),
        };

        self.last_lap_num = entry.current_lap_number;
        Ok(display)
    }
}

pub fn fetch_lap_timer(base_url: &str) -> Result<LapTimerSnapshot, FetchTelemetryError> {
    let value = fetch_telemetry_info_value(base_url)?;
    LapTimerSnapshot::from_telemetry_info_value(&value).map_err(FetchTelemetryError::Parse)
}

fn parse_table_entry(value: &Value) -> Result<LapTimerEntry, TelemetryParseError> {
    let driver_info = value
        .get("driver-info")
        .ok_or(TelemetryParseError::MissingField("driver-info"))?;
    let lap_info = value
        .get("lap-info")
        .ok_or(TelemetryParseError::MissingField("lap-info"))?;
    let last_lap = lap_info
        .get("last-lap")
        .ok_or(TelemetryParseError::MissingField("last-lap"))?;
    let best_lap = lap_info
        .get("best-lap")
        .ok_or(TelemetryParseError::MissingField("best-lap"))?;
    let current_lap = lap_info
        .get("curr-lap")
        .ok_or(TelemetryParseError::MissingField("curr-lap"))?;

    Ok(LapTimerEntry {
        index: value_usize(driver_info, "index")?,
        position: value
            .pointer("/driver-info/position")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok()),
        is_player: driver_info
            .get("is-player")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        current_lap_number: lap_info
            .get("current-lap")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok()),
        last_lap: LapTimerCompletedLap {
            lap_time_ms: optional_u32(last_lap.get("lap-time-ms")),
            sector_status: sector_status_array(last_lap.get("sector-status")),
        },
        best_lap: LapTimerCompletedLap {
            lap_time_ms: optional_u32(best_lap.get("lap-time-ms")),
            sector_status: sector_status_array(best_lap.get("sector-status")),
        },
        current_lap: LapTimerCurrentLap {
            lap_time_ms: optional_u32(current_lap.get("lap-time-ms")),
            driver_status: current_lap
                .get("driver-status")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            sector_status: sector_status_array(current_lap.get("sector-status")),
            delta_ms: optional_i32(current_lap.get("delta-ms")),
            delta_sc_sec: current_lap
                .get("delta-sc-sec")
                .and_then(Value::as_f64)
                .map(|value| value as f32),
        },
    })
}

fn current_time_label(snapshot: &LapTimerSnapshot, entry: &LapTimerEntry) -> String {
    if is_race_type_session(&snapshot.session_type) {
        if is_safety_car(&snapshot.safety_car_status) {
            return if snapshot.safety_car_status == "VIRTUAL_SAFETY_CAR" {
                "VSC".to_string()
            } else {
                "SC".to_string()
            };
        }
        return format_lap_time(entry.current_lap.lap_time_ms);
    }

    if matches!(
        entry.current_lap.driver_status.as_str(),
        "FLYING_LAP" | "ON_TRACK"
    ) {
        format_lap_time(entry.current_lap.lap_time_ms)
    } else {
        entry.current_lap.driver_status.clone()
    }
}

fn calculate_delta_ms(snapshot: &LapTimerSnapshot, entry: &LapTimerEntry) -> Option<i32> {
    if is_safety_car(&snapshot.safety_car_status) {
        entry
            .current_lap
            .delta_sc_sec
            .map(|delta_sc| (delta_sc * 1000.0) as i32)
    } else {
        entry.current_lap.delta_ms
    }
}

fn delta_state(snapshot: &LapTimerSnapshot, delta_ms: Option<i32>) -> LapTimerDeltaState {
    let Some(delta_ms) = delta_ms else {
        return LapTimerDeltaState::Neutral;
    };
    let delta_sec = delta_ms as f64 / 1000.0;
    let is_good = (delta_sec < 0.0) != is_safety_car(&snapshot.safety_car_status);
    if is_good {
        LapTimerDeltaState::Good
    } else {
        LapTimerDeltaState::Bad
    }
}

fn format_delta(delta_ms: Option<i32>) -> String {
    let Some(delta_ms) = delta_ms else {
        return DEFAULT_DELTA.to_string();
    };
    format_signed_seconds(delta_ms as f64 / 1000.0, 3)
}

fn estimated_time_label(
    snapshot: &LapTimerSnapshot,
    entry: &LapTimerEntry,
    delta_ms: Option<i32>,
) -> String {
    let Some(best_ms) = entry.best_lap.lap_time_ms else {
        return DEFAULT_TIME.to_string();
    };
    let Some(delta_ms) = delta_ms else {
        return DEFAULT_TIME.to_string();
    };

    let estimated_ms = best_ms as i64 + i64::from(delta_ms);
    if estimated_ms <= 0 {
        return DEFAULT_TIME.to_string();
    }

    let mut estimated = format_lap_time(Some(estimated_ms as u32));
    if should_predict_position(&snapshot.session_type) {
        if let Some(position) = predicted_position(snapshot, estimated_ms as u32, delta_ms) {
            estimated.push_str(&format!(" | (P{position})"));
        }
    }

    estimated
}

fn predicted_position(snapshot: &LapTimerSnapshot, estimated_ms: u32, delta_ms: i32) -> Option<u8> {
    if delta_ms > 0 {
        return snapshot
            .table_entries
            .iter()
            .find(|entry| entry.index == snapshot.ref_driver_index)
            .and_then(|entry| entry.position);
    }

    let mut ranked = Vec::new();
    let mut has_other_laps = false;
    for entry in &snapshot.table_entries {
        if entry.index == snapshot.ref_driver_index {
            ranked.push((estimated_ms, true, entry.index));
        } else if let Some(best_ms) = entry.best_lap.lap_time_ms {
            has_other_laps = true;
            ranked.push((best_ms, false, entry.index));
        }
    }

    if !has_other_laps {
        return Some(1);
    }

    ranked.sort_by_key(|&(lap_ms, is_ref, _)| (lap_ms, is_ref));
    ranked
        .iter()
        .position(|&(_, _, index)| index == snapshot.ref_driver_index)
        .and_then(|index| u8::try_from(index + 1).ok())
}

fn should_predict_position(session_type: &str) -> bool {
    is_practice_session(session_type) || is_qualifying_session(session_type)
}

fn is_race_type_session(session_type: &str) -> bool {
    session_type.contains("Race")
}

fn is_practice_session(session_type: &str) -> bool {
    session_type == "Practice"
}

fn is_qualifying_session(session_type: &str) -> bool {
    session_type.contains("Qualifying") || session_type.contains("Shootout")
}

fn is_safety_car(status: &str) -> bool {
    matches!(status, "FULL_SAFETY_CAR" | "VIRTUAL_SAFETY_CAR")
}

fn format_lap_time(value: Option<u32>) -> String {
    let Some(milliseconds) = value else {
        return DEFAULT_TIME.to_string();
    };
    let total_seconds = milliseconds / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    let millis = milliseconds % 1000;
    format!("{minutes:02}:{seconds:02}.{millis:03}")
}

fn format_signed_seconds(value: f64, precision: usize) -> String {
    let normalized = if value.abs() < 1e-12 { 0.0 } else { value };
    if normalized >= 0.0 {
        format!("+{normalized:.precision$}")
    } else {
        format!("{normalized:.precision$}")
    }
}

fn sector_status_array(value: Option<&Value>) -> [i32; 3] {
    let Some(value) = value else {
        return DEFAULT_SECTOR_STATUS;
    };
    let Some(items) = value.as_array() else {
        return DEFAULT_SECTOR_STATUS;
    };
    if items.len() != 3 {
        return DEFAULT_SECTOR_STATUS;
    }

    let mut output = [0; 3];
    for (index, item) in items.iter().enumerate() {
        output[index] = item
            .as_i64()
            .and_then(|value| i32::try_from(value).ok())
            .unwrap_or(DEFAULT_SECTOR_STATUS[index]);
    }
    output
}

fn value_usize(value: &Value, key: &'static str) -> Result<usize, TelemetryParseError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(TelemetryParseError::MissingField(key))
}

fn optional_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn optional_i32(value: Option<&Value>) -> Option<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{LapTimerController, LapTimerDeltaState, LapTimerSnapshot};

    fn telemetry_value(
        session_type: &str,
        driver_status: &str,
        delta_ms: i32,
    ) -> serde_json::Value {
        json!({
            "event-type": session_type,
            "session-uid": 42,
            "safety-car-status": "NO_SAFETY_CAR",
            "is-spectating": false,
            "table-entries": [
                {
                    "driver-info": {
                        "position": 2,
                        "is-player": true,
                        "index": 7
                    },
                    "lap-info": {
                        "current-lap": 5,
                        "last-lap": {
                            "lap-time-ms": 91234,
                            "sector-status": [1, 1, 0]
                        },
                        "best-lap": {
                            "lap-time-ms": 90800,
                            "sector-status": [2, 1, 1]
                        },
                        "curr-lap": {
                            "lap-time-ms": 23123,
                            "driver-status": driver_status,
                            "sector-status": [1, -2, -2],
                            "delta-ms": delta_ms,
                            "delta-sc-sec": null
                        }
                    }
                },
                {
                    "driver-info": {
                        "position": 1,
                        "is-player": false,
                        "index": 3
                    },
                    "lap-info": {
                        "current-lap": 5,
                        "last-lap": {
                            "lap-time-ms": 90720,
                            "sector-status": [1, 2, 1]
                        },
                        "best-lap": {
                            "lap-time-ms": 90720,
                            "sector-status": [1, 2, 1]
                        },
                        "curr-lap": {
                            "lap-time-ms": 25000,
                            "driver-status": "FLYING_LAP",
                            "sector-status": [1, -2, -2],
                            "delta-ms": 120,
                            "delta-sc-sec": null
                        }
                    }
                }
            ]
        })
    }

    #[test]
    fn parses_reference_driver_from_player_flag() {
        let snapshot = LapTimerSnapshot::from_telemetry_info_value(&telemetry_value(
            "Qualifying",
            "FLYING_LAP",
            -150,
        ))
        .expect("parse telemetry");

        assert_eq!(snapshot.ref_driver_index, 7);
        assert_eq!(snapshot.table_entries.len(), 2);
        assert_eq!(snapshot.reference_entry().expect("ref").position, Some(2));
    }

    #[test]
    fn controller_formats_quali_display_and_predicted_position() {
        let snapshot = LapTimerSnapshot::from_telemetry_info_value(&telemetry_value(
            "Qualifying",
            "FLYING_LAP",
            -150,
        ))
        .expect("parse telemetry");
        let mut controller = LapTimerController::new();
        let display = controller.update(&snapshot, 0).expect("display");

        assert_eq!(display.current_time, "00:23.123");
        assert_eq!(display.last_time, "01:31.234");
        assert_eq!(display.best_time, "01:30.800");
        assert_eq!(display.delta_time, "-0.150");
        assert_eq!(display.delta_state, LapTimerDeltaState::Good);
        assert_eq!(display.estimated_time, "01:30.650 | (P1)");
    }

    #[test]
    fn controller_shows_safety_car_status_and_uses_sc_delta() {
        let value = json!({
            "event-type": "Race",
            "session-uid": 42,
            "safety-car-status": "VIRTUAL_SAFETY_CAR",
            "is-spectating": false,
            "table-entries": [
                {
                    "driver-info": {
                        "position": 1,
                        "is-player": true,
                        "index": 7
                    },
                    "lap-info": {
                        "current-lap": 12,
                        "last-lap": {
                            "lap-time-ms": 91234,
                            "sector-status": [1, 1, 1]
                        },
                        "best-lap": {
                            "lap-time-ms": 90500,
                            "sector-status": [2, 1, 1]
                        },
                        "curr-lap": {
                            "lap-time-ms": 11111,
                            "driver-status": "ON_TRACK",
                            "sector-status": [1, -2, -2],
                            "delta-ms": -250,
                            "delta-sc-sec": 0.321
                        }
                    }
                }
            ]
        });
        let snapshot = LapTimerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let mut controller = LapTimerController::new();
        let display = controller.update(&snapshot, 0).expect("display");

        assert_eq!(display.current_time, "VSC");
        assert_eq!(display.delta_time, "+0.321");
        assert_eq!(display.delta_state, LapTimerDeltaState::Good);
        assert_eq!(display.estimated_time, "01:30.821");
    }

    #[test]
    fn controller_uses_last_lap_sector_bar_for_five_seconds_after_lap_change() {
        let value = telemetry_value("Race", "ON_TRACK", 120);
        let snapshot = LapTimerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let mut controller = LapTimerController::new();

        let _ = controller.update(&snapshot, 0).expect("first display");

        let changed_value = json!({
            "event-type": "Race",
            "session-uid": 42,
            "safety-car-status": "NO_SAFETY_CAR",
            "is-spectating": false,
            "table-entries": [
                {
                    "driver-info": {
                        "position": 2,
                        "is-player": true,
                        "index": 7
                    },
                    "lap-info": {
                        "current-lap": 6,
                        "last-lap": {
                            "lap-time-ms": 90321,
                            "sector-status": [2, 1, 1]
                        },
                        "best-lap": {
                            "lap-time-ms": 90321,
                            "sector-status": [2, 1, 1]
                        },
                        "curr-lap": {
                            "lap-time-ms": 1234,
                            "driver-status": "ON_TRACK",
                            "sector-status": [0, -2, -2],
                            "delta-ms": 40,
                            "delta-sc-sec": null
                        }
                    }
                }
            ]
        });
        let changed_snapshot =
            LapTimerSnapshot::from_telemetry_info_value(&changed_value).expect("parse changed");

        let display = controller
            .update(&changed_snapshot, 1_000)
            .expect("changed");
        assert_eq!(display.current_sector_status, [2, 1, 1]);

        let display = controller
            .update(&changed_snapshot, 6_001)
            .expect("expired");
        assert_eq!(display.current_sector_status, [0, -2, -2]);
    }
}
