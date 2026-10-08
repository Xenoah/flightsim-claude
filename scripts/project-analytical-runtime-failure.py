#!/usr/bin/env python3
"""Bounded observations from an existing failed native scene; never a retry/pass.

Only fixed recognized message fragments are exported. Unknown error/panic lines
are represented by byte spans and hashes, never arbitrary text or source paths.
The original full streams remain private and are required for revalidation.
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import io
import math
from pathlib import Path
import re
import zlib

SPEC = importlib.util.spec_from_file_location('runtime_failure_capture', Path(__file__).with_name('capture-analytical-swift-msvc.py'))
capture = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(capture)
candidate, require = capture.check.candidate, capture.require
DETAIL_SPEC = importlib.util.spec_from_file_location('runtime_wgpu_details', Path(__file__).with_name('analytical-wgpu-error-details.py'))
wgpu_details = importlib.util.module_from_spec(DETAIL_SPEC); DETAIL_SPEC.loader.exec_module(wgpu_details)
EXPORT = 'runtime-failure.json'
MAX_PUBLIC = 65536
MAX_LINE = 65536
MAX_JOURNAL = 32768
MAX_STREAM = 64 * 1024 * 1024
MAX_PNG = 64 * 1024 * 1024
SCENES = ('default-swift', 'day-cockpit', 'low-sun', 'night-cockpit', 'fog-cockpit', 'cloud-high', 'water-high', 'tower-day', 'legacy-no-model')
COMMANDS = (*SCENES, 'absent-light-single', 'default-rejects-legacy')
# Each emitted message is a reviewed fixed fragment; no captured free text.
SIGNATURES = {
    'wgpu_validation': ('gpu_validation', 'wgpu error: Validation Error'),
    'create_render_pipeline': ('gpu_pipeline', 'Device::create_render_pipeline'),
    'create_shader_module': ('shader_processing', 'Device::create_shader_module'),
    'create_bind_group': ('gpu_binding', 'Device::create_bind_group'),
    'tonemapping_pipeline': ('tonemapping_context', 'tonemapping pipeline'),
    'shader_processing': ('shader_processing', 'failed to process shader'),
    'shader_module_failure': ('shader_processing', 'failed to create shader module'),
    'tonemapping_requirement': ('tonemapping_context', 'tonemapping requires'),
    'device_lost': ('gpu_device', 'device lost'),
    'gpu_backpressure': ('capture_backpressure', 'screenshot GPU backpressure failed'),
    'save_error': ('screenshot_save', 'Cannot save screenshot'),
    'batch_failure': ('batch_exit', 'Batch capture complete: status 1'),
    'batch_success_marker': ('batch_exit', 'Batch capture complete: status 0'),
    'cpu_ready': ('stage_marker', 'screenshot CPU ready:'),
    'capture_requested': ('stage_marker', 'capturing a screenshot to'),
    'saved_marker': ('stage_marker', 'Screenshot saved to'),
    'model_fitted': ('stage_marker', 'aircraft model fitted:'),
    'asset_load': ('asset_loading', 'Failed to load asset'),
    'render_panic': ('render_thread', 'render thread panicked'),
}
SIGNATURES.update(wgpu_details.FIXED_SIGNATURES)
ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')
PANIC = re.compile(r"\bthread\s+['\"].*?['\"]\s+panicked(?:\s+at)?\b|\bpanicked at\b", re.I)
ERROR = re.compile(r'\bERROR\b|\bfatal\b', re.I)
OUTCOMES = ('succeeded', 'failed', 'timed_out', 'launch_error', 'termination_failed')


def canonical(value):
    return (json.dumps(value, indent=2, sort_keys=True) + '\n').encode('ascii')


def scan_stream(reader):
    """Read a maximum 64 MiB stream with one 64 KiB line prefix in memory."""
    counts = {key: 0 for key in SIGNATURES}
    error_lines = panic_lines = oversized_lines = 0
    first, last, offset, total = [], [], 0, 0
    full_hash = hashlib.sha256()
    structured, structured_keys = [], set()
    structured_omitted = structured_unparsed = 0
    while True:
        chunk = reader.readline(MAX_LINE + 1)
        if not chunk: break
        prefix = chunk[:MAX_LINE]
        line_hash = hashlib.sha256(chunk); full_hash.update(chunk)
        length = len(chunk)
        oversized = length > MAX_LINE
        while not chunk.endswith(b'\n'):
            chunk = reader.readline(MAX_LINE + 1)
            if not chunk: break
            length += len(chunk); line_hash.update(chunk); full_hash.update(chunk)
            require(offset + length <= MAX_STREAM, 'unbounded runtime stream')
            oversized = True
        require(offset + length <= MAX_STREAM, 'unbounded runtime stream')
        oversized_lines += int(oversized)
        text = ANSI.sub('', prefix.decode('utf-8', errors='replace'))
        folded = text.casefold()
        found = [key for key, (_, fragment) in SIGNATURES.items() if fragment.casefold() in folded]
        for key in found: counts[key] += 1
        try:
            details = wgpu_details.details(text)
        except ValueError:
            details = []; structured_unparsed += 1
        for detail in details:
            identity = canonical(detail)
            if identity in structured_keys: continue
            if len(structured) == 16:
                structured_omitted += 1; continue
            structured_keys.add(identity)
            structured.append({'detail': detail, 'start_byte': offset, 'byte_length': length, 'sha256': line_hash.hexdigest()})
        panic, error = bool(PANIC.search(text)), bool(ERROR.search(text))
        panic_lines += int(panic); error_lines += int(error)
        if found or panic or error or oversized or details:
            item = {'start_byte': offset, 'byte_length': length, 'sha256': line_hash.hexdigest(),
                    'signatures': found, 'panic_marker': panic, 'error_marker': error, 'oversized_line': oversized,
                    'messages': [SIGNATURES[key][1] for key in found] + (['thread <redacted> panicked'] if panic else [])}
            total += 1
            if len(first) < 4: first.append(item)
            else:
                last.append(item)
                if len(last) > 4: last.pop(0)
        offset += length
    return {'record': {'sha256': full_hash.hexdigest(), 'bytes': offset},
            'signature_counts': counts, 'error_lines': error_lines, 'panic_lines': panic_lines,
            'selected_lines': first + last, 'matching_lines': total, 'omitted_matching_lines': total - len(first) - len(last),
            'structured_details': structured, 'omitted_structured_observations': structured_omitted,
            'unparsed_structured_lines': structured_unparsed,
            'oversized_lines': oversized_lines, 'maximum_scanned_line_bytes': MAX_LINE,
            'coverage': 'complete_bounded_line_scan' if oversized_lines == 0 else 'oversized_line_prefixes_only',
            'unknown_message_text_exported': False}


def stream_observations(raw):
    require(isinstance(raw, bytes) and len(raw) <= MAX_STREAM, 'unbounded runtime stream')
    return scan_stream(io.BytesIO(raw))


def png_observation(private, command_id):
    result = {'state': 'not_requested', 'record': None, 'width': None, 'height': None}
    if command_id not in SCENES: return result
    path = private / (command_id + '.png')
    capture.no_links(path)
    if not path.exists():
        result['state'] = 'absent'; return result
    require(path.is_file() and path.stat().st_nlink == 1, 'private screenshot must be an independent file')
    if path.stat().st_size > MAX_PNG:
        result['state'] = 'present_exceeds_bound'; return result
    result.update(state='present_invalid_png', record=capture.file_record(path))
    try:
        image = candidate.validate_png(path)
    except (ValueError, OSError, zlib.error):
        return result
    result.update(state='present_valid_png', width=image['width'], height=image['height'])
    return result


def project(private, expected, command_id):
    require(capture.hex_string(expected, 40) and command_id in COMMANDS, 'unsupported runtime failure identity')
    capture.no_links(private)
    base = private / 'commands' / command_id
    journal_path = base / 'journal.json'; capture.no_links(journal_path)
    require(journal_path.is_file() and journal_path.stat().st_size <= MAX_JOURNAL, 'unbounded runtime journal')
    journal = capture.read_private_json(journal_path)
    outcome, code = journal['outcome'], journal['exit_code']
    require(outcome in OUTCOMES and ((type(code) is int and -(2**32) <= code <= 2**32) or
            (code is None and outcome in ('launch_error', 'termination_failed'))), 'invalid actual failure outcome')
    require(outcome not in ('succeeded', 'failed') or (outcome == 'succeeded') == (code == 0), 'actual outcome/status mismatch')
    elapsed, timeout = journal['elapsed_seconds'], journal['timeout_seconds']
    require(type(elapsed) in (int, float) and math.isfinite(elapsed) and 0 <= elapsed <= 21600
            and type(timeout) is int and 0 < timeout <= 180, 'invalid runtime timing')
    result = {'schema_version': 1, 'kind': 'runtime_failure_observations_not_acceptance', 'source_sha': expected,
              'command_id': command_id, 'outcome': outcome, 'exit_code': code,
              'elapsed_ms': round(elapsed * 1000), 'supervisor_timeout_seconds': timeout,
              'journal': capture.file_record(journal_path), 'streams': {}, 'png': png_observation(private, command_id),
              'root_cause': 'not_established', 'runtime_accepted': False, 'release_authorized': False}
    for name in ('stdout', 'stderr'):
        path = base / name; capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and path.stat().st_size <= MAX_STREAM, 'invalid bounded runtime stream')
        with path.open('rb') as stream:
            observed = scan_stream(stream)
        require(observed['record'] == journal[name], 'runtime failure stream differs from journal')
        result['streams'][name] = observed
    classes = {SIGNATURES[key][0] for row in result['streams'].values() for key, count in row['signature_counts'].items() if count}
    if any(row['panic_lines'] for row in result['streams'].values()): classes.add('panic_marker')
    if any(row['error_lines'] for row in result['streams'].values()): classes.add('error_marker')
    result['observed_message_classes'] = sorted(classes)
    require(len(canonical(result)) <= MAX_PUBLIC, 'runtime failure projection exceeds bound')
    return result


def save(private, expected, commands):
    if not commands or commands[-1]['id'] not in COMMANDS: return None
    value = project(private, expected, commands[-1]['id'])
    path = private / EXPORT
    require(not path.exists(), 'fresh runtime failure projection required')
    path.write_bytes(canonical(value))
    return capture.file_record(path)


def verify_export(directory, private, expected, commands, binding):
    require(commands and commands[-1]['id'] in COMMANDS and capture.valid_record(binding)
            and binding['bytes'] <= MAX_PUBLIC, 'invalid failure projection binding')
    value = project(private, expected, commands[-1]['id'])
    for path in (directory / EXPORT, private / EXPORT):
        capture.no_links(path)
        require(path.is_file() and path.stat().st_nlink == 1 and capture.file_record(path) == binding
                and path.read_bytes() == canonical(value), 'runtime failure export differs from original private observations')
    return value
