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

import ctypes
import logging
import os
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Optional, final

import psutil
from PySide6.QtCore import QObject
from PySide6.QtGui import QIcon

from .base import BaseOverlay
from lib.config import OverlayPosition


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", ctypes.c_long),
        ("top", ctypes.c_long),
        ("right", ctypes.c_long),
        ("bottom", ctypes.c_long),
    ]


class RustNativeOverlay(BaseOverlay, QObject):
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

    BASE_WINDOW_WIDTH = 100
    BASE_WINDOW_HEIGHT = 100
    RUST_BINARY_STEM = ""

    def __init__(
        self,
        config: OverlayPosition,
        logger: logging.Logger,
        locked: bool,
        opacity: int,
        scale_factor: float,
        windowed_overlay: bool,
        base_url: Optional[str] = None,
    ) -> None:
        assert self.RUST_BINARY_STEM
        self._backend_url = base_url
        self._launch_generation = 0
        self._title = self._build_window_title()
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
            raise RuntimeError(f"failed to query native {self.OVERLAY_ID} window position")
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
        try:
            self.config = self.get_window_info()
        except Exception:  # pylint: disable=broad-except
            pass

        self.scale_factor = ui_scale
        self._restart_native_overlay()

    @final
    def get_visibility(self) -> bool:
        if self._hwnd is None:
            return self._visible
        return bool(self._user32.IsWindowVisible(self._hwnd))

    @final
    def set_locked_state(self, locked: bool):
        if self.locked == locked:
            return

        self.locked = locked
        self.update_window_flags()

        if self.locked and not self.telemetry_active:
            self.logger.debug(
                "%s locking overlay. But hiding it since telemetry is not active",
                self.OVERLAY_ID,
            )
            self.set_visibility(False)

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

    def _build_overlay_args(self) -> list[str]:
        raise NotImplementedError

    def _launch_native_overlay(self) -> None:
        self._launch_generation += 1
        self._title = self._build_window_title()
        command, workdir = self._build_launch_command()
        self.logger.info(
            "%s | Launching Rust overlay: %s",
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
            name=f"rust-{self.OVERLAY_ID}-log",
        )
        self._output_thread.start()

    def _build_launch_command(self) -> tuple[list[str], Path]:
        overlay_args = self._build_overlay_args()

        if env_bin := os.environ.get("PNG_HUD_RENDERER_BIN"):
            env_path = Path(env_bin)
            if env_path.is_file():
                return [str(env_path), *overlay_args], env_path.parent
            if env_path.is_dir():
                for candidate_name in self._rust_binary_names():
                    candidate = env_path / candidate_name
                    if candidate.is_file():
                        return [str(candidate), *overlay_args], candidate.parent

        if getattr(sys, "frozen", False):
            search_dirs = []
            meipass = getattr(sys, "_MEIPASS", None)
            if meipass:
                search_dirs.append(Path(meipass))
            search_dirs.append(Path(sys.executable).resolve().parent)

            for search_dir in search_dirs:
                for candidate_name in self._rust_binary_names():
                    candidate = search_dir / candidate_name
                    if candidate.is_file():
                        return [str(candidate), *overlay_args], candidate.parent

            raise RuntimeError(f"Packaged Rust HUD renderer binary was not found for {self.OVERLAY_ID}")

        repo_root = self._find_repo_root()
        rust_workspace = repo_root / "apps" / "backend" / "rust"
        latest_source_mtime = self._latest_renderer_source_mtime(rust_workspace)

        for build_profile in ("debug", "release"):
            for candidate_name in self._rust_binary_names():
                candidate = rust_workspace / "target" / build_profile / candidate_name
                if candidate.is_file() and candidate.stat().st_mtime >= latest_source_mtime:
                    return [str(candidate), *overlay_args], candidate.parent

        cargo = shutil.which("cargo")
        manifest_path = rust_workspace / "Cargo.toml"
        if cargo and manifest_path.is_file():
            return [
                cargo,
                "run",
                "--manifest-path",
                str(manifest_path),
                "-p",
                "hud-renderer",
                "--bin",
                self.RUST_BINARY_STEM,
                "--",
                *overlay_args,
            ], rust_workspace

        raise RuntimeError(f"Rust HUD renderer launch command was not found for {self.OVERLAY_ID}")

    def _build_window_title(self) -> str:
        return f"png-rust-{self.OVERLAY_ID}-{os.getpid()}-{self._launch_generation}"

    def _find_repo_root(self) -> Path:
        current = Path(__file__).resolve()
        for parent in current.parents:
            if (parent / "apps" / "backend" / "rust" / "Cargo.toml").is_file():
                return parent
        raise RuntimeError("Could not locate repository root for Rust HUD renderer")

    def _latest_renderer_source_mtime(self, rust_workspace: Path) -> float:
        renderer_dir = rust_workspace / "crates" / "hud-renderer"
        candidate_paths = [rust_workspace / "Cargo.toml", renderer_dir / "Cargo.toml"]
        candidate_paths.extend(renderer_dir.rglob("*.rs"))
        return max(path.stat().st_mtime for path in candidate_paths if path.is_file())

    def _rust_binary_names(self) -> tuple[str, ...]:
        return (f"{self.RUST_BINARY_STEM}.exe", self.RUST_BINARY_STEM)

    @property
    def _scaled_window_width(self) -> int:
        return max(1, round(self.BASE_WINDOW_WIDTH * self.scale_factor))

    @property
    def _scaled_window_height(self) -> int:
        return max(1, round(self.BASE_WINDOW_HEIGHT * self.scale_factor))

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
                raise RuntimeError(f"Rust {self.OVERLAY_ID} process exited before creating a window")

            hwnd = self._user32.FindWindowW(None, self._title)
            if hwnd:
                self._hwnd = hwnd
                return

            time.sleep(0.1)

        raise TimeoutError(f"Timed out waiting for the Rust {self.OVERLAY_ID} window")

    def _require_hwnd(self) -> int:
        if self._hwnd and self._user32.IsWindow(self._hwnd):
            return self._hwnd

        hwnd = self._user32.FindWindowW(None, self._title)
        if hwnd:
            self._hwnd = hwnd
            return hwnd

        raise RuntimeError(f"Rust {self.OVERLAY_ID} window is not available")

    def _restart_native_overlay(self) -> None:
        was_visible = self._visible
        self.shutdown()
        self._launch_native_overlay()
        self._wait_for_window()
        self.set_opacity(self.opacity)
        self.update_window_flags()
        self.set_visibility(was_visible)
