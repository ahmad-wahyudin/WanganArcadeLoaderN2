#!/usr/bin/env python3
"""
jvs_debug.py - JVS I/O board debugger (Linux, Python 3.6+, no extra packages)

Talks JVS over a USB-RS485 adapter and shows the board ID, its capabilities
and a live view of switches, coins and analog axes, plus link statistics
(poll rate, round-trip time, timeouts, checksum errors, re-inits).

  python3 jvs_debug.py /dev/ttyUSB0 --info                      # ID + capabilities, then exit
  python3 jvs_debug.py /dev/ttyUSB0                             # live monitor (full screen)
  python3 jvs_debug.py /dev/ttyUSB0 --log                       # print only changes, timestamped
  python3 jvs_debug.py /dev/ttyUSB0 --info --reset-delay 0.1    # reset with delay

Stop jvscore and the game first: only one program can be the bus master.
"""
import argparse
import collections
import os
import select
import sys
import termios
import time

SYNC, ESC = 0xE0, 0xD0
BROADCAST, HOST = 0xFF, 0x00

CMD_RESET, CMD_SETADDR = 0xF0, 0xF1
CMD_IOIDENT, CMD_CMDREV, CMD_JVSREV, CMD_COMVER = 0x10, 0x11, 0x12, 0x13
CMD_FEATURES, CMD_MAINID = 0x14, 0x15
CMD_SWITCHES, CMD_COINS, CMD_ANALOG = 0x20, 0x21, 0x22

STATUS = {1: "normal", 2: "unknown command", 3: "checksum error",
          4: "acknowledge overflow"}
REPORT = {1: "normal", 2: "invalid parameter", 3: "data error", 4: "busy"}
COIN_COND = {0: "ok", 1: "JAM", 2: "NO-CONNECT", 3: "busy"}

SYS_NAMES = ["TEST", "TILT1", "TILT2", "TILT3",
             "SYS.b3", "SYS.b2", "SYS.b1", "SYS.b0"]
PLAYER_NAMES = [
    ["START", "SERVICE", "UP", "DOWN", "LEFT", "RIGHT", "PUSH1", "PUSH2"],
    ["PUSH3 (UP)", "PUSH4 (DOWN)", "PUSH5 (LEFT)", "PUSH6 (RIGHT)", "PUSH7", "PUSH8", "PUSH9 (PERSPECTIVE)", "PUSH10 (REJECT)"],
]


class JvsError(Exception):
    pass


class Timeout(JvsError):
    pass


class BadChecksum(JvsError):
    pass


def encode(node, payload):
    body = [node, len(payload) + 1] + list(payload)
    body.append(sum(body) & 0xFF)
    out = bytearray([SYNC])
    for b in body:
        if b in (SYNC, ESC):
            out += bytes([ESC, b - 1])
        else:
            out.append(b)
    return bytes(out)


