use f1_types::{F1PacketType, PacketEventData, PacketHeader};
use serde_json::json;

fn assert_f64_close(actual: f64, expected: f64) {
    let delta = (actual - expected).abs();
    assert!(
        delta <= 1e-6,
        "numeric mismatch: actual={actual} expected={expected} delta={delta}"
    );
}

fn sample_header(packet_format: u16) -> PacketHeader {
    PacketHeader::from_values(
        packet_format,
        if packet_format >= 2025 { 25 } else { 24 },
        1,
        2,
        1,
        F1PacketType::Event,
        333,
        19.0,
        6,
        7,
        0,
        255,
    )
}

#[test]
fn event_packet_parses_session_started() {
    let packet = PacketEventData::parse(
        sample_header(2024),
        b"SSTA\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
    )
    .expect("parse");

    let value = serde_json::to_value(packet).expect("serialize");
    assert_eq!(
        value,
        json!({
            "event-string-code": "SSTA",
            "event-details": null
        })
    );
}

#[test]
fn event_packet_parses_fastest_lap_2024() {
    let packet = PacketEventData::parse(
        sample_header(2024),
        b"FTLP\x04\x11X\xdcB\x00\x00\x00\x00\x00\x00\x00",
    )
    .expect("parse");

    let value = serde_json::to_value(packet).expect("serialize");
    assert_eq!(value["event-string-code"], json!("FTLP"));
    assert_eq!(value["event-details"]["vehicle-idx"], json!(4));
    assert_f64_close(
        value["event-details"]["lap-time"]
            .as_f64()
            .expect("lap-time f64"),
        110.17200469970703,
    );
}

#[test]
fn event_packet_parses_fastest_lap_2025() {
    let packet = PacketEventData::parse(
        sample_header(2025),
        b"FTLP\x07\x14.\x89B\x00\x00\x00\x02\x1amP",
    )
    .expect("parse");

    let value = serde_json::to_value(packet).expect("serialize");
    assert_eq!(value["event-string-code"], json!("FTLP"));
    assert_eq!(value["event-details"]["vehicle-idx"], json!(7));
    assert_f64_close(
        value["event-details"]["lap-time"]
            .as_f64()
            .expect("lap-time f64"),
        68.58999633789062,
    );
}
