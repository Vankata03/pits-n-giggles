# MIT License
#
# Copyright (c) [2026] [Ashwin Natarajan]
#
# Permission is hereby granted, free of charge, to any person obtaining a copy
# of this software and associated documentation files (the "Software"), to deal
# in the Software without restriction, including without limitation the rights
# to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
# copies of the Software, and to permit persons to whom the Software is
# furnished to do so, subject to the following conditions:
#
# The above copyright notice and this permission notice shall be included in all
# copies or substantial portions of the Software.
#
# THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
# IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
# FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
# AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
# LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
# OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
# SOFTWARE.

import logging
from typing import Optional

from apps.hud.ui.overlays.base import RustNativeOverlay
from lib.config import LAP_TIMER_OVERLAY_ID, OverlayPosition


class LapTimerOverlay(RustNativeOverlay):
    OVERLAY_ID = LAP_TIMER_OVERLAY_ID
    BASE_WINDOW_WIDTH = 280
    BASE_WINDOW_HEIGHT = 180
    RUST_BINARY_STEM = "lap_timer"

    def __init__(
        self,
        config: OverlayPosition,
        logger: logging.Logger,
        locked: bool,
        opacity: int,
        scale_factor: float,
        windowed_overlay: bool,
        render_interval_ms: int,
        fetch_interval_ms: int,
        base_url: Optional[str] = None,
    ) -> None:
        assert render_interval_ms > 0
        assert fetch_interval_ms > 0
        self._render_interval_ms = render_interval_ms
        self._fetch_interval_ms = fetch_interval_ms
        super().__init__(
            config,
            logger,
            locked,
            opacity,
            scale_factor,
            windowed_overlay,
            base_url=base_url,
        )

    def _build_overlay_args(self) -> list[str]:
        overlay_args = [
            "--title",
            self._title,
            "--render-interval-ms",
            str(self._render_interval_ms),
            "--fetch-interval-ms",
            str(self._fetch_interval_ms),
            "--x",
            str(self.config.x),
            "--y",
            str(self.config.y),
            "--width",
            str(self._scaled_window_width),
            "--height",
            str(self._scaled_window_height),
        ]
        if self._backend_url:
            overlay_args.append(self._backend_url)
        return overlay_args
