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
from lib.config import TIMING_TOWER_OVERLAY_ID, OverlayPosition


class TimingTowerOverlay(RustNativeOverlay):
    OVERLAY_ID = TIMING_TOWER_OVERLAY_ID
    BASE_WINDOW_WIDTH = 1
    BASE_WINDOW_HEIGHT = 1
    RUST_BINARY_STEM = "timing_tower"
    MAX_SUPPORTED_CARS = 22

    def __init__(
        self,
        config: OverlayPosition,
        logger: logging.Logger,
        locked: bool,
        opacity: int,
        scale_factor: float,
        num_adjacent_cars: int,
        windowed_overlay: bool,
        show_team_logos: bool,
        show_tyre_info: bool,
        show_deltas: bool,
        show_ers_drs_info: bool,
        show_pens: bool,
        show_tl_warns: bool,
        render_interval_ms: int,
        fetch_interval_ms: int,
        base_url: Optional[str] = None,
    ) -> None:
        assert render_interval_ms > 0
        assert fetch_interval_ms > 0
        self._num_adjacent_cars = max(0, min(num_adjacent_cars, self.MAX_SUPPORTED_CARS))
        self._show_team_logos = show_team_logos
        self._show_tyre_info = show_tyre_info
        self._show_deltas = show_deltas
        self._show_ers_drs_info = show_ers_drs_info
        self._show_pens = show_pens
        self._show_tl_warns = show_tl_warns
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

    def _base_window_width(self) -> int:
        width = 40 + 160
        if self._show_team_logos:
            width += 30
        if self._show_deltas:
            width += 90
        if self._show_tyre_info:
            width += 75
        if self._show_ers_drs_info:
            width += 75 if self._show_pens else 85
        if self._show_pens:
            width += 80
        return width + 20

    def _base_window_height(self) -> int:
        total_rows = min(((self._num_adjacent_cars * 2) + 1), self.MAX_SUPPORTED_CARS)
        return 40 + (32 * total_rows) + 25

    def _build_overlay_args(self) -> list[str]:
        overlay_args = [
            "--title",
            self._title,
            "--render-interval-ms",
            str(self._render_interval_ms),
            "--fetch-interval-ms",
            str(self._fetch_interval_ms),
            "--num-adjacent-cars",
            str(self._num_adjacent_cars),
            "--show-team-logos",
            str(self._show_team_logos).lower(),
            "--show-tyre-info",
            str(self._show_tyre_info).lower(),
            "--show-deltas",
            str(self._show_deltas).lower(),
            "--show-ers-drs-info",
            str(self._show_ers_drs_info).lower(),
            "--show-pens",
            str(self._show_pens).lower(),
            "--show-tl-warns",
            str(self._show_tl_warns).lower(),
            "--x",
            str(self.config.x),
            "--y",
            str(self.config.y),
            "--width",
            str(max(1, round(self._base_window_width() * self.scale_factor))),
            "--height",
            str(max(1, round(self._base_window_height() * self.scale_factor))),
        ]
        if self._backend_url:
            overlay_args.append(self._backend_url)
        return overlay_args
