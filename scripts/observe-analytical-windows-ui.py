#!/usr/bin/env python3
"""Bounded uninstrumented Windows UI evidence, never visual/release acceptance.

Only the launched, exact-hash simulator HWND receives targeted Win32 messages.
No global SendInput, desktop capture, OCR service, focus bypass or security change.
PrintWindow client pixels and Windows OCR must actually work or the run blocks.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import time
import zlib

SPEC = importlib.util.spec_from_file_location('ui_qualification', Path(__file__).with_name('qualify-analytical-swift-windows.py'))
q = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(q)
reviewed = q.load('ui_reviewed_bundle', 'recheck-reviewed-analytical-bundle.py')
require = q.require
IDENTITY = 'analytical-swift-win32-ui-observation-v1'
MAX_SECONDS = 1200
MAX_PNG_BYTES = 16 * 1024 * 1024
MAX_EXPORT_BYTES = 96 * 1024 * 1024
MAX_LOG_BYTES = 32 * 1024 * 1024
VIEW_NAMES = ('COCKPIT', 'CHASE', 'FREE', 'TOWER')
KEYS = {'map': 0x4d, 'camera': 0x43, 'escape': 0x1b, 'enter': 0x0d, 'restart': 0x52, 'replay-pause': 0x74}
LIVE_STEPS = ('map-initial', 'map-edit-alps', 'cancel-to-flight', 'map-reopened', 'map-edit-tokyo',
              'start-once', 'paused', 'paused-map', 'paused-map-edit', 'cancel-keeps-paused', 'resumed',
              'restarted', 'view-chase', 'view-free', 'view-tower', 'view-cockpit', 'resize-1024', 'resize-640',
              'resize-restored', 'cancel-attempt-map', 'cancel-attempt-tokyo', 'cancel-after-start-observation')
LEGACY_STEPS = ('legacy-initial', 'legacy-chase', 'legacy-map', 'legacy-return', 'legacy-1024', 'legacy-640', 'legacy-restored')
CHECKPOINTS = {
    'map-initial': ('map-tokyo', None, 1280, 900), 'map-edit-alps': ('map-alps', None, 1280, 900),
    'cancel-to-flight': ('flight', 'COCKPIT', 1280, 900), 'map-reopened': ('map', None, 1280, 900),
    'map-edit-tokyo': ('map-tokyo', None, 1280, 900), 'start-once': ('flight', 'COCKPIT', 1280, 900),
    'paused': ('paused', None, 1280, 900), 'paused-map': ('map', None, 1280, 900),
    'paused-map-edit': ('map-alps', None, 1280, 900), 'cancel-keeps-paused': ('paused', None, 1280, 900),
    'resumed': ('flight', 'COCKPIT', 1280, 900), 'restarted': ('flight', 'COCKPIT', 1280, 900),
    **{'view-' + view.lower(): ('paused-view', view, 1280, 900) for view in VIEW_NAMES},
    'resize-1024': ('paused-view', 'COCKPIT', 1024, 720), 'resize-640': ('paused-view', 'COCKPIT', 640, 480),
    'resize-restored': ('paused-view', 'COCKPIT', 1280, 900), 'cancel-attempt-map': ('map', None, 1280, 900),
    'cancel-attempt-tokyo': ('map-tokyo', None, 1280, 900), 'cancel-after-start-observation': ('paused', None, 1280, 900),
    'legacy-initial': ('legacy', 'COCKPIT', 1280, 900), 'legacy-chase': ('legacy', 'CHASE', 1280, 900),
    'legacy-map': ('legacy-map', None, 1280, 900), 'legacy-return': ('legacy', 'CHASE', 1280, 900),
    'legacy-1024': ('legacy', 'CHASE', 1024, 720), 'legacy-640': ('legacy', 'CHASE', 640, 480),
    'legacy-restored': ('legacy', 'CHASE', 1280, 900),
}
LIMITS = ['message_driven_input_not_physical_keyboard_or_controller', 'ocr_is_state_evidence_not_visual_quality_acceptance',
          'printwindow_not_batch_screenshot_readback_proof', 'physical_gpu_audio_unqualified',
          'pending_start_cancellation_may_race_and_remains_separate', 'publication_and_rights_not_approved']


def write_png(path, width, height, bgra):
    require(640 <= width <= 4096 and 360 <= height <= 4096 and len(bgra) == width * height * 4, 'invalid client pixels')
    # Encode the native capture directly. No external desktop crop or image edit.
    raw = bytearray()
    colors = set()
    for y in range(height):
        raw.append(0)
        row = bgra[y * width * 4:(y + 1) * width * 4]
        for x in range(0, len(row), 4):
            rgb = bytes((row[x + 2], row[x + 1], row[x]))
            raw.extend(rgb)
            if len(colors) < 32: colors.add(rgb)
    require(len(colors) >= 16, 'client capture is blank or unsupported')
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    encoded = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
    encoded += chunk(b'IDAT', zlib.compress(bytes(raw), 6)) + chunk(b'IEND', b'')
    require(len(encoded) <= MAX_PNG_BYTES, 'client PNG exceeds budget')
    with path.open('xb') as out: out.write(encoded)
    return q.candidate.validate_png(path)


class NativeWindow:
    """Lazy native API boundary; construction is forbidden on other platforms."""
    def __init__(self, pid, executable):
        require(sys.platform == 'win32', 'native Windows desktop required')
        self.pid, self.executable = pid, executable.resolve()
        self.u = ctypes.WinDLL('user32', use_last_error=True)
        self.k = ctypes.WinDLL('kernel32', use_last_error=True)
        self.g = ctypes.WinDLL('gdi32', use_last_error=True)
        self.hwnd = None
        self.callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        self._types()
        if hasattr(self.u, 'SetProcessDpiAwarenessContext'):
            self.u.SetProcessDpiAwarenessContext.argtypes = [ctypes.c_void_p]
            self.u.SetProcessDpiAwarenessContext.restype = wintypes.BOOL
            self.u.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))
        self.assert_desktop()
        self.assert_process()

    def _types(self):
        declarations = {
            'GetWindowThreadProcessId': ([wintypes.HWND, ctypes.POINTER(wintypes.DWORD)], wintypes.DWORD),
            'EnumWindows': ([self.callback_type, wintypes.LPARAM], wintypes.BOOL),
            'IsWindow': ([wintypes.HWND], wintypes.BOOL), 'IsWindowVisible': ([wintypes.HWND], wintypes.BOOL),
            'GetWindow': ([wintypes.HWND, wintypes.UINT], wintypes.HWND),
            'GetForegroundWindow': ([], wintypes.HWND), 'SetForegroundWindow': ([wintypes.HWND], wintypes.BOOL),
            'GetClientRect': ([wintypes.HWND, ctypes.POINTER(wintypes.RECT)], wintypes.BOOL),
            'GetWindowRect': ([wintypes.HWND, ctypes.POINTER(wintypes.RECT)], wintypes.BOOL),
            'SendMessageTimeoutW': ([wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM, wintypes.UINT, wintypes.UINT,
                                    ctypes.POINTER(ctypes.c_size_t)], wintypes.LPARAM),
            'MapVirtualKeyW': ([wintypes.UINT, wintypes.UINT], wintypes.UINT),
            'GetDC': ([wintypes.HWND], wintypes.HDC), 'ReleaseDC': ([wintypes.HWND, wintypes.HDC], ctypes.c_int),
            'PrintWindow': ([wintypes.HWND, wintypes.HDC, wintypes.UINT], wintypes.BOOL),
            'GetWindowLongW': ([wintypes.HWND, ctypes.c_int], wintypes.LONG),
            'AdjustWindowRectEx': ([ctypes.POINTER(wintypes.RECT), wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.BOOL),
            'SetWindowPos': ([wintypes.HWND, wintypes.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wintypes.UINT], wintypes.BOOL),
            'OpenInputDesktop': ([wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
            'CloseDesktop': ([wintypes.HANDLE], wintypes.BOOL),
            'GetUserObjectInformationW': ([wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD, ctypes.POINTER(wintypes.DWORD)], wintypes.BOOL),
        }
        for name, (args, result) in declarations.items():
            function = getattr(self.u, name); function.argtypes = args; function.restype = result
        self.k.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]; self.k.OpenProcess.restype = wintypes.HANDLE
        self.k.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
        self.k.QueryFullProcessImageNameW.restype = wintypes.BOOL
        self.k.CloseHandle.argtypes = [wintypes.HANDLE]; self.k.CloseHandle.restype = wintypes.BOOL
        self.g.CreateCompatibleDC.argtypes = [wintypes.HDC]; self.g.CreateCompatibleDC.restype = wintypes.HDC
        self.g.CreateDIBSection.argtypes = [wintypes.HDC, ctypes.c_void_p, wintypes.UINT, ctypes.POINTER(ctypes.c_void_p), wintypes.HANDLE, wintypes.DWORD]
        self.g.CreateDIBSection.restype = wintypes.HBITMAP
        self.g.SelectObject.argtypes = [wintypes.HDC, wintypes.HGDIOBJ]; self.g.SelectObject.restype = wintypes.HGDIOBJ
        self.g.DeleteObject.argtypes = [wintypes.HGDIOBJ]; self.g.DeleteObject.restype = wintypes.BOOL
        self.g.DeleteDC.argtypes = [wintypes.HDC]; self.g.DeleteDC.restype = wintypes.BOOL

    def assert_desktop(self):
        desktop = self.u.OpenInputDesktop(0, False, 1)
        require(desktop, 'interactive input desktop unavailable; no service/locked-desktop fallback')
        try:
            name = ctypes.create_unicode_buffer(128); size = wintypes.DWORD()
            require(self.u.GetUserObjectInformationW(desktop, 2, name, ctypes.sizeof(name), ctypes.byref(size))
                    and name.value.casefold() == 'default', 'secure or unsupported desktop; no switch attempted')
        finally: self.u.CloseDesktop(desktop)

    def assert_process(self):
        handle = self.k.OpenProcess(0x1000, False, self.pid)
        require(handle, 'launched process unavailable')
        try:
            name = ctypes.create_unicode_buffer(32768); length = wintypes.DWORD(len(name))
            require(self.k.QueryFullProcessImageNameW(handle, 0, name, ctypes.byref(length)), 'cannot verify process image')
            require(Path(name.value).resolve() == self.executable, 'process image is not the bound simulator')
        finally: self.k.CloseHandle(handle)

    def owner(self, hwnd):
        pid = wintypes.DWORD()
        require(self.u.GetWindowThreadProcessId(hwnd, ctypes.byref(pid)), 'window disappeared')
        return pid.value

    def find(self):
        self.assert_process(); windows = []
        def visit(hwnd, _):
            pid = wintypes.DWORD()
            self.u.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == self.pid and self.u.IsWindowVisible(hwnd) and not self.u.GetWindow(hwnd, 4): windows.append(hwnd)
            return True
        callback = self.callback_type(visit)
        require(self.u.EnumWindows(callback, 0), 'window enumeration failed')
        require(len(windows) <= 1, 'ambiguous simulator windows')
        if windows: self.hwnd = windows[0]
        return self.hwnd

    def assert_owned(self, foreground=True):
        self.assert_desktop(); self.assert_process()
        require(self.hwnd and self.u.IsWindow(self.hwnd) and self.u.IsWindowVisible(self.hwnd)
                and self.owner(self.hwnd) == self.pid, 'simulator window ownership changed')
        if foreground: require(self.u.GetForegroundWindow() == self.hwnd, 'simulator lost foreground; input stopped')

    def focus(self):
        self.assert_owned(False)
        self.u.SetForegroundWindow(self.hwnd)
        self.assert_owned()  # No AttachThreadInput, synthetic Alt or focus-policy bypass.

    def message(self, message, key=0, detail=0):
        self.assert_owned(); result = ctypes.c_size_t()
        require(self.u.SendMessageTimeoutW(self.hwnd, message, key, detail, 0x2 | 0x20, 2000, ctypes.byref(result)),
                'owned-window message timed out or was rejected')
        self.assert_owned()

    def key(self, name):
        require(name in KEYS, 'key outside fixed UI vocabulary')
        key = KEYS[name]; scan = self.u.MapVirtualKeyW(key, 0)
        require(scan, 'key has no native scan mapping')
        self.message(0x100, key, 1 | (scan << 16))
        time.sleep(0.12)
        self.message(0x101, key, 1 | (scan << 16) | (1 << 30) | (1 << 31))

    def close(self):
        self.assert_owned(); result = ctypes.c_size_t()
        # A successfully delivered close can destroy the HWND before the call
        # returns. The owner's actual process exit, not a surviving HWND, is the
        # later normal-shutdown witness.
        sent = self.u.SendMessageTimeoutW(self.hwnd, 0x10, 0, 0, 0x2 | 0x20, 2000, ctypes.byref(result))
        require(sent or not self.u.IsWindow(self.hwnd), 'normal close request timed out')

    def click(self, x, y, expected_size):
        self.assert_owned(); rect = wintypes.RECT()
        require(self.u.GetClientRect(self.hwnd, ctypes.byref(rect)) and (rect.right, rect.bottom) == expected_size,
                'client changed after OCR; click refused')
        require(type(x) is int and type(y) is int and 0 <= x < rect.right and 0 <= y < rect.bottom,
                'click outside verified client')
        detail = x | (y << 16)
        self.message(0x200, 0, detail)  # HWND-targeted client pointer, never global mouse input.
        self.message(0x201, 1, detail)
        time.sleep(0.12)
        self.message(0x202, 0, detail)

    def resize(self, width, height):
        require((width, height) in ((1280, 900), (1024, 720), (640, 480)), 'viewport outside fixed qualification sizes')
        self.assert_owned()
        client = wintypes.RECT(0, 0, width, height)
        require(self.u.AdjustWindowRectEx(ctypes.byref(client), self.u.GetWindowLongW(self.hwnd, -16), False,
                                           self.u.GetWindowLongW(self.hwnd, -20)), 'client-size calculation failed')
        require(self.u.SetWindowPos(self.hwnd, None, 0, 0, client.right - client.left, client.bottom - client.top,
                                    0x2 | 0x4 | 0x10), 'simulator resize failed')

    def capture(self, path):
        self.assert_owned()
        rect = wintypes.RECT(); require(self.u.GetClientRect(self.hwnd, ctypes.byref(rect)), 'cannot read client rectangle')
        width, height = rect.right, rect.bottom
        require(640 <= width <= 4096 and 360 <= height <= 4096, 'client outside capture bounds')
        dc = self.u.GetDC(self.hwnd); require(dc, 'client DC unavailable')
        memory = self.g.CreateCompatibleDC(dc); bitmap = None; previous = None
        try:
            require(memory, 'capture DC unavailable')
            # Top-down BGRA DIB: no desktop coordinates or screen pixels are read.
            header = struct.pack('<IiiHHIIiiII', 40, width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0)
            info = ctypes.create_string_buffer(header); bits = ctypes.c_void_p()
            bitmap = self.g.CreateDIBSection(dc, info, 0, ctypes.byref(bits), None, 0)
            require(bitmap and bits.value, 'capture bitmap unavailable')
            previous = self.g.SelectObject(memory, bitmap); require(previous, 'capture bitmap selection failed')
            require(self.u.PrintWindow(self.hwnd, memory, 1), 'client PrintWindow unsupported')
            self.assert_owned()
            return write_png(path, width, height, ctypes.string_at(bits.value, width * height * 4))
        finally:
            if previous: self.g.SelectObject(memory, previous)
            if bitmap: self.g.DeleteObject(bitmap)
            if memory: self.g.DeleteDC(memory)
            self.u.ReleaseDC(self.hwnd, dc)


def ocr_value(raw, width, height):
    require(len(raw) <= 512 * 1024, 'OCR result exceeds budget')
    value = json.loads(raw.decode('utf-8-sig'))
    require(set(value) == {'schema_version', 'engine', 'language', 'text', 'words'} and value['schema_version'] == 1
            and value['engine'] == 'Windows.Media.Ocr' and value['language'] == 'en-US', 'unexpected OCR engine/schema')
    require(isinstance(value['text'], str) and len(value['text']) <= 32768 and isinstance(value['words'], list)
            and len(value['words']) <= 2048, 'invalid OCR bounds')
    for word in value['words']:
        require(set(word) == {'text', 'x', 'y', 'width', 'height'} and isinstance(word['text'], str)
                and len(word['text']) <= 256, 'invalid OCR word')
        numbers = [word[key] for key in ('x', 'y', 'width', 'height')]
        require(all(type(item) in (int, float) and math.isfinite(item) and item >= 0 for item in numbers)
                and numbers[0] + numbers[2] <= width + 1 and numbers[1] + numbers[3] <= height + 1, 'OCR box escapes captured client')
    return value


def text_state(text, state, view=None):
    plain = ' '.join(text.upper().split())
    is_map = 'WORLD EXPLORER' in plain and 'START NEW FLIGHT' in plain
    hud = all(re.search(r'\b' + item + r'\b', plain) for item in ('EAS', 'ALT'))
    paused = 'PAUSED' in plain
    if state == 'map': return is_map
    if state == 'map-alps': return is_map and re.search(r'LAT\s*\+?46\.58000', plain) is not None and re.search(r'LON\s*\+?8\.00000', plain) is not None
    if state == 'map-tokyo': return is_map and re.search(r'LAT\s*\+?35\.55000', plain) is not None and re.search(r'LON\s*\+?139\.78000', plain) is not None
    if state == 'paused': return hud and paused and not is_map
    if state == 'paused-view': return hud and paused and not is_map and re.search(r'VIEW\s+' + view + r'\b', plain)
    if state == 'flight': return hud and not paused and not is_map and (view is None or re.search(r'VIEW\s+' + view + r'\b', plain))
    legacy_notice = all(item in plain for item in ('LEGACY PARTIAL IDENTITY', 'YAW_RATE_P', 'NOT RECORDED OR VERIFIED'))
    if state == 'legacy': return not is_map and legacy_notice and (view is None or re.search(r'VIEW\s+' + view + r'\b', plain))
    if state == 'legacy-map': return is_map and legacy_notice and 'AIRCRAFT LOCKED' in plain
    raise ValueError('unrecognized UI observation state')


def read_log(private):
    return log_snapshot(private)[0]


def log_snapshot(private):
    paths = [private / 'app.stdout', private / 'app.stderr']
    require(all(path.stat().st_size <= MAX_LOG_BYTES for path in paths), 'application log budget exceeded')
    contents = {path.name: path.read_bytes() for path in paths}
    require(all(len(data) <= MAX_LOG_BYTES for data in contents.values()), 'application log grew beyond budget')
    plain = q.candidate.ANSI.sub('', '\n'.join(data.decode('utf-8', errors='replace') for data in contents.values()))
    require(not re.search(r'(?m)(^|\s)ERROR(\s|:|$)|(?im:thread .+ panicked at|panic(?:ked)? at|Failed to load asset|unregistered type)', plain),
            'application logged runtime failure')
    return plain, {name: {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)} for name, data in contents.items()}


def log_counts(text):
    return {'world_flight': len(re.findall(r'world flight: ', text)),
            'swift_commits': len(re.findall(r'new-flight aircraft: [^\r\n]*\(swift-sport\); model ', text)),
            'paused': len(re.findall(r'(?m)(?:^|\s)paused\s*$', text)),
            'resumed': len(re.findall(r'(?m)(?:^|\s)resumed\s*$', text)),
            'restarted': len(re.findall(r'(?m)(?:^|\s)restarted\s*$', text))}


def scenario_command(executable, qualification_private, scenario):
    if scenario == 'live':
        return [str(executable), '--world-map', '--view', 'cockpit', '--time', '12:00', '--weather', 'clear']
    require(scenario == 'legacy', 'unknown launch scenario')
    return [str(executable), '--aircraft', 'light-single', '--no-model', '--legacy-replay-compatibility',
            '--replay', str(qualification_private / 'legacy.fsreplay'), '--view', 'cockpit']


def validate_sequence(rows, scenario, cancellation, complete):
    """The validator repeats the scenario's observed invariants, not caller labels."""
    previous = 0
    count_keys = {'world_flight', 'swift_commits', 'paused', 'resumed', 'restarted'}
    for row in rows:
        require(type(row['attempt']) is int and previous < row['attempt'] <= 128, 'UI attempts must be distinct and increasing')
        previous = row['attempt']
        name, counts = row['checkpoint'], row['log_counts']
        require(isinstance(counts, dict) and set(counts) == count_keys
                and all(type(count) is int and 0 <= count <= 1_000_000 for count in counts.values()), 'invalid bounded UI log counts')
        if not complete: continue
        if scenario == 'legacy':
            require(counts['world_flight'] == counts['swift_commits'] == counts['restarted'] == 0,
                    'legacy observation unexpectedly created/restarted a flight')
            continue
        index = LIVE_STEPS.index(name)
        expected = {'world_flight': 0, 'swift_commits': 0, 'paused': 0, 'resumed': 0, 'restarted': 0}
        if index >= LIVE_STEPS.index('start-once'): expected.update(world_flight=1, swift_commits=1)
        if index >= LIVE_STEPS.index('paused'): expected['paused'] = 1
        if index >= LIVE_STEPS.index('resumed'): expected['resumed'] = 1
        if index >= LIVE_STEPS.index('restarted'): expected['restarted'] = 1
        if index >= LIVE_STEPS.index('view-chase'): expected['paused'] = 2
        if name == 'cancel-after-start-observation':
            if cancellation == 'commit_won_race_cancellation_not_established':
                expected.update(world_flight=2, swift_commits=2, paused=3)
            else:
                require(cancellation == 'no_commit_after_cancel_observed_preparation_phase_not_proven', 'missing actual cancellation observation')
        require(counts == expected, 'checkpoint log witnesses disagree with the fixed lifecycle sequence')
    if complete:
        require(cancellation == 'legacy_partial_identity_only' if scenario == 'legacy' else cancellation in (
                'commit_won_race_cancellation_not_established', 'no_commit_after_cancel_observed_preparation_phase_not_proven'),
                'scenario and completed cancellation/identity observation disagree')


