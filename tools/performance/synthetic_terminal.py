#!/usr/bin/env python3
"""Synthetic Linux PTY workload. Never executes input or saves terminal contents."""

import codecs
import os
import select
import signal
import sys
import termios
import tty


def screen():
    columns, rows = os.get_terminal_size()
    print("\x1b[2J\x1b[H\x1b[?2004h", end="")
    print("VINTAGE synthetic terminal fixture\r")
    print(f"Size: {columns} columns x {rows} rows\r")
    print("\x1b[31mRed \x1b[32mGreen \x1b[34mBlue \x1b[0mnormal\r")
    print("Japanese: 日本語入力  Wide: ＡＢＣ  Combining: e\u0301  Emoji: 🙂🚀\r")
    print("\x1b[1mBold\x1b[22m  \x1b[3mItalic\x1b[23m  \x1b[4mUnderline\x1b[24m\r")
    print("Type to echo; q: quit; b: burst; a: alternate screen; r: cursor query\r")
    print("Synthetic input> ", end="", flush=True)


def main():
    if os.name != "posix" or not sys.stdin.isatty():
        raise SystemExit("This fixture requires a POSIX terminal")
    original = termios.tcgetattr(sys.stdin.fileno())
    decoder = codecs.getincrementaldecoder("utf-8")("replace")
    redraw = True

    def resized(_signum, _frame):
        nonlocal redraw
        redraw = True

    signal.signal(signal.SIGWINCH, resized)
    try:
        tty.setraw(sys.stdin.fileno())
        while True:
            if redraw:
                screen()
                redraw = False
            if not select.select([sys.stdin], [], [], 0.1)[0]:
                continue
            data = os.read(sys.stdin.fileno(), 4096)
            if not data or data in (b"q", b"\x03"):
                break
            if data == b"b":
                for index in range(10000):
                    print(
                        f"\r\nSynthetic output {index:05d}: 日本語 e\u0301 🙂", end=""
                    )
                print("\r\nBURST_COMPLETE\r\n", end="", flush=True)
            elif data == b"a":
                print(
                    "\x1b[?1049h\x1b[2J\x1b[HAlternate screen — press any key to restore",
                    end="",
                    flush=True,
                )
                os.read(sys.stdin.fileno(), 4096)
                print("\x1b[?1049l", end="", flush=True)
            elif data == b"r":
                print("\x1b[6n", end="", flush=True)
            else:
                # Display escape/control bytes literally, so pasted input cannot
                # inject output-side terminal commands into the fixture.
                text = decoder.decode(data)
                text = "".join(c if c >= " " else f"<{ord(c):02x}>" for c in text)
                print(text, end="", flush=True)
    finally:
        print("\x1b[?2004l\x1b[0m\r\n", end="", flush=True)
        termios.tcsetattr(sys.stdin.fileno(), termios.TCSADRAIN, original)


if __name__ == "__main__":
    main()
