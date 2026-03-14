# MIT License
#
# Copyright (c) [2024] [Ashwin Natarajan]
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

# -------------------------------------- IMPORTS -----------------------------------------------------------------------

import logging

import requests

from lib.ipc.socketio_client import SocketioClient
from ..ui.infra import OverlaysMgr

# -------------------------------------- CLASSES -----------------------------------------------------------------------

class HudClient(SocketioClient):
    """HUD action listener backed by the Rust backend update feed."""
    def __init__(self, port: int, logger: logging.Logger, overlays_mgr: OverlaysMgr):
        """Args:
            port: Port number of the Socket.IO server.
            logger: Logger instance.
        """
        url = f"http://localhost:{port}"
        super().__init__(url, logger, msg_packed=True)
        self.m_overlays_mgr = overlays_mgr
        self._base_url = url.rstrip("/")
        self._last_update_id = None
        self._poll_interval_s = 0.1
        self._request_timeout_s = 1.0

        # optional connect/disconnect hooks
        @self.on_connect
        def connected():
            self.logger.info("HUD subsytem connected to Core subsystem")
            self.logger.debug("[HudClient] Registering client. SID=%s", self._sio.sid)
            try:
                self._sio.emit('register-client', {
                    'type': 'hud',
                    'id': 'hud-mgr',
                })
            except Exception as e: # pylint: disable=broad-except
                self.logger.exception("[HudClient] Failed to register client: %s", e)

        @self.on_disconnect
        def disconnected():
            """Post disconnection callback."""
            self.logger.info("[HudClient] Disconnected")

        @self.on('hud-toggle-notification')
        def handle_hud_toggle_notification(data):
            """HUD toggle notification handler."""
            self._handle_hud_toggle_notification(data)

        @self.on('hud-cycle-mfd-notification')
        def handle_hud_cycle_mfd_notification(_data):
            """Cycle MFD notification handler."""
            self._handle_hud_cycle_mfd_notification()

        @self.on('hud-prev-page-mfd-notification')
        def handle_hud_prev_page_mfd_notification(_data):
            """Previous page MFD notification handler."""
            self._handle_hud_prev_page_mfd_notification()

        @self.on('hud-mfd-interaction-notification')
        def handle_hud_mfd_interact_notification(_data):
            """MFD interact notification handler."""
            self._handle_hud_mfd_interact_notification()

    def run(self) -> None:
        """Poll the Rust backend HUD update feed and dispatch notifications."""
        endpoint = f"{self._base_url}/hud-updates"
        self.logger.debug("Starting HUD update polling loop against %s", endpoint)

        with requests.Session() as session:
            while not self._stop_event.is_set():
                params = {}
                if self._last_update_id is not None:
                    params["after"] = self._last_update_id

                try:
                    response = session.get(
                        endpoint,
                        params=params,
                        timeout=self._request_timeout_s,
                    )
                    response.raise_for_status()
                    payload = response.json()
                    self._connected = True
                    self._handle_update_entries(payload.get("entries", []))
                except requests.RequestException as exc:
                    if self._connected:
                        self.logger.warning(
                            "[HudClient] Lost backend HUD update feed at %s: %s",
                            endpoint,
                            exc,
                        )
                    else:
                        self.logger.debug(
                            "[HudClient] Waiting for backend HUD update feed at %s: %s",
                            endpoint,
                            exc,
                        )
                    self._connected = False
                except ValueError as exc:
                    self.logger.warning(
                        "[HudClient] Failed to decode HUD update payload from %s: %s",
                        endpoint,
                        exc,
                    )

                self._stop_event.wait(self._poll_interval_s)

        self.logger.debug("HUD update polling loop finished")

    def _handle_update_entries(self, entries) -> None:
        """Handle a snapshot of HUD update entries from the backend."""
        for entry in entries:
            if not isinstance(entry, dict):
                continue

            entry_id = entry.get("id")
            if isinstance(entry_id, int):
                self._last_update_id = entry_id

            payload = entry.get("payload")
            if not isinstance(payload, dict):
                continue

            self._dispatch_hud_message(payload)

    def _dispatch_hud_message(self, payload: dict) -> None:
        """Dispatch a backend HUD update payload to the overlays manager."""
        message_type = payload.get("message-type")
        if message_type == "hud-toggle-notification":
            self._handle_hud_toggle_notification(payload)
        elif message_type == "hud-cycle-mfd-notification":
            self._handle_hud_cycle_mfd_notification()
        elif message_type == "hud-prev-page-mfd-notification":
            self._handle_hud_prev_page_mfd_notification()
        elif message_type == "hud-mfd-interaction-notification":
            self._handle_hud_mfd_interact_notification()
        else:
            self.logger.debug("[HudClient] Ignoring unsupported HUD message type: %s", message_type)

    def _handle_hud_toggle_notification(self, data: dict) -> None:
        """HUD toggle notification handler."""
        self.logger.debug("[HudClient] Received HUD toggle notification. data=%s", data)
        oid = data.get('message', {}).get('oid')
        if oid is None:
            self.logger.warning("[HudClient] Received HUD toggle notification with no overlay ID. Ignoring.")
        else:
            self.m_overlays_mgr.toggle_overlays_visibility(oid)

    def _handle_hud_cycle_mfd_notification(self) -> None:
        """Cycle MFD notification handler."""
        self.logger.debug("[HudClient] Received Cycle MFD notification")
        self.m_overlays_mgr.next_page()

    def _handle_hud_prev_page_mfd_notification(self) -> None:
        """Previous page MFD notification handler."""
        self.logger.debug("[HudClient] Received Previous page MFD notification")
        self.m_overlays_mgr.prev_page()

    def _handle_hud_mfd_interact_notification(self) -> None:
        """MFD interact notification handler."""
        self.logger.debug("[HudClient] Received MFD interact notification")
        self.m_overlays_mgr.mfd_interact()