class Observer:
    def __init__(self, repo, private, process, window, scenario):
        self.repo, self.private, self.process, self.window, self.scenario = repo, private, process, window, scenario
        self.started = time.monotonic(); self.rows = []; self.number = 0
        self.powershell = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
        self.last_ocr = None

    def click_destination(self, label):
        require(label in ('ALPS', 'TOKYO') and self.last_ocr is not None, 'destination outside observed UI vocabulary')
        actual, pixels = self.last_ocr
        require(text_state(actual['text'], 'map'), 'destination click requires an observed plain map')
        words = [word for word in actual['words'] if word['text'].upper() == label]
        require(len(words) == 1, 'destination label is absent or ambiguous; no coordinate guess')
        word = words[0]
        self.window.click(int(word['x'] + word['width'] / 2), int(word['y'] + word['height'] / 2),
                          (pixels['width'], pixels['height']))
        self.last_ocr = None

    def observe(self, name, state, view=None, timeout=45):
        require(name in (LIVE_STEPS if self.scenario == 'live' else LEGACY_STEPS), 'checkpoint outside fixed scenario')
        require((state, view) == CHECKPOINTS[name][:2], 'checkpoint must use its fixed state and view')
        deadline = min(self.started + MAX_SECONDS, time.monotonic() + timeout)
        while time.monotonic() < deadline:
            require(self.process.poll() is None, 'simulator exited before observation')
            self.window.assert_owned(); self.number += 1
            require(self.number <= 128, 'capture attempt budget exhausted')
            attempt = self.private / ('frame-' + str(self.number)); attempt.mkdir()
            image = attempt / 'client.png'
            # PrintWindow can block in the target WndProc. A separate helper has
            # a hard deadline and may be killed without claiming capture success.
            command = [sys.executable, str(Path(__file__).resolve()), '--capture-helper', '--pid', str(self.process.pid),
                       '--hwnd', str(self.window.hwnd), '--executable', str(self.window.executable), '--output', str(image)]
            result = q.capture.execute(command, cwd=self.repo, env=os.environ.copy(), stdout=attempt / 'capture.stdout',
                                       stderr=attempt / 'capture.stderr', journal=attempt / 'capture-command.json', timeout=10)
            require(result['outcome'] == 'succeeded', 'owned client capture unavailable; no screen fallback')
            pixels = q.candidate.validate_png(image)
            result = q.capture.execute([str(self.powershell), '-NoProfile', '-NonInteractive', '-File',
                       str(self.repo / 'scripts/windows-ui-ocr.ps1'), '-Image', str(image)], cwd=self.repo, env=os.environ.copy(),
                       stdout=attempt / 'ocr.json', stderr=attempt / 'ocr.stderr', journal=attempt / 'ocr-command.json', timeout=25)
            require(result['outcome'] == 'succeeded', 'local OCR unavailable; no cloud OCR fallback')
            actual = ocr_value((attempt / 'ocr.json').read_bytes(), pixels['width'], pixels['height'])
            log, prefixes = log_snapshot(self.private); counts = log_counts(log)
            if (pixels['width'], pixels['height']) == CHECKPOINTS[name][2:] and text_state(actual['text'], state, view):
                self.last_ocr = (actual, pixels)
                row = {'checkpoint': name, 'state': state, 'view': view, 'attempt': self.number,
                       'image': pixels, 'ocr': q.record(attempt / 'ocr.json'), 'log_counts': counts,
                       'log_prefixes': prefixes,
                       'source': 'owned_client_PrintWindow_and_Windows_OCR', 'visual_acceptance': False}
                self.rows.append(row); q.write_json(self.private / 'observations.json', self.rows)
                return counts
            time.sleep(min(1.0, max(0, deadline - time.monotonic())))
        raise ValueError('expected observable UI state was not established')


