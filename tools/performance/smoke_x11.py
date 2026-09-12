#!/usr/bin/env python3
"""Capture synthetic GPUI X11 input/resize/output and verify window shutdown.

Uses only the window whose PID matches the child launched by this process.
Screenshots are evidence for visual review, not automatic rendering assertions.
"""

import argparse
import ctypes as c
import json
import os
from pathlib import Path
import re
import subprocess
import time


class Key(c.Structure):
    _fields_ = [
        ("type", c.c_int),
        ("serial", c.c_ulong),
        ("send_event", c.c_int),
        ("display", c.c_void_p),
        ("window", c.c_ulong),
        ("root", c.c_ulong),
        ("subwindow", c.c_ulong),
        ("time", c.c_ulong),
        ("x", c.c_int),
        ("y", c.c_int),
        ("x_root", c.c_int),
        ("y_root", c.c_int),
        ("state", c.c_uint),
        ("keycode", c.c_uint),
        ("same_screen", c.c_int),
    ]


class Data(c.Union):
    _fields_ = [("l", c.c_long * 5), ("b", c.c_char * 20)]


class Client(c.Structure):
    _fields_ = [
        ("type", c.c_int),
        ("serial", c.c_ulong),
        ("send_event", c.c_int),
        ("display", c.c_void_p),
        ("window", c.c_ulong),
        ("message_type", c.c_ulong),
        ("format", c.c_int),
        ("data", Data),
    ]


class Event(c.Union):
    _fields_ = [("key", Key), ("client", Client), ("pad", c.c_long * 24)]


class WindowControl:
    def __init__(self, window):
        self.lib = c.CDLL("libX11.so.6")
        for name, arguments, result in [
            ("XOpenDisplay", [c.c_char_p], c.c_void_p),
            ("XDefaultRootWindow", [c.c_void_p], c.c_ulong),
            ("XStringToKeysym", [c.c_char_p], c.c_ulong),
            ("XKeysymToKeycode", [c.c_void_p, c.c_ulong], c.c_uint),
            (
                "XSendEvent",
                [c.c_void_p, c.c_ulong, c.c_int, c.c_long, c.c_void_p],
                c.c_int,
            ),
            ("XFlush", [c.c_void_p], c.c_int),
            ("XCloseDisplay", [c.c_void_p], c.c_int),
            ("XResizeWindow", [c.c_void_p, c.c_ulong, c.c_uint, c.c_uint], c.c_int),
            ("XInternAtom", [c.c_void_p, c.c_char_p, c.c_int], c.c_ulong),
        ]:
            function = getattr(self.lib, name)
            function.argtypes = arguments
            function.restype = result
        self.display = self.lib.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError("Cannot connect to the X11 display")
        self.window = window
        self.root = self.lib.XDefaultRootWindow(self.display)

    def key(self, name, modifiers=0):
        event = Event()
        code = self.lib.XKeysymToKeycode(
            self.display, self.lib.XStringToKeysym(name.encode())
        )
        if not code:
            raise RuntimeError(f"No X11 keycode for {name}")
        event.key = Key(
            2,
            0,
            1,
            self.display,
            self.window,
            self.root,
            0,
            0,
            20,
            100,
            20,
            100,
            0,
            code,
            1,
        )
        event.key.state = modifiers
        if not self.lib.XSendEvent(self.display, self.window, 0, 1, c.byref(event)):
            raise RuntimeError("Cannot send key press")
        event.key.type = 3
        if not self.lib.XSendEvent(self.display, self.window, 0, 2, c.byref(event)):
            raise RuntimeError("Cannot send key release")
        self.lib.XFlush(self.display)
        time.sleep(0.1)

    def resize(self):
        self.lib.XResizeWindow(self.display, self.window, 800, 520)
        self.lib.XFlush(self.display)
        time.sleep(0.3)

    def close_window(self):
        event = Event()
        event.client = Client(
            33,
            0,
            1,
            self.display,
            self.window,
            self.lib.XInternAtom(self.display, b"WM_PROTOCOLS", 0),
            32,
            Data(),
        )
        event.client.data.l[0] = self.lib.XInternAtom(
            self.display, b"WM_DELETE_WINDOW", 0
        )
        if not self.lib.XSendEvent(self.display, self.window, 0, 0, c.byref(event)):
            raise RuntimeError("Cannot send window close")
        self.lib.XFlush(self.display)

    def disconnect(self):
        self.lib.XCloseDisplay(self.display)


