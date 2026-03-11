use hud_renderer::fetch_input_telemetry;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("PNG_BACKEND_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:4768".to_string());

    let telemetry = fetch_input_telemetry(&base_url)?;

    println!("Rust HUD prototype input telemetry");
    println!("  source: {base_url}/stream-overlay-info");
    println!("  throttle: {:.1}%", telemetry.throttle);
    println!("  brake: {:.1}%", telemetry.brake);
    println!("  steering: {:.1}%", telemetry.steering);
    println!("  rev lights: {}%", telemetry.rev_lights_percentage);

    Ok(())
}