def live_sequence(observer):
    w = observer.window
    base = observer.observe('map-initial', 'map-tokyo', timeout=180)
    require(base['world_flight'] == base['swift_commits'] == 0, 'unexpected flight commit before interaction')
    observer.click_destination('ALPS'); observer.observe('map-edit-alps', 'map-alps')
    w.key('escape'); after = observer.observe('cancel-to-flight', 'flight', 'COCKPIT')
    require(after['world_flight'] == after['swift_commits'] == 0 and after['paused'] == 0, 'map cancellation changed flight/pause')
    w.key('map'); observer.observe('map-reopened', 'map')
    observer.click_destination('TOKYO'); observer.observe('map-edit-tokyo', 'map-tokyo')
    w.key('enter'); w.key('enter')
    after = observer.observe('start-once', 'flight', 'COCKPIT', timeout=90)
    require(after['world_flight'] == after['swift_commits'] == 1, 'repeated Start did not commit exactly one Swift flight')
    w.key('escape'); counts = observer.observe('paused', 'paused')
    require(counts['paused'] == 1 and counts['resumed'] == 0, 'pause log witness missing')
    w.key('map'); observer.observe('paused-map', 'map')
    observer.click_destination('ALPS'); observer.observe('paused-map-edit', 'map-alps')
    w.key('escape'); counts = observer.observe('cancel-keeps-paused', 'paused')
    require(counts['world_flight'] == counts['swift_commits'] == 1 and counts['paused'] == 1 and counts['resumed'] == 0,
            'paused map cancellation altered flight or pause')
    w.key('escape'); counts = observer.observe('resumed', 'flight', 'COCKPIT')
    require(counts['resumed'] == 1, 'resume log witness missing')
    w.key('restart'); counts = observer.observe('restarted', 'flight', 'COCKPIT')
    require(counts['restarted'] == 1, 'restart log witness missing')
    w.key('escape')  # Keep the restarted aircraft stable during slower UI/OCR checks.
    for view in ('CHASE', 'FREE', 'TOWER', 'COCKPIT'):
        w.key('camera'); observer.observe('view-' + view.lower(), 'paused-view', view)
    for name, size in (('resize-1024', (1024, 720)), ('resize-640', (640, 480)), ('resize-restored', (1280, 900))):
        w.resize(*size); observer.observe(name, 'paused-view', 'COCKPIT')
    # One bounded Start/Cancel race. Do not claim cancellation during preparation
    # if the commit won the race, even when the application remains healthy.
    w.key('map'); observer.observe('cancel-attempt-map', 'map')
    observer.click_destination('TOKYO'); observer.observe('cancel-attempt-tokyo', 'map-tokyo')
    w.key('enter'); w.key('escape')
    counts = observer.observe('cancel-after-start-observation', 'paused', timeout=90)
    require(counts['world_flight'] == counts['swift_commits'] and counts['world_flight'] in (1, 2), 'unexpected Start/Cancel outcome')
    return 'no_commit_after_cancel_observed_preparation_phase_not_proven' if counts['world_flight'] == 1 else 'commit_won_race_cancellation_not_established'


