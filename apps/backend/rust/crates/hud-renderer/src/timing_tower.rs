use serde_json::Value;

use crate::{FetchTelemetryError, TelemetryParseError, fetch_telemetry_info_value};

const DEFAULT_SESSION_INFO: &str = "-- / --";
const TIME_TRIAL_NOT_SUPPORTED: &str = "TIME TRIAL NOT YET SUPPORTED";
const GENERIC_TIMING_TOWER_ERROR: &str = "ERROR: Please check the logs";

#[derive(Debug, Clone, PartialEq)]
pub struct TimingTowerEntry {
    pub index: usize,
    pub position: u8,
    pub is_player: bool,
    pub name: String,
    pub team: String,
    pub telemetry_public: bool,
    pub drs: bool,
    pub is_pitting: bool,
    pub dnf_status: Option<String>,
    pub delta_to_car_in_front_ms: i32,
    pub best_lap_time_ms: Option<u32>,
    pub visual_tyre_compound: String,
    pub tyre_age_laps: u32,
    pub max_tyre_wear_percent: Option<u8>,
    pub ers_percent: Option<f32>,
    pub ers_mode: String,
    pub num_dt: u8,
    pub num_sg: u8,
    pub time_penalties_sec: u32,
    pub corner_cutting_warnings: u8,
    pub relative_delta_ms: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimingTowerSnapshot {
    pub session_type: String,
    pub current_lap: Option<u8>,
    pub total_laps: Option<u8>,
    pub session_time_left_sec: Option<u32>,
    pub ref_driver_index: usize,
    pub table_entries: Vec<TimingTowerEntry>,
}

impl TimingTowerSnapshot {
    pub fn from_telemetry_info_value(value: &Value) -> Result<Self, TelemetryParseError> {
        let session_type = value
            .get("event-type")
            .and_then(Value::as_str)
            .unwrap_or("None")
            .to_string();
        let current_lap = optional_u8(value.get("current-lap"));
        let total_laps = optional_u8(value.get("total-laps"));
        let session_time_left_sec = optional_u32(value.get("session-time-left"));
        let is_spectating = value
            .get("is-spectating")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let spectator_car_index = value
            .get("spectator-car-index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok());

        let table_entries_value = value
            .get("table-entries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let mut table_entries = table_entries_value
            .iter()
            .map(parse_table_entry)
            .collect::<Result<Vec<_>, _>>()?;
        table_entries.sort_by_key(|entry| entry.position);

        let ref_driver_index = if table_entries.is_empty() {
            0
        } else if is_spectating {
            spectator_car_index
                .and_then(|spectator_index| {
                    table_entries
                        .iter()
                        .find(|entry| entry.index == spectator_index)
                        .map(|entry| entry.index)
                })
                .or_else(|| table_entries.first().map(|entry| entry.index))
                .unwrap_or(0)
        } else {
            table_entries
                .iter()
                .find(|entry| entry.is_player)
                .map(|entry| entry.index)
                .or_else(|| table_entries.first().map(|entry| entry.index))
                .unwrap_or(0)
        };

        Ok(Self {
            session_type,
            current_lap,
            total_laps,
            session_time_left_sec,
            ref_driver_index,
            table_entries,
        })
    }

    pub fn reference_entry(&self) -> Option<&TimingTowerEntry> {
        self.table_entries
            .iter()
            .find(|entry| entry.index == self.ref_driver_index)
    }

    pub fn display_state(&self, num_adjacent_cars: usize) -> TimingTowerDisplay {
        if self.table_entries.is_empty() {
            return TimingTowerDisplay::empty();
        }

        if is_tt_session(&self.session_type) {
            return TimingTowerDisplay {
                session_info: DEFAULT_SESSION_INFO.to_string(),
                error_message: Some(TIME_TRIAL_NOT_SUPPORTED.to_string()),
                rows: Vec::new(),
                ref_driver_index: self.ref_driver_index,
            };
        }

        let Some(reference_entry) = self.reference_entry() else {
            return TimingTowerDisplay {
                session_info: session_info_label(self),
                error_message: Some(GENERIC_TIMING_TOWER_ERROR.to_string()),
                rows: Vec::new(),
                ref_driver_index: self.ref_driver_index,
            };
        };

        let Some((lower_bound, upper_bound)) = adjacent_positions(
            usize::from(reference_entry.position),
            self.table_entries.len(),
            num_adjacent_cars,
        ) else {
            return TimingTowerDisplay {
                session_info: session_info_label(self),
                error_message: Some(GENERIC_TIMING_TOWER_ERROR.to_string()),
                rows: Vec::new(),
                ref_driver_index: self.ref_driver_index,
            };
        };

        let reference_best_lap_ms = reference_entry.best_lap_time_ms;
        let mut rows = self.table_entries[(lower_bound - 1)..upper_bound].to_vec();
        if is_race_type_session(&self.session_type) {
            insert_relative_deltas_race(&mut rows, self.ref_driver_index);
        } else {
            insert_relative_deltas_non_race(&mut rows, reference_best_lap_ms);
        }

        TimingTowerDisplay {
            session_info: session_info_label(self),
            error_message: None,
            rows,
            ref_driver_index: self.ref_driver_index,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimingTowerDisplay {
    pub session_info: String,
    pub error_message: Option<String>,
    pub rows: Vec<TimingTowerEntry>,
    pub ref_driver_index: usize,
}

impl TimingTowerDisplay {
    pub fn empty() -> Self {
        Self {
            session_info: DEFAULT_SESSION_INFO.to_string(),
            error_message: None,
            rows: Vec::new(),
            ref_driver_index: 0,
        }
    }
}

pub fn fetch_timing_tower(base_url: &str) -> Result<TimingTowerSnapshot, FetchTelemetryError> {
    let value = fetch_telemetry_info_value(base_url)?;
    TimingTowerSnapshot::from_telemetry_info_value(&value).map_err(FetchTelemetryError::Parse)
}

fn parse_table_entry(value: &Value) -> Result<TimingTowerEntry, TelemetryParseError> {
    let driver_info = value
        .get("driver-info")
        .ok_or(TelemetryParseError::MissingField("driver-info"))?;
    let delta_info = value
        .get("delta-info")
        .ok_or(TelemetryParseError::MissingField("delta-info"))?;
    let tyre_info = value
        .get("tyre-info")
        .ok_or(TelemetryParseError::MissingField("tyre-info"))?;
    let ers_info = value
        .get("ers-info")
        .ok_or(TelemetryParseError::MissingField("ers-info"))?;
    let warns_pens_info = value
        .get("warns-pens-info")
        .ok_or(TelemetryParseError::MissingField("warns-pens-info"))?;
    let lap_info = value
        .get("lap-info")
        .ok_or(TelemetryParseError::MissingField("lap-info"))?;

    Ok(TimingTowerEntry {
        index: value_usize(driver_info, "index")?,
        position: value_u8(driver_info, "position")?,
        is_player: driver_info
            .get("is-player")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        name: driver_info
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_string(),
        team: driver_info
            .get("team")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_string(),
        telemetry_public: driver_info
            .get("telemetry-setting")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "Public"),
        drs: driver_info
            .get("drs")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_pitting: driver_info
            .get("is-pitting")
            .or_else(|| driver_info.get("is_pitting"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        dnf_status: driver_info
            .get("dnf-status")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        delta_to_car_in_front_ms: optional_i32(delta_info.get("delta-to-car-in-front"))
            .unwrap_or(0),
        best_lap_time_ms: lap_info
            .get("best-lap")
            .and_then(|value| value.get("lap-time-ms"))
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok()),
        visual_tyre_compound: tyre_info
            .get("visual-tyre-compound")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_string(),
        tyre_age_laps: tyre_info
            .get("tyre-age")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0),
        max_tyre_wear_percent: max_tyre_wear_percent(tyre_info.get("current-wear")),
        ers_percent: optional_f32(ers_info.get("ers-percent-float")),
        ers_mode: ers_info
            .get("ers-mode")
            .and_then(Value::as_str)
            .unwrap_or("None")
            .to_string(),
        num_dt: optional_u8(warns_pens_info.get("num-dt")).unwrap_or(0),
        num_sg: optional_u8(warns_pens_info.get("num-sg")).unwrap_or(0),
        time_penalties_sec: optional_u32(warns_pens_info.get("time-penalties")).unwrap_or(0),
        corner_cutting_warnings: optional_u8(warns_pens_info.get("corner-cutting-warnings"))
            .unwrap_or(0),
        relative_delta_ms: None,
    })
}

fn insert_relative_deltas_race(rows: &mut [TimingTowerEntry], ref_driver_index: usize) {
    let Some(ref_position) = rows.iter().position(|row| row.index == ref_driver_index) else {
        return;
    };

    for index in 0..rows.len() {
        if index == ref_position {
            rows[index].relative_delta_ms = Some(0);
            continue;
        }

        let total_delta = if index < ref_position {
            -rows[index + 1..=ref_position]
                .iter()
                .map(|row| row.delta_to_car_in_front_ms)
                .sum::<i32>()
        } else {
            rows[ref_position + 1..=index]
                .iter()
                .map(|row| row.delta_to_car_in_front_ms)
                .sum::<i32>()
        };
        rows[index].relative_delta_ms = Some(total_delta);
    }
}

fn insert_relative_deltas_non_race(
    rows: &mut [TimingTowerEntry],
    ref_best_lap_time_ms: Option<u32>,
) {
    for row in rows {
        row.relative_delta_ms = match (row.best_lap_time_ms, ref_best_lap_time_ms) {
            (Some(best_lap_ms), Some(ref_best_lap_ms)) => {
                let best_lap_ms = i64::from(best_lap_ms);
                let ref_best_lap_ms = i64::from(ref_best_lap_ms);
                i32::try_from(best_lap_ms - ref_best_lap_ms).ok()
            }
            _ => None,
        };
    }
}

fn session_info_label(snapshot: &TimingTowerSnapshot) -> String {
    if snapshot.session_type.is_empty() || snapshot.session_type == "None" {
        return "----".to_string();
    }

    if is_race_type_session(&snapshot.session_type) {
        return format!(
            "{}    |    LAP {} / {}",
            snapshot.session_type.to_uppercase(),
            snapshot.current_lap.unwrap_or(0),
            snapshot.total_laps.unwrap_or(0),
        );
    }

    let time_left_sec = snapshot.session_time_left_sec.unwrap_or(0);
    let minutes = time_left_sec / 60;
    let seconds = time_left_sec % 60;
    format!(
        "{}    |    TIME: {minutes:02}:{seconds:02}",
        snapshot.session_type.to_uppercase(),
    )
}

fn adjacent_positions(
    position: usize,
    total_cars: usize,
    num_adjacent_cars: usize,
) -> Option<(usize, usize)> {
    if position == 0 || position > total_cars {
        return None;
    }

    let mut lower_bound = position.saturating_sub(num_adjacent_cars);
    let mut upper_bound = position + num_adjacent_cars;

    if lower_bound == 0 {
        let shift = 1 - lower_bound;
        lower_bound = 1;
        upper_bound = (upper_bound + shift).min(total_cars);
    }

    if upper_bound > total_cars {
        let shift = upper_bound - total_cars;
        upper_bound = total_cars;
        lower_bound = lower_bound.saturating_sub(shift).max(1);
    }

    Some((lower_bound, upper_bound))
}

fn is_race_type_session(session_type: &str) -> bool {
    session_type.contains("Race")
}

fn is_tt_session(session_type: &str) -> bool {
    session_type == "Time Trial"
}

fn value_usize(value: &Value, key: &'static str) -> Result<usize, TelemetryParseError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(TelemetryParseError::MissingField(key))
}

fn value_u8(value: &Value, key: &'static str) -> Result<u8, TelemetryParseError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or(TelemetryParseError::MissingField(key))
}

fn optional_u8(value: Option<&Value>) -> Option<u8> {
    value
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
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

fn optional_f32(value: Option<&Value>) -> Option<f32> {
    value.and_then(Value::as_f64).map(|value| value as f32)
}

fn max_tyre_wear_percent(value: Option<&Value>) -> Option<u8> {
    if let Some(items) = value.and_then(Value::as_array) {
        let max_value = items.iter().filter_map(Value::as_f64).reduce(f64::max)?;
        return Some(normalize_tyre_wear_percent(max_value));
    }

    let Some(map) = value.and_then(Value::as_object) else {
        return None;
    };
    let max_value = map.values().filter_map(Value::as_f64).reduce(f64::max)?;
    Some(normalize_tyre_wear_percent(max_value))
}

fn normalize_tyre_wear_percent(value: f64) -> u8 {
    let percent = if value <= 1.5 { value * 100.0 } else { value };
    percent.round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{TimingTowerDisplay, TimingTowerSnapshot};

    #[test]
    fn race_display_uses_player_reference_and_relative_gaps() {
        let value = json!({
            "event-type": "Race",
            "current-lap": 12,
            "total-laps": 57,
            "session-time-left": 1800,
            "is-spectating": false,
            "table-entries": [
                table_entry(3, 1, false, "Leader", 0, Some(90_000)),
                table_entry(7, 2, true, "Player", 820, Some(90_400)),
                table_entry(4, 3, false, "Behind", 500, Some(90_950))
            ]
        });

        let snapshot = TimingTowerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let display = snapshot.display_state(1);

        assert_eq!(snapshot.ref_driver_index, 7);
        assert_eq!(display.session_info, "RACE    |    LAP 12 / 57");
        assert_eq!(display.rows.len(), 3);
        assert_eq!(display.rows[0].relative_delta_ms, Some(-820));
        assert_eq!(display.rows[1].relative_delta_ms, Some(0));
        assert_eq!(display.rows[2].relative_delta_ms, Some(500));
    }

    #[test]
    fn non_race_display_uses_best_lap_deltas() {
        let value = json!({
            "event-type": "Qualifying",
            "current-lap": 5,
            "total-laps": 0,
            "session-time-left": 321,
            "is-spectating": false,
            "table-entries": [
                table_entry(3, 1, false, "Fast", 0, Some(89_800)),
                table_entry(7, 2, true, "Player", 0, Some(90_000)),
                table_entry(4, 3, false, "Slow", 0, Some(90_550))
            ]
        });

        let snapshot = TimingTowerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let display = snapshot.display_state(1);

        assert_eq!(display.session_info, "QUALIFYING    |    TIME: 05:21");
        assert_eq!(display.rows[0].relative_delta_ms, Some(-200));
        assert_eq!(display.rows[1].relative_delta_ms, Some(0));
        assert_eq!(display.rows[2].relative_delta_ms, Some(550));
    }

    #[test]
    fn spectating_uses_spectator_car_index_as_reference() {
        let value = json!({
            "event-type": "Race",
            "current-lap": 1,
            "total-laps": 5,
            "session-time-left": 300,
            "is-spectating": true,
            "spectator-car-index": 4,
            "table-entries": [
                table_entry(3, 1, false, "Leader", 0, Some(90_000)),
                table_entry(4, 2, false, "Watched", 900, Some(90_800)),
                table_entry(7, 3, true, "Player", 700, Some(91_200))
            ]
        });

        let snapshot = TimingTowerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let display = snapshot.display_state(1);

        assert_eq!(snapshot.ref_driver_index, 4);
        assert_eq!(display.rows[0].relative_delta_ms, Some(-900));
        assert_eq!(display.rows[1].relative_delta_ms, Some(0));
        assert_eq!(display.rows[2].relative_delta_ms, Some(700));
    }

    #[test]
    fn tolerates_empty_payload_before_telemetry_arrives() {
        let value = json!({
            "event-type": null,
            "current-lap": null,
            "total-laps": null,
            "session-time-left": null,
            "is-spectating": false,
            "table-entries": []
        });

        let snapshot = TimingTowerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let display = snapshot.display_state(2);

        assert_eq!(snapshot.ref_driver_index, 0);
        assert!(snapshot.table_entries.is_empty());
        assert_eq!(display, TimingTowerDisplay::empty());
    }

    #[test]
    fn time_trial_returns_unsupported_error_display() {
        let value = json!({
            "event-type": "Time Trial",
            "current-lap": 1,
            "total-laps": 0,
            "session-time-left": 600,
            "is-spectating": false,
            "table-entries": [
                table_entry(7, 1, true, "Player", 0, Some(90_000))
            ]
        });

        let snapshot = TimingTowerSnapshot::from_telemetry_info_value(&value).expect("parse");
        let display = snapshot.display_state(2);

        assert_eq!(display.session_info, "-- / --");
        assert_eq!(
            display.error_message.as_deref(),
            Some("TIME TRIAL NOT YET SUPPORTED")
        );
        assert!(display.rows.is_empty());
    }

    fn table_entry(
        index: usize,
        position: u8,
        is_player: bool,
        name: &str,
        delta_to_car_in_front: i32,
        best_lap_ms: Option<u32>,
    ) -> Value {
        json!({
            "driver-info": {
                "index": index,
                "position": position,
                "is-player": is_player,
                "name": name,
                "team": "Ferrari",
                "telemetry-setting": "Public",
                "drs": position % 2 == 0,
                "is-pitting": false,
                "dnf-status": null
            },
            "delta-info": {
                "delta-to-car-in-front": delta_to_car_in_front
            },
            "lap-info": {
                "best-lap": {
                    "lap-time-ms": best_lap_ms
                }
            },
            "tyre-info": {
                "visual-tyre-compound": "Soft",
                "tyre-age": 5,
                "current-wear": [0.12, 0.18, 0.14, 0.16]
            },
            "ers-info": {
                "ers-percent-float": 62.0,
                "ers-mode": "Hotlap"
            },
            "warns-pens-info": {
                "corner-cutting-warnings": 1,
                "num-dt": 0,
                "num-sg": 0,
                "time-penalties": 0
            }
        })
    }
}
