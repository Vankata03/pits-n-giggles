import os
import sys
import threading
import types
import unittest
from unittest.mock import Mock, patch

sys.path.append(os.path.abspath(os.path.join(os.path.dirname(__file__), '..')))

fake_ipc_pkg = types.ModuleType("lib.ipc")
fake_ipc_pkg.__path__ = []
fake_socketio_client_module = types.ModuleType("lib.ipc.socketio_client")


class FakeSocketioClient:
    def __init__(self, url, logger=None, msg_packed=False):
        self.url = url
        self.logger = logger
        self._msg_packed = msg_packed
        self._stop_event = threading.Event()
        self._connected = False

    def on(self, _event_name):
        def decorator(func):
            return func
        return decorator

    def on_connect(self, func):
        return func

    def on_disconnect(self, func):
        return func

    def stop(self):
        self._stop_event.set()
        self._connected = False


fake_socketio_client_module.SocketioClient = FakeSocketioClient
fake_ipc_pkg.socketio_client = fake_socketio_client_module
sys.modules.setdefault("lib.ipc", fake_ipc_pkg)
sys.modules["lib.ipc.socketio_client"] = fake_socketio_client_module

fake_ui_infra_module = types.ModuleType("apps.hud.ui.infra")
fake_ui_infra_module.OverlaysMgr = object
sys.modules["apps.hud.ui.infra"] = fake_ui_infra_module

fake_listener_task_module = types.ModuleType("apps.hud.listener.task")
fake_listener_task_module.run_hud_update_threads = Mock()
sys.modules["apps.hud.listener.task"] = fake_listener_task_module

from apps.hud.listener.client import HudClient


class TestHudListenerClient(unittest.TestCase):
    @patch("apps.hud.listener.client.requests.Session")
    def test_polls_backend_hud_updates_and_dispatches_overlay_actions(
        self,
        mock_session_cls,
    ):
        logger = Mock()
        overlays_mgr = Mock()
        client = HudClient(port=4768, logger=logger, overlays_mgr=overlays_mgr)
        client._poll_interval_s = 0

        response_one = Mock()
        response_one.raise_for_status.return_value = None
        response_one.json.return_value = {
            "entries": [
                {
                    "id": 10,
                    "payload": {
                        "message-type": "hud-toggle-notification",
                        "message": {"oid": "mfd"},
                    },
                },
                {
                    "id": 11,
                    "payload": {
                        "message-type": "hud-cycle-mfd-notification",
                        "message": {},
                    },
                },
            ]
        }

        response_two = Mock()
        response_two.raise_for_status.return_value = None
        response_two.json.return_value = {"entries": []}

        session = Mock()
        call_count = {"value": 0}

        def fake_get(*args, **kwargs):
            call_count["value"] += 1
            if call_count["value"] == 1:
                return response_one
            client.stop()
            return response_two

        session.get.side_effect = fake_get
        mock_session_cls.return_value.__enter__.return_value = session
        mock_session_cls.return_value.__exit__.return_value = False

        client.run()

        overlays_mgr.toggle_overlays_visibility.assert_called_once_with("mfd")
        overlays_mgr.next_page.assert_called_once_with()

        self.assertEqual(session.get.call_args_list[0].kwargs["params"], {})
        self.assertEqual(session.get.call_args_list[1].kwargs["params"], {"after": 11})