class Port:
    def __init__(self, path):
        self.path = path
        self.fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        attrs = termios.tcgetattr(self.fd)
        attrs[0] = 0                                    # iflag
        attrs[1] = 0                                    # oflag
        attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
        attrs[3] = 0                                    # lflag: raw, no echo
        attrs[4] = attrs[5] = termios.B115200
        attrs[6][termios.VMIN] = 0
        attrs[6][termios.VTIME] = 0
        termios.tcsetattr(self.fd, termios.TCSANOW, attrs)
        termios.tcflush(self.fd, termios.TCIOFLUSH)
        self.buf = bytearray()
        self.timeouts = 0
        self.crc_errors = 0
        self.rtts = collections.deque(maxlen=300)

    def close(self):
        try:
            os.close(self.fd)
        except OSError:
            pass

    def flush_input(self):
        self.buf.clear()
        try:
            termios.tcflush(self.fd, termios.TCIFLUSH)
        except termios.error:
            pass

    def write(self, data):
        view = memoryview(data)
        while view:
            try:
                n = os.write(self.fd, view)
            except BlockingIOError:
                select.select([], [self.fd], [], 0.05)
                continue
            except OSError as e:
                raise JvsError("write failed: %s" % e)
            view = view[n:]
        try:
            termios.tcdrain(self.fd)
        except termios.error:
            pass

    def _byte(self, deadline):
        while not self.buf:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            ready, _, _ = select.select([self.fd], [], [], remaining)
            if not ready:
                continue
            try:
                chunk = os.read(self.fd, 256)
            except BlockingIOError:
                continue
            except OSError as e:
                raise JvsError("read failed: %s" % e)
            if chunk:
                self.buf += chunk
            else:
                time.sleep(0.001)
        return self.buf.pop(0)

    def _esc(self, deadline):
        b = self._byte(deadline)
        if b is None:
            raise Timeout("timeout")
        if b == ESC:
            b = self._byte(deadline)
            if b is None:
                raise Timeout("timeout")
            b = (b + 1) & 0xFF
        return b

    def read_packet(self, timeout):
        deadline = time.monotonic() + timeout
        while True:
            b = self._byte(deadline)
            if b is None:
                raise Timeout("timeout")
            if b != SYNC:
                continue
            node = self._esc(deadline)
            length = self._esc(deadline)
            if length == 0:
                continue
            data = [self._esc(deadline) for _ in range(length)]
            if (node + length + sum(data[:-1])) & 0xFF != data[-1]:
                raise BadChecksum("checksum mismatch")
            return node, data[:-1]

    def request(self, node, payload, timeout=0.1, retries=3):
        """Send a command packet, return (response_data, round_trip_seconds).
        response_data = [status, report, ...]. Retries on timeout/CRC error."""
        last = None
        for _ in range(retries):
            self.flush_input()
            t0 = time.monotonic()
            try:
                self.write(encode(node, payload))
                while True:
                    n, data = self.read_packet(timeout)
                    if n == HOST:        # anything else is our own echo
                        break
                rtt = time.monotonic() - t0
                if not data:
                    raise JvsError("empty response")
                if data[0] != 1:
                    raise JvsError("status %d (%s)" % (
                        data[0], STATUS.get(data[0], "?")))
                self.rtts.append(rtt)
                return data, rtt
            except Timeout as e:
                self.timeouts += 1
                last = e
            except BadChecksum as e:
                self.crc_errors += 1
                last = e
            except JvsError as e:
                last = e
        raise last


def need_report(d, i):
    if i >= len(d):
        raise JvsError("response too short")
    if d[i] != 1:
        raise JvsError("report %d (%s)" % (d[i], REPORT.get(d[i], "?")))


def bcd(b):
    return "?" if b is None else "%d.%d" % (b >> 4, b & 15)


def init_bus(port, args, do_reset=True):
    if do_reset:
        for _ in range(2):
            port.write(encode(BROADCAST, [CMD_RESET, 0xD9]))
        time.sleep(args.reset_delay)
        port.flush_input()
        d, _ = port.request(BROADCAST, [CMD_SETADDR, 1])
        need_report(d, 1)


def parse_features(raw):
    feats, i = [], 0
    while i < len(raw) and raw[i] != 0 and i + 4 <= len(raw):
        feats.append(tuple(raw[i:i + 4]))
        i += 4
    return feats


def read_info(port):
    info = {}
    d, _ = port.request(1, [CMD_IOIDENT])
    need_report(d, 1)
    info["id"] = bytes(d[2:]).split(b"\0")[0].decode("latin-1")
    for key, cmd in (("cmdrev", CMD_CMDREV), ("jvsrev", CMD_JVSREV),
                     ("comver", CMD_COMVER)):
        d, _ = port.request(1, [cmd])
        need_report(d, 1)
        info[key] = d[2] if len(d) > 2 else None
    d, _ = port.request(1, [CMD_FEATURES])
    need_report(d, 1)
    info["features"] = parse_features(d[2:])
    return info


def describe_feature(code, a, b, c):
    names = {
        0x01: "Switch input: %d player(s), %d buttons each" % (a, b),
        0x02: "Coin input: %d slot(s)" % a,
        0x03: "Analog input: %d channel(s), %d bits" % (a, b),
        0x04: "Rotary encoder: %d channel(s)" % a,
        0x05: "Keycode input",
        0x06: "Screen position input: x %d bits, y %d bits, %d channel(s)" % (a, b, c),
        0x07: "Misc switch input: %d switches" % ((a << 8) | b),
        0x10: "Card system: %d slot(s)" % a,
        0x11: "Medal hopper: %d channel(s)" % a,
        0x12: "General purpose output: %d slot(s)" % a,
        0x13: "Analog output: %d channel(s)" % a,
        0x14: "Character display: %dx%d, type %d" % (a, b, c),
        0x15: "Backup memory",
    }
    return names.get(code, "Unknown feature 0x%02X (%02X %02X %02X)" % (code, a, b, c))