def owned_window(process, env):
    for _ in range(100):
        if process.poll() is not None:
            raise RuntimeError("Preview exited before its window opened")
        result = subprocess.run(
            ["xwininfo", "-name", "VINTAGE GPUI Preview"],
            capture_output=True,
            text=True,
            env=env,
            timeout=5,
        )
        match = re.search(r"Window id: (0x[0-9a-fA-F]+)", result.stdout)
        if match:
            window = match.group(1)
            prop = subprocess.run(
                ["xprop", "-id", window, "_NET_WM_PID"],
                capture_output=True,
                text=True,
                env=env,
                timeout=5,
            )
            pid_match = re.search(r"=\s*(\d+)\s*$", prop.stdout)
            if pid_match and int(pid_match.group(1)) == process.pid:
                return int(window, 16)
        time.sleep(0.1)
    raise RuntimeError(
        "No owned Preview window appeared; close other Preview windows first"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary", type=Path, default=Path("target/debug/vintage-gpui")
    )
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--workspace",
        action="store_true",
        help="Exercise tabs, recursive splits and pane cleanup using synthetic PTYs",
    )
    parser.add_argument(
        "--ime",
        action="store_true",
        help="Keep the current XIM configuration and capture preedit/commit",
    )
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output directory already exists; choose a new path")
    args.output.mkdir(parents=True)
    env = dict(os.environ, WAYLAND_DISPLAY="")
    if not args.ime:
        env["XMODIFIERS"] = "@im=none"
    fixture = Path(__file__).resolve().with_name("synthetic_terminal.py")
    with (args.output / "application.log").open("w") as log:
        process = subprocess.Popen(
            [str(args.binary.resolve()), "--shell", str(fixture)],
            env=env,
            stdout=log,
            stderr=log,
        )
        control = None
        try:
            window = owned_window(process, env)
            control = WindowControl(window)
            time.sleep(0.5)

            def capture(name):
                subprocess.run(
                    ["import", "-window", hex(window), str(args.output / name)],
                    env=env,
                    check=True,
                    timeout=10,
                )

            capture("initial.png")
            if args.workspace:

                def wait_children(expected):
                    children_path = Path(
                        f"/proc/{process.pid}/task/{process.pid}/children"
                    )
                    for _ in range(100):
                        children = children_path.read_text().split()
                        if len(children) == expected:
                            return
                        time.sleep(0.05)
                    raise RuntimeError(
                        f"Expected {expected} synthetic PTYs, found {len(children)}"
                    )

                wait_children(1)
                control.key("d", modifiers=5)  # Control + Shift
                wait_children(2)
                control.key("e", modifiers=5)
                wait_children(3)
                time.sleep(0.5)
                capture("workspace-three-panes.png")
                control.key("t", modifiers=5)
                wait_children(4)
                capture("workspace-second-tab.png")
                control.key("Tab", modifiers=5)
                time.sleep(0.3)
                capture("workspace-restored-tab.png")
                control.key("w", modifiers=5)
                wait_children(3)
                capture("workspace-pane-closed.png")
                control.key("w", modifiers=5)
                wait_children(2)
                control.key("w", modifiers=5)
                wait_children(1)
                capture("workspace-last-tab.png")
            control.resize()
            for character in "nihongo" if args.ime else "check":
                control.key(character)
            capture("preedit.png" if args.ime else "input-resize.png")
            if args.ime:
                control.key("Return")
                capture("committed.png")
            else:
                control.key("b")
                time.sleep(1)
                capture("burst.png")
                control.key("q")
                time.sleep(0.3)
                capture("shell-exited.png")
            control.close_window()
            process.wait(timeout=5)
            if process.returncode != 0:
                raise RuntimeError(f"Preview exited with code {process.returncode}")
            report = {
                "exit_code": process.returncode,
                "backend": "X11 (may be XWayland)",
                "ime_requested": args.ime,
                "workspace_exercised": args.workspace,
                "rendering": "Screenshots require visual review; not an automatic pass",
            }
            (args.output / "result.json").write_text(
                json.dumps(report, indent=2) + "\n"
            )
            print(
                "Owned-window smoke sequence completed; inspect screenshots in",
                args.output,
            )
        finally:
            if control:
                control.disconnect()
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)


if __name__ == "__main__":
    main()