def legacy_sequence(observer):
    w = observer.window
    observer.observe('legacy-initial', 'legacy', 'COCKPIT', timeout=180)
    w.key('camera'); observer.observe('legacy-chase', 'legacy', 'CHASE')
    w.key('map'); observer.observe('legacy-map', 'legacy-map')
    w.key('escape'); observer.observe('legacy-return', 'legacy', 'CHASE')
    for name, size in (('legacy-1024', (1024, 720)), ('legacy-640', (640, 480)), ('legacy-restored', (1280, 900))):
        w.resize(*size); observer.observe(name, 'legacy', 'CHASE')
    return 'legacy_partial_identity_only'


def run(repo, expected, qualification_private, qualification_export, private, scenario):
    require(sys.platform == 'win32', 'interactive native Windows executor required')
    for path in (repo, qualification_private, qualification_export, private): q.capture.no_links(path)
    q.capture.disjoint(repo, qualification_private, qualification_export, private)
    require(not private.exists(), 'fresh UI observation root required')
    previous = reviewed.verify_export(qualification_export, repo, expected, qualification_private)
    require(previous['status'] == 'final_bundle_runtime_observed_review_required',
            'the final supplemented extracted production runtime must succeed first')
    source = q.capture.source_evidence(repo, expected)
    bundle = qualification_private / 'extracted/swift-candidate'; executable = bundle / 'flightsim-app.exe'
    executable_record = q.record(executable)
    private.mkdir(parents=True); (private / 'unrelated-cwd').mkdir()
    result = {'schema_version': 1, 'identity': IDENTITY, 'source_sha': expected, 'source_tree': source['source_tree'],
              'scenario': scenario, 'status': 'blocked', 'executable': executable_record,
              'prior_runtime_evidence': q.record(qualification_export / reviewed.EXPORT), 'release_authorized': False,
              'appearance_accepted': False, 'lifecycle_accepted': False, 'observations': [], 'normal_close_exit': None,
              'cancellation_observation': 'not_established', 'limits': LIMITS, 'final_streams': None}
    process, observer = None, None
    with q.capture.private_console(private):
        try:
            NativeWindow(os.getpid(), Path(sys.executable))  # Desktop probe before launching any simulator scene.
            powershell = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
            probe = q.capture.execute([str(powershell), '-NoProfile', '-NonInteractive', '-File',
                     str(repo / 'scripts/windows-ui-ocr.ps1'), '-Probe'], cwd=repo, env=os.environ.copy(), stdout=private / 'ocr-probe.json',
                     stderr=private / 'ocr-probe.stderr', journal=private / 'ocr-probe-command.json', timeout=30)
            require(probe['outcome'] == 'succeeded' and json.loads((private / 'ocr-probe.json').read_text(encoding='utf-8-sig')) ==
                    {'schema_version': 1, 'engine': 'Windows.Media.Ocr', 'language': 'en-US', 'available': True}, 'required local OCR capability unavailable')
            env = {**os.environ, 'WGPU_BACKEND': 'dx12', 'WGPU_FORCE_FALLBACK_ADAPTER': '1', 'RUST_LOG': 'info',
                   'BEVY_ASSET_ROOT': str(repo), 'CARGO_MANIFEST_DIR': str(repo)}
            command = scenario_command(executable, qualification_private, scenario)
            if scenario == 'legacy':
                fixture = qualification_private / 'legacy.fsreplay'; q.candidate.legacy_identity(fixture)
            launch = {'command': command, 'cwd': str(private / 'unrelated-cwd'), 'executable': executable_record,
                      'pid': None, 'hwnd': None}
            q.write_json(private / 'launch.json', launch)
            with (private / 'app.stdout').open('xb') as out, (private / 'app.stderr').open('xb') as err:
                process = subprocess.Popen(command, cwd=private / 'unrelated-cwd', env=env, stdout=out, stderr=err,
                                           creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
                window = NativeWindow(process.pid, executable)
                deadline = time.monotonic() + 180
                while window.find() is None and process.poll() is None and time.monotonic() < deadline: time.sleep(0.25)
                require(process.poll() is None and window.hwnd is not None, 'simulator window did not become available')
                launch.update(pid=process.pid, hwnd=window.hwnd); q.write_json(private / 'launch.json', launch)
                window.focus(); window.resize(1280, 900)
                observer = Observer(repo, private, process, window, scenario)
                result['cancellation_observation'] = (live_sequence if scenario == 'live' else legacy_sequence)(observer)
                window.close()  # Actual WM_CLOSE; normal app shutdown, never batch/forced exit.
                result['normal_close_exit'] = process.wait(timeout=180)
                require(result['normal_close_exit'] == 0, 'normal window close did not exit 0')
            log = read_log(private)
            require('aircraft model fitted: 7.12 m' in log if scenario == 'live' else q.candidate.LEGACY_NOTICE in log,
                    'required model or legacy startup witness missing')
            require(q.record(executable) == executable_record and q.capture.source_evidence(repo, expected) == source, 'source/executable changed')
            reviewed.verify_export(qualification_export, repo, expected, qualification_private)
            result['status'] = 'observable_sequence_recorded_review_required'
        except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError, zlib.error) as error:
            q.write_json(private / 'failure.json', {'type': type(error).__name__, 'message': str(error)})
        finally:
            if process is not None and process.poll() is None:
                q.capture.stop_process_tree(process)  # Own failed process only; never normal-close evidence.
            if observer is not None: result['observations'] = observer.rows
            if (private / 'app.stdout').is_file() and (private / 'app.stderr').is_file():
                result['final_streams'] = {name: q.record(private / name) for name in ('app.stdout', 'app.stderr')}
            q.write_json(private / 'ui-observation.json', result)
    return result