def caps_from(info, args):
    caps = {"players": None, "buttons": None, "coins": 0,
            "analog": 0, "abits": 0}
    for code, a, b, _c in info["features"]:
        if code == 0x01:
            caps["players"], caps["buttons"] = a, b
        elif code == 0x02:
            caps["coins"] = a
        elif code == 0x03:
            caps["analog"], caps["abits"] = a, b
    if args.players is not None:
        caps["players"] = args.players
    if caps["players"] is None:
        caps["players"] = 2
    caps["players"] = max(1, caps["players"])
    if args.sw_bytes is not None:
        caps["sw_bytes"] = args.sw_bytes
    elif caps["buttons"]:
        caps["sw_bytes"] = max(1, (caps["buttons"] + 7) // 8)
    else:
        caps["sw_bytes"] = 2
    if caps["abits"] == 0:
        caps["abits"] = 16
    return caps


def print_info(info):
    print("I/O board ID : %s" % info["id"])
    print("Cmd format   : %s    JVS rev: %s    Comm ver: %s" % (
        bcd(info["cmdrev"]), bcd(info["jvsrev"]), bcd(info["comver"])))
    print("Capabilities :")
    if not info["features"]:
        print("  (none reported)")
    for f in info["features"]:
        print("  - " + describe_feature(*f))


def build_request(caps):
    req = [CMD_SWITCHES, caps["players"], caps["sw_bytes"]]
    if caps["coins"]:
        req += [CMD_COINS, caps["coins"]]
    if caps["analog"]:
        req += [CMD_ANALOG, caps["analog"]]
    return req


def parse_poll(d, caps):
    pos = [1]

    def take(n):
        s = d[pos[0]:pos[0] + n]
        if len(s) != n:
            raise JvsError("short response")
        pos[0] += n
        return s

    if take(1)[0] != 1:
        raise JvsError("switch report not normal")
    cur = {"system": take(1)[0], "players": [], "coins": [], "analog": []}
    for _ in range(caps["players"]):
        cur["players"].append(list(take(caps["sw_bytes"])))
    if caps["coins"]:
        if take(1)[0] != 1:
            raise JvsError("coin report not normal")
        for _ in range(caps["coins"]):
            b = take(2)
            cur["coins"].append((b[0] >> 6, ((b[0] & 0x3F) << 8) | b[1]))
    if caps["analog"]:
        if take(1)[0] != 1:
            raise JvsError("analog report not normal")
        for _ in range(caps["analog"]):
            b = take(2)
            cur["analog"].append((b[0] << 8) | b[1])
    return cur


def bit_names(value, names):
    return [n for i, n in enumerate(names) if (value >> (7 - i)) & 1]


def byte_names(idx, value):
    if idx < len(PLAYER_NAMES):
        names = PLAYER_NAMES[idx]
    else:
        names = ["B%d.%d" % (idx + 1, 7 - k) for k in range(8)]
    return bit_names(value, names)


def player_label(i):
    return "P%d" % (i + 1)


def bar(v, width=32):
    n = int(round(v / 65535.0 * width))
    return "#" * n + "." * (width - n)


def stamp():
    t = time.time()
    return time.strftime("%H:%M:%S", time.localtime(t)) + ".%03d" % (int(t * 1000) % 1000)


def diff_events(prev, cur, last_analog, threshold):
    ev = []
    if prev is None:
        return ev
    for n in bit_names(cur["system"] & ~prev["system"] & 0xFF, SYS_NAMES):
        ev.append("SYSTEM %s pressed" % n)
    for n in bit_names(prev["system"] & ~cur["system"] & 0xFF, SYS_NAMES):
        ev.append("SYSTEM %s released" % n)
    for p, (old, new) in enumerate(zip(prev["players"], cur["players"])):
        for idx, (o, n) in enumerate(zip(old, new)):
            for name in byte_names(idx, n & ~o & 0xFF):
                ev.append("%s %s pressed" % (player_label(p), name))
            for name in byte_names(idx, o & ~n & 0xFF):
                ev.append("%s %s released" % (player_label(p), name))
    for i, ((oc, ocount), (nc, ncount)) in enumerate(zip(prev["coins"], cur["coins"])):
        if ncount != ocount:
            ev.append("COIN %d count %d -> %d (%+d)" % (
                i + 1, ocount, ncount, (ncount - ocount + 0x2000) % 0x4000 - 0x2000))
        if nc != oc:
            ev.append("COIN %d condition %s" % (i + 1, COIN_COND.get(nc, "?")))
    for ch, v in enumerate(cur["analog"]):
        if abs(v - last_analog[ch]) >= threshold:
            ev.append("ANALOG ch%d raw 0x%04X (%5.1f%%)" % (ch, v, v * 100.0 / 65535))
            last_analog[ch] = v
    return ev


class Ctx:
    pass


def render(ctx):
    port, caps, cur = ctx.port, ctx.caps, ctx.cur
    times = ctx.poll_times
    hz = 0.0
    if len(times) > 1 and times[-1] > times[0]:
        hz = (len(times) - 1) / (times[-1] - times[0])
    rt = port.rtts
    avg = (sum(rt) / len(rt) * 1000.0) if rt else 0.0
    mx = (max(rt) * 1000.0) if rt else 0.0
    L = []
    L.append("JVS monitor  %s   (Ctrl-C to quit)" % ctx.args.device)
    L.append("board: %s" % ctx.info["id"])
    L.append("poll %5.1f Hz | rtt avg %4.1f ms max %5.1f ms | timeouts %d | "
             "crc %d | re-inits %d" % (hz, avg, mx, port.timeouts,
                                       port.crc_errors, ctx.reinits))
    L.append("last error: %s" % ctx.last_error if ctx.last_error else "last error: -")
    L.append("")
    L.append("SYSTEM   %02X           %s" % (
        cur["system"], " ".join(bit_names(cur["system"], SYS_NAMES)) or "-"))
    for p, pl in enumerate(cur["players"]):
        names = []
        for idx, b in enumerate(pl):
            names += byte_names(idx, b)
        L.append("%-8s %-12s %s" % (player_label(p), " ".join("%02X" % b for b in pl),
                                    " ".join(names) or "-"))
    for i, (cond, count) in enumerate(cur["coins"]):
        L.append("COIN %-3d count %5d   condition %s" % (i + 1, count, COIN_COND.get(cond, "?")))
    if cur["analog"]:
        L.append("")
        L.append("ANALOG   raw     scaled(%d-bit)                                     min..max seen" % caps["abits"])
        for ch, v in enumerate(cur["analog"]):
            lo, hi = ctx.amin[ch], ctx.amax[ch]
            L.append("ch%-2d    %04X  %6d  %5.1f%% [%s]  %04X..%04X" % (
                ch, v, v >> (16 - caps["abits"]), v * 100.0 / 65535, bar(v), lo, hi))
    sys.stdout.write("\x1b[H" + "".join(line + "\x1b[K\n" for line in L) + "\x1b[J")
    sys.stdout.flush()


def latency_check(path):
    name = os.path.basename(os.path.realpath(path))
    p = "/sys/bus/usb-serial/devices/%s/latency_timer" % name
    try:
        with open(p) as f:
            v = int(f.read().strip())
    except (OSError, ValueError):
        return
    if v > 2:
        print("WARNING: FTDI latency_timer is %d ms (want 1). Fix: "
              "echo 1 | sudo tee %s" % (v, p))
    else:
        print("FTDI latency_timer: %d ms (ok)" % v)


def fail_hint():
    print("\nNo valid response from the I/O board. Check:")
    print("  - jvscore / the game are not running (only one bus master)")
    print("  - A+ -> JVS D+, B- -> JVS D-, GND connected; try swapping A/B")
    print("  - the board is powered by the cabinet supply (not by USB)")
    print("  - adapter has automatic TX/RX direction (FTDI-based is best)")
    print("  - try --no-reset if the board is already addressed, or a longer --reset-delay 1.5")


def main():
    ap = argparse.ArgumentParser(description="JVS I/O board debugger")
    ap.add_argument("device", help="serial device, e.g. /dev/ttyUSB0")
    ap.add_argument("--info", action="store_true", help="print ID/capabilities and exit")
    ap.add_argument("--log", action="store_true", help="print only changes, timestamped")
    ap.add_argument("--main-id", metavar="STR",
                    help="send main board ID string (cmd 0x15) after addressing, "
                         "e.g. 'namco ltd.;N2;Ver0.50;JPN'")
    ap.add_argument("--no-reset", action="store_true",
                    help="skip bus reset/address assignment (board already addressed as 1)")
    ap.add_argument("--reset-delay", type=float, default=0.5)
    ap.add_argument("--players", type=int, help="override number of players polled")
    ap.add_argument("--sw-bytes", type=int, help="override switch bytes per player")
    ap.add_argument("--threshold", type=int, default=1024,
                    help="--log: min raw analog change to report (default 1024)")
    ap.add_argument("--timeout", type=float, default=0.1, help="response timeout seconds")
    ap.add_argument("--interval", type=float, default=0.002, help="sleep between polls")
    ap.add_argument("--count", type=int, help="stop after N polls")
    args = ap.parse_args()

    try:
        port = Port(args.device)
    except OSError as e:
        print("Cannot open %s: %s" % (args.device, e))
        print("(permissions? add your user to the 'dialout' group)")
        return 1
    latency_check(args.device)

    try:
        init_bus(port, args, do_reset=not args.no_reset)
        info = read_info(port)
    except JvsError as e:
        print("Init failed: %s" % e)
        fail_hint()
        return 1

    print_info(info)
    if args.main_id is not None:
        try:
            d, _ = port.request(1, [CMD_MAINID] + list(args.main_id.encode("latin-1")) + [0])
            need_report(d, 1)
            print("Main board ID accepted.")
        except JvsError as e:
            print("Main board ID command failed: %s" % e)
    if args.info:
        return 0

    caps = caps_from(info, args)
    req = build_request(caps)
    print("\nPolling: %d player(s) x %d byte(s), %d coin slot(s), %d analog ch. "
          "Starting in 1s..." % (caps["players"], caps["sw_bytes"], caps["coins"], caps["analog"]))
    time.sleep(1.0)

    ctx = Ctx()
    ctx.port, ctx.args, ctx.info, ctx.caps = port, args, info, caps
    ctx.cur, ctx.prev = None, None
    ctx.reinits, ctx.last_error = 0, None
    ctx.poll_times = collections.deque(maxlen=200)
    ctx.amin = [0xFFFF] * caps["analog"]
    ctx.amax = [0] * caps["analog"]
    last_analog = [0] * caps["analog"]
    fails, polls, next_render = 0, 0, 0.0

    if not args.log:
        sys.stdout.write("\x1b[2J\x1b[?25l")
    try:
        while True:
            try:
                d, _ = port.request(1, req, timeout=args.timeout)
                ctx.cur = parse_poll(d, caps)
                if fails and args.log:
                    print("%s LINK recovered" % stamp())
                fails = 0
            except JvsError as e:
                fails += 1
                ctx.last_error = "%s: %s" % (stamp(), e)
                if args.log and fails == 1:
                    print("%s LINK error: %s" % (stamp(), e))
                if fails >= 5:
                    if args.log:
                        print("%s LINK re-initialising bus" % stamp())
                    try:
                        init_bus(port, args, do_reset=True)
                        ctx.info = read_info(port)
                        ctx.reinits += 1
                    except JvsError:
                        pass
                    fails = 0
                time.sleep(0.02)
                continue

            polls += 1
            now = time.monotonic()
            ctx.poll_times.append(now)
            for ch, v in enumerate(ctx.cur["analog"]):
                ctx.amin[ch] = min(ctx.amin[ch], v)
                ctx.amax[ch] = max(ctx.amax[ch], v)

            if args.log:
                if ctx.prev is None:
                    last_analog[:] = ctx.cur["analog"]
                    print("%s (baseline taken; now reporting changes)" % stamp())
                for line in diff_events(ctx.prev, ctx.cur, last_analog, args.threshold):
                    print("%s %s" % (stamp(), line))
                sys.stdout.flush()
            elif now >= next_render:
                render(ctx)
                next_render = now + 0.05
            ctx.prev = ctx.cur

            if args.count and polls >= args.count:
                break
            time.sleep(args.interval)
    except KeyboardInterrupt:
        pass
    finally:
        if not args.log:
            sys.stdout.write("\x1b[?25h\n")
        print("polls %d, timeouts %d, crc errors %d, re-inits %d" % (
            polls, port.timeouts, port.crc_errors, ctx.reinits))
        port.close()
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:       # e.g. piped into head/grep
        os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno())
        sys.exit(0)
