# HUD UI Optimization Notes

This note captures what the current HUD uses today, why it can lag under high-frequency updates, and which Rust UI stack is the best fit if the HUD is migrated away from Qt/QML.

## Current HUD stack

The current HUD is a **Python + PySide6 + QML** subsystem.

- `apps/hud/ui/infra/overlays_mgr.py`
  - creates the `QApplication`
  - registers each overlay window
  - rate-limits some update paths
- `apps/hud/ui/overlays/base/base_qml.py`
  - creates a `QQmlApplicationEngine` per overlay
  - loads a `QQuickWindow`
  - uses `QTimer` for frame updates when an overlay is time-driven
  - uses `QPropertyAnimation` for fade in/out
- `apps/hud/ui/infra/window_mgr.py`
  - pushes overlay commands through Qt signals/slots
  - uses queued calls to fan out updates safely onto the GUI thread

Today the HUD overlays are mostly QML files under `apps/hud/ui/overlays/**`:

- `lap_timer/lap_timer_overlay.qml`
- `timing_tower/timing_tower.qml`
- `input_telemetry/input_telemetry.qml`
- `track_radar/track_radar.qml`
- `mfd/mfd.qml`

## Why this stack can lag

The current implementation is flexible, but it has a few performance costs that are easy to feel in a racing HUD:

1. **Python -> Qt -> QML crossings on live updates**
   - High-frequency overlays push data from Python into QML with `QMetaObject.invokeMethod(...)`.
   - That adds overhead on every frame/update before the renderer even draws anything.

2. **Multiple QML engines/windows**
   - Each QML overlay owns its own `QQmlApplicationEngine` and `QQuickWindow`.
   - That is convenient architecturally, but it is heavier than a single native renderer drawing multiple overlay panels.

3. **Transparent always-on-top windows**
   - The HUD relies on frameless transparent windows, which are often one of the more expensive desktop rendering paths.

4. **Timer-driven realtime overlays**
   - `InputTelemetryOverlay` and `TrackRadarOverlay` are explicitly driven by refresh timers.
   - Those are the most likely places to show jitter or dropped frames first.

5. **Optional software rendering path**
   - In `apps/hud/ui/infra/overlays_mgr.py`, enabling `settings.Display.use_cpu_acceleration` sets `QT_QUICK_BACKEND=software`.
   - Software rendering is useful as a fallback, but it is the wrong direction if the goal is lower HUD latency.

## Best Rust alternative

The best Rust fit for this HUD is:

**`egui` + `wgpu` + `winit`**

### Why this is the best fit

- **Fast for HUD-style drawing**
  - The HUD mostly renders text, bars, icons, tables, and simple shapes.
  - Immediate-mode UI works well for that kind of frequently refreshed telemetry view.

- **GPU-backed**
  - `wgpu` avoids the software-rendering pitfall and is a much better fit for smooth, frequent redraws.

- **Simpler update path**
  - The data can stay inside Rust once it reaches the HUD renderer, instead of bouncing through Python -> Qt -> QML bindings.

- **Good match for the repo**
  - The project already ships a Rust backend under `apps/backend/rust/`.
  - Adding a Rust HUD renderer follows the existing mixed Python/Rust architecture instead of introducing a totally new ecosystem.

### First overlay to migrate

If the migration starts small, the best first target is:

**`InputTelemetryOverlay`**

Why:

- it is already one of the high-frequency overlays
- it has a very small data surface: throttle, brake, steering, revs
- it is visually simple, so it is the safest place to measure whether the lag is mostly in Qt/QML

After that, the next best candidate is **`TrackRadarOverlay`**, because it also updates frequently and would benefit from GPU-backed drawing.

## Recommended migration path

1. **Keep the Python launcher and settings system unchanged**
   - no need to rewrite startup/config just to test HUD performance

2. **Add a separate Rust HUD prototype**
   - start with only the input telemetry overlay
   - feed it the same live telemetry data the Python HUD already uses
   - this repo now includes a small scaffold crate at `apps/backend/rust/crates/hud-renderer/`
   - the first example intentionally stops at data fetch + parse so the data path can be validated before any renderer work starts

   ```bash
   cargo run --manifest-path apps/backend/rust/Cargo.toml -p hud-renderer --example basic_input_telemetry
   ```

   To bring up a visible standalone prototype window without changing the launcher:

   ```bash
   cargo run --manifest-path apps/backend/rust/Cargo.toml -p hud-renderer -- --sample
   ```

   The next migrated prototype now has its own standalone radar binary:

   ```bash
   cargo run --manifest-path apps/backend/rust/Cargo.toml -p hud-renderer --bin track_radar -- --sample
   ```

   The event-driven lap timer prototype is also available as a standalone binary:

   ```bash
   cargo run --manifest-path apps/backend/rust/Cargo.toml -p hud-renderer --bin lap_timer -- --sample
   ```

3. **Compare frame pacing**
   - check update smoothness and CPU/GPU usage against the current PySide6/QML overlay

4. **Only migrate the rest if the prototype is clearly better**
   - next target: track radar
   - keep event-driven overlays like lap timer in Python longer if they are already smooth enough

## Secondary Rust option

If a more declarative UI is preferred over immediate-mode rendering, the next-best Rust option is **Slint**.

Slint is worth considering if:

- keeping a declarative UI language matters
- you want a smaller mental jump from QML concepts
- the team prefers a UI-first workflow over a custom renderer

That said, for a **performance-sensitive realtime HUD**, `egui` + `wgpu` is the stronger default recommendation.