def validate_observation(private, repo, expected, qualification_private, qualification_export):
    value = q.capture.read_private_json(private / 'ui-observation.json')
    require(set(value) == {'schema_version', 'identity', 'source_sha', 'source_tree', 'scenario', 'status', 'executable',
                         'prior_runtime_evidence', 'release_authorized', 'appearance_accepted', 'lifecycle_accepted',
                         'observations', 'normal_close_exit', 'cancellation_observation', 'limits', 'final_streams'}, 'unexpected UI evidence fields')
    require(type(value['schema_version']) is int and value['schema_version'] == 1 and value['identity'] == IDENTITY
            and value['source_sha'] == expected and value['scenario'] in ('live', 'legacy'), 'wrong UI evidence identity')
    require(all(value[key] is False for key in ('release_authorized', 'appearance_accepted', 'lifecycle_accepted'))
            and value['limits'] == LIMITS, 'UI evidence cannot grant acceptance')
    require(value['status'] in ('blocked', 'observable_sequence_recorded_review_required'), 'wrong UI evidence status')
    require(value['normal_close_exit'] is None or (type(value['normal_close_exit']) is int
            and -(2**32) <= value['normal_close_exit'] <= 2**32), 'normal close result must be a bounded actual integer or null')
    if value['final_streams'] is not None:
        require(isinstance(value['final_streams'], dict) and set(value['final_streams']) == {'app.stdout', 'app.stderr'}, 'invalid final stream set')
        for name, binding in value['final_streams'].items():
            require(q.capture.valid_record(binding) and binding['bytes'] <= MAX_LOG_BYTES and binding == q.record(private / name),
                    'complete final application stream changed')
    else:
        require(value['status'] == 'blocked' and not (private / 'app.stdout').exists() and not (private / 'app.stderr').exists(),
                'existing final streams may not be omitted')
    source = q.capture.source_evidence(repo, expected)
    prior = reviewed.verify_export(qualification_export, repo, expected, qualification_private)
    require(prior['status'] == 'final_bundle_runtime_observed_review_required' and value['source_tree'] == source['source_tree']
            and value['prior_runtime_evidence'] == q.record(qualification_export / reviewed.EXPORT)
            and value['executable'] == q.record(qualification_private / 'extracted/swift-candidate/flightsim-app.exe'), 'UI source/final bundle changed')
    steps = LIVE_STEPS if value['scenario'] == 'live' else LEGACY_STEPS
    require(isinstance(value['observations'], list) and len(value['observations']) <= len(steps), 'unbounded UI observations')
    for index, row in enumerate(value['observations']):
        require(set(row) == {'checkpoint', 'state', 'view', 'attempt', 'image', 'ocr', 'log_counts', 'log_prefixes', 'source', 'visual_acceptance'}
                and row['checkpoint'] == steps[index] and row['visual_acceptance'] is False
                and row['source'] == 'owned_client_PrintWindow_and_Windows_OCR' and type(row['attempt']) is int
                and 1 <= row['attempt'] <= 128, 'invalid UI checkpoint')
        require(row['view'] is None or row['view'] in VIEW_NAMES, 'invalid UI view')
        require((row['state'], row['view']) == CHECKPOINTS[row['checkpoint']][:2]
                and (row['image'].get('width'), row['image'].get('height')) == CHECKPOINTS[row['checkpoint']][2:],
                'checkpoint state/view/actual client dimensions differ from the fixed qualification case')
        attempt = private / ('frame-' + str(row['attempt']))
        launch = q.capture.read_private_json(private / 'launch.json')
        executable = qualification_private / 'extracted/swift-candidate/flightsim-app.exe'
        require(set(launch) == {'command', 'cwd', 'executable', 'pid', 'hwnd'}
                and launch['command'] == scenario_command(executable, qualification_private, value['scenario'])
                and launch['cwd'] == str(private / 'unrelated-cwd')
                and launch['executable'] == q.record(executable) and type(launch['pid']) is int and launch['pid'] > 0
                and type(launch['hwnd']) is int and launch['hwnd'] > 0, 'missing launched process/window binding')
        helper = [sys.executable, str(Path(__file__).resolve()), '--capture-helper', '--pid', str(launch['pid']),
                  '--hwnd', str(launch['hwnd']), '--executable', str(executable), '--output', str(attempt / 'client.png')]
        capture_journal = q.capture.read_private_json(attempt / 'capture-command.json')
        require(capture_journal['command'] == helper and capture_journal['cwd'] == str(repo)
                and capture_journal['outcome'] == 'succeeded' and capture_journal['exit_code'] == 0,
                'capture is not bound to the launched simulator window')
        require(capture_journal['stdout'] == q.record(attempt / 'capture.stdout')
                and capture_journal['stderr'] == q.record(attempt / 'capture.stderr'), 'capture helper streams changed')
        ocr_journal = q.capture.read_private_json(attempt / 'ocr-command.json')
        powershell = Path(os.environ['SystemRoot']) / 'System32/WindowsPowerShell/v1.0/powershell.exe'
        require(ocr_journal['command'] == [str(powershell), '-NoProfile', '-NonInteractive', '-File',
                str(repo / 'scripts/windows-ui-ocr.ps1'), '-Image', str(attempt / 'client.png')]
                and ocr_journal['cwd'] == str(repo) and ocr_journal['outcome'] == 'succeeded' and ocr_journal['exit_code'] == 0,
                'OCR is not bound to the captured client pixels')
        require(ocr_journal['stdout'] == q.record(attempt / 'ocr.json')
                and ocr_journal['stderr'] == q.record(attempt / 'ocr.stderr'), 'OCR helper streams changed')
        image = q.candidate.validate_png(attempt / 'client.png')
        require(image == row['image'] and q.record(attempt / 'ocr.json') == row['ocr'], 'UI capture/OCR changed')
        actual = ocr_value((attempt / 'ocr.json').read_bytes(), image['width'], image['height'])
        require(text_state(actual['text'], row['state'], row['view']), 'UI checkpoint lacks its observable state')
        require(set(row['log_prefixes']) == {'app.stdout', 'app.stderr'}, 'invalid log-prefix fields')
        text = []
        for name in ('app.stdout', 'app.stderr'):
            binding = row['log_prefixes'][name]
            require(q.capture.valid_record(binding) and binding['bytes'] <= MAX_LOG_BYTES, 'invalid log-prefix bounds')
            with (private / name).open('rb') as stream: data = stream.read(binding['bytes'])
            require(len(data) == binding['bytes'] and hashlib.sha256(data).hexdigest() == binding['sha256'], 'UI log-prefix changed')
            text.append(data.decode('utf-8', errors='replace'))
        require(log_counts(q.candidate.ANSI.sub('', '\n'.join(text))) == row['log_counts'], 'UI log witness changed')
    require(value['cancellation_observation'] in ('not_established', 'legacy_partial_identity_only',
            'no_commit_after_cancel_observed_preparation_phase_not_proven', 'commit_won_race_cancellation_not_established'), 'unknown cancellation evidence')
    validate_sequence(value['observations'], value['scenario'], value['cancellation_observation'], value['status'] != 'blocked')
    if value['status'] != 'blocked':
        require(len(value['observations']) == len(steps) and type(value['normal_close_exit']) is int and value['normal_close_exit'] == 0,
                'incomplete sequence or unverified normal close')
        require(value['final_streams'] is not None, 'successful UI evidence needs complete final logs')
        read_log(private)  # Errors after the last visual checkpoint still fail.
    return value


