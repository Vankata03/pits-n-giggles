# MIT License
#
# Copyright (c) [2025] [Ashwin Natarajan]
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

import ctypes
import logging
import os
import subprocess
import threading
import time
from pathlib import Path
from typing import Optional, final

import psutil
from PySide6.QtCore import QObject
from PySide6.QtGui import QIcon

from apps.hud.ui.overlays.base import BaseOverlay
from lib.config import INPUT_TELEMETRY_OVERLAY_ID, OverlayPosition


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", ctypes.c_long),
        ("top", ctypes.c_long),
        ("right", ctypes.c_long),
        ("bottom", ctypes.c_long),
    ]


class InputTelemetryOverlay(BaseOverlay, QObject):
    OVERLAY_ID = INPUT_TELEMETRY_OVERLAY_ID

    BASE_WINDOW_WIDTH = 480
    BASE_WINDOW_HEIGHT = 160
    FETCH_INTERVAL_MS = 100
    STARTUP_TIMEOUT_SEC = 45.0
    HWND_TOPMOST = -1
    SWP_NOSIZE = 0x0001
    SWP_NOMOVE = 0x0002
    SWP_NOACTIVATE = 0x0010
    SWP_SHOWWINDOW = 0x0040
    SW_HIDE = 0
    SW_SHOWNA = 8
    GWL_EXSTYLE = -20
    WS_EX_LAYERED = 0x00080000
    WS_EX_TRANSPARENT = 0x00000020
    LWA_ALPHA = 0x00000002

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
        window_duration_sec: float,
        base_url: Optional[str] = None,
    ) -> None:
        self._backend_url = base_url
        assert render_interval_ms > 0
        assert fetch_interval_ms > 0
        self._render_interval_ms = render_interval_ms
        self._fetch_interval_ms = fetch_interval_ms
        self._window_duration_sec = window_duration_sec
        self._title = f"png-rust-input-telemetry-{os.getpid()}"
        self._process: Optional[subprocess.Popen] = None
        self._hwnd: Optional[int] = None
        self._output_thread: Optional[threading.Thread] = None
        self._visible = True

        self._user32 = ctypes.windll.user32

        QObject.__init__(self)
        super().__init__(
            config,
            logger,
            locked,
            opacity,
            scale_factor,
            windowed_overlay,
        )

    @final
    def _setup_window(self):
        super()._setup_window()
        self._launch_native_overlay()
        self._wait_for_window()
        self.update_window_flags()

    @final
    def build_ui(self):
        pass

    def set_window_title(self, _title: str):
        # The native Rust process owns the real window title.
        pass

    def set_window_icon(self, _icon: QIcon):
        # The native Rust process owns the real window icon.
        pass

    @final
    def apply_config(self):
        self.set_ui_scale(self.scale_factor)
        self.set_window_position(self.config)
        self.set_opacity(self.opacity)
        self.set_visibility(self._visible)

    @final
    def update_window_flags(self):
        hwnd = self._require_hwnd()
        ex_style = self._user32.GetWindowLongW(hwnd, self.GWL_EXSTYLE)
        ex_style |= self.WS_EX_LAYERED

        if self.windowed_overlay or not self.locked:
            ex_style &= ~self.WS_EX_TRANSPARENT
        else:
            ex_style |= self.WS_EX_TRANSPARENT

        self._user32.SetWindowLongW(hwnd, self.GWL_EXSTYLE, ex_style)
        self._user32.SetWindowPos(
            hwnd,
            self.HWND_TOPMOST,
            0,
            0,
            0,
            0,
            self.SWP_NOMOVE | self.SWP_NOSIZE | self.SWP_NOACTIVATE | self.SWP_SHOWWINDOW,
        )
        self.set_opacity(self.opacity)

    @final
    def set_opacity(self, opacity: int):
        self.opacity = opacity
        hwnd = self._require_hwnd()
        ex_style = self._user32.GetWindowLongW(hwnd, self.GWL_EXSTYLE) | self.WS_EX_LAYERED
        self._user32.SetWindowLongW(hwnd, self.GWL_EXSTYLE, ex_style)
        self._user32.SetLayeredWindowAttributes(hwnd, 0, int((opacity / 100.0) * 255), self.LWA_ALPHA)

    @final
    def get_window_info(self) -> OverlayPosition:
        hwnd = self._require_hwnd()
        rect = RECT()
        if not self._user32.GetWindowRect(hwnd, ctypes.byref(rect)):
            raise RuntimeError("failed to query native input telemetry window position")
        return OverlayPosition(x=rect.left, y=rect.top)

    @final
    def set_window_position(self, config: OverlayPosition):
        hwnd = self._require_hwnd()
        self.config = config
        self._user32.SetWindowPos(
            hwnd,
            self.HWND_TOPMOST,
            config.x,
            config.y,
            0,
            0,
            self.SWP_NOSIZE | self.SWP_NOACTIVATE | self.SWP_SHOWWINDOW,
        )

    @final
    def set_visibility(self, visible: bool):
        hwnd = self._require_hwnd()
        self._visible = visible
        self._user32.ShowWindow(hwnd, self.SW_SHOWNA if visible else self.SW_HIDE)

    @final
    def set_ui_scale(self, ui_scale: float):
        hwnd = self._require_hwnd()
        self.scale_factor = ui_scale
        width = max(1, round(self.BASE_WINDOW_WIDTH * ui_scale))
        height = max(1, round(self.BASE_WINDOW_HEIGHT * ui_scale))
        self._user32.SetWindowPos(
            hwnd,
            self.HWND_TOPMOST,
            0,
            0,
            width,
            height,
            self.SWP_NOMOVE | self.SWP_NOACTIVATE | self.SWP_SHOWWINDOW,
        )

    @final
    def get_visibility(self) -> bool:
        if self._hwnd is None:
            return self._visible
        return bool(self._user32.IsWindowVisible(self._hwnd))

    @final
    def set_locked_state(self, locked: bool):
        if self.locked == locked:
            return

        try:
            self.config = self.get_window_info()
        except Exception:  # pylint: disable=broad-except
            pass

        self.locked = locked
        self._restart_native_overlay()

    def shutdown(self):
        if not self._process:
            return

        try:
            process = psutil.Process(self._process.pid)
            children = process.children(recursive=True)
            for child in children:
                child.terminate()
            process.terminate()
            _, alive = psutil.wait_procs([process, *children], timeout=5)
            for proc in alive:
                proc.kill()
        except psutil.NoSuchProcess:
            pass
        finally:
            self._process = None
            self._hwnd = None

    def _launch_native_overlay(self) -> None:
        command, workdir = self._build_launch_command()
        self.logger.info(
            "%s | Launching Rust input telemetry overlay: %s",
            self.OVERLAY_ID,
            " ".join(command),
        )
        self._process = subprocess.Popen(  # pylint: disable=consider-using-with
            command,
            cwd=workdir,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        self._output_thread = threading.Thread(
            target=self._drain_output,
            daemon=True,
            name="rust-input-telemetry-log",
        )
        self._output_thread.start()

    def _build_launch_command(self) -> tuple[list[str], Path]:
        repo_root = Path(__file__).resolve().parents[5]
        rust_workspace = repo_root / "apps" / "backend" / "rust"
        binary = rust_workspace / "target" / "debug" / "hud-renderer.exe"

        history_length = max(
            1,
            round((self._window_duration_sec * 1000.0) / self._fetch_interval_ms),
        )

        overlay_args = [
            "--title",
            self._title,
            "--history-length",
            str(history_length),
            "--render-interval-ms",
            str(self._render_interval_ms),
            "--fetch-interval-ms",
            str(self._fetch_interval_ms),
        ]
        if not self.locked:
            overlay_args.append("--movable")
        if self._backend_url:
            overlay_args.append(self._backend_url)

        if binary.is_file():
            return [str(binary), *overlay_args], rust_workspace

        return ["cargo", "run", "-p", "hud-renderer", "--", *overlay_args], rust_workspace

    def _drain_output(self):
        assert self._process and self._process.stdout
        with self._process.stdout as stdout:
            for raw_line in stdout:
                line = raw_line.rstrip()
                if line:
                    self.logger.debug("%s | rust: %s", self.OVERLAY_ID, line)

    def _wait_for_window(self):
        assert self._process is not None
        deadline = time.time() + self.STARTUP_TIMEOUT_SEC

        while time.time() < deadline:
            if self._process.poll() is not None:
                raise RuntimeError("Rust input telemetry process exited before creating a window")

            hwnd = self._user32.FindWindowW(None, self._title)
            if hwnd:
                self._hwnd = hwnd
                return

            time.sleep(0.1)

        raise TimeoutError("Timed out waiting for the Rust input telemetry window")

    def _require_hwnd(self) -> int:
        if self._hwnd and self._user32.IsWindow(self._hwnd):
            return self._hwnd

        hwnd = self._user32.FindWindowW(None, self._title)
        if hwnd:
            self._hwnd = hwnd
            return hwnd

        raise RuntimeError("Rust input telemetry window is not available")

    def _restart_native_overlay(self) -> None:
        was_visible = self._visible
        self.shutdown()
        self._launch_native_overlay()
        self._wait_for_window()
        self.apply_config()
        self.update_window_flags()
        self.set_visibility(was_visible)