def export_observation(private, evidence, repo, expected, qualification_private, qualification_export):
    value = validate_observation(private, repo, expected, qualification_private, qualification_export)
    q.capture.no_links(evidence); require(not evidence.exists(), 'fresh bounded UI export required')
    q.capture.disjoint(private, evidence); evidence.mkdir(parents=True)
    q.write_json(evidence / 'ui-observation.json', value)
    # Legacy no-model images remain private, as required by the established
    # engineering export boundary. Only the actual Swift client may be exported.
    if value['scenario'] == 'live':
        for row in value['observations']:
            shutil.copyfile(private / ('frame-' + str(row['attempt'])) / 'client.png', evidence / (row['checkpoint'] + '.png'))
    verify_export(evidence, private, repo, expected, qualification_private, qualification_export)


def verify_export(evidence, private, repo, expected, qualification_private, qualification_export):
    value = validate_observation(private, repo, expected, qualification_private, qualification_export)
    names = {row['checkpoint'] + '.png' for row in value['observations']} if value['scenario'] == 'live' else set()
    q.capture.no_links(evidence)
    require({path.name for path in evidence.iterdir()} == {'ui-observation.json', *names}, 'unexpected UI export file')
    path = evidence / 'ui-observation.json'; q.capture.no_links(path)
    require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= 128 * 1024
            and path.read_bytes() == (json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii'), 'noncanonical UI summary')
    total = path.stat().st_size
    if value['scenario'] == 'live':
        for row in value['observations']:
            path = evidence / (row['checkpoint'] + '.png'); q.capture.no_links(path)
            require(path.is_file() and path.stat().st_nlink == 1 and q.candidate.validate_png(path) == row['image'], 'invalid Swift client export')
            total += path.stat().st_size
    require(total <= MAX_EXPORT_BYTES, 'UI export budget exceeded')
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--describe', action='store_true'); parser.add_argument('--run', action='store_true')
    parser.add_argument('--probe-desktop', action='store_true')
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--source-sha'); parser.add_argument('--qualification-private', type=Path)
    parser.add_argument('--qualification-export', type=Path); parser.add_argument('--private', type=Path)
    parser.add_argument('--evidence', type=Path); parser.add_argument('--validate-evidence', type=Path)
    parser.add_argument('--scenario', choices=('live', 'legacy'))
    parser.add_argument('--capture-helper', action='store_true'); parser.add_argument('--pid', type=int)
    parser.add_argument('--hwnd', type=int); parser.add_argument('--executable', type=Path); parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.describe:
        print(json.dumps({'identity': IDENTITY, 'status': 'prepared_unexecuted', 'live_checkpoints': LIVE_STEPS,
                          'legacy_checkpoints': LEGACY_STEPS, 'requires': ['native_Windows_Default_input_desktop',
                          'local_en_US_Windows_OCR', 'owned_foreground_window', 'functional_client_PrintWindow'],
                          'limits': LIMITS, 'appearance_accepted': False, 'release_authorized': False}, indent=2))
        return 0
    try:
        if args.probe_desktop:
            NativeWindow(os.getpid(), Path(sys.executable))
            print('Default interactive Windows desktop available; no simulator launched.'); return 0
        if args.capture_helper:
            require(not args.run and args.pid and args.hwnd and args.executable and args.output, 'invalid internal capture invocation')
            q.capture.no_links(args.output)
            window = NativeWindow(args.pid, args.executable); window.hwnd = args.hwnd; window.capture(args.output)
            return 0
        if args.validate_evidence:
            require(args.source_sha and args.private and args.qualification_private and args.qualification_export, 'UI validation inputs required')
            verify_export(args.validate_evidence, args.private, args.repo.resolve(), args.source_sha,
                          args.qualification_private, args.qualification_export)
            print('Bounded UI evidence validated; appearance/lifecycle/publication acceptance remains false.'); return 0
        require(args.run and args.source_sha and args.qualification_private and args.qualification_export
                and args.private and args.scenario, 'explicit reviewed run arguments required')
        result = run(args.repo.resolve(), args.source_sha, args.qualification_private.resolve(), args.qualification_export.resolve(),
                     args.private.absolute(), args.scenario)
        if args.evidence:
            export_observation(args.private.absolute(), args.evidence.absolute(), args.repo.resolve(), args.source_sha,
                               args.qualification_private.resolve(), args.qualification_export.resolve())
        print('Windows UI observations: ' + result['status'] + '; acceptance remains false; files remain private.')
        return 0 if result['status'] != 'blocked' else 1
    except (ValueError, OSError, KeyError, TypeError, subprocess.SubprocessError):
        print('Windows UI observation blocked; no desktop, input or capture fallback was attempted.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
