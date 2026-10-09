#!/usr/bin/env python3
"""P3: bounded structural facts from private pre-present ERROR-marked lines.

No arbitrary input text is exported. Source-shaped records are observations, not proof of
producer identity, root cause, runtime acceptance, or release authorization.
P2 and all native source/command behavior remain unchanged.
"""
from __future__ import annotations

from collections import Counter, deque
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import unicodedata

SPEC = importlib.util.spec_from_file_location('device_present_boundary', Path(__file__).with_name('project-present-error-excerpt.py'))
present = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(present)
MAX_STREAM, MAX_LINE, MAX_PUBLIC = present.MAX_STREAM, present.MAX_LINE, present.MAX_PUBLIC
MAX_ROWS = 16
MAX_ACTUAL_ERRORS, MAX_ACTUAL_ERROR_BYTES = 4, 512
NEAR_LINES, NEAR_BYTES = 8, 4096
# Identical marker semantics and prefix limit to the old runtime scanner. A
# marker is NOT a parsed severity. The exact fatal heading is counted separately.
MARKER = re.compile(r'\bERROR\b|\bfatal\b', re.I)
OLD_ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')
SGR = re.compile(r'\x1b\[(?:0|1|2|3[0-7]|9[0-7])m')
SGR_RUN = re.compile(r'(?:' + SGR.pattern + r')+')
# Only these fieldless span names have reviewed literals in locked Bevy render.
# Dynamic span fields/names are not accepted as diagnostic logger envelopes.
FIELDLESS_SPANS = (
    'present_frames', 'command_buffer_generation_tasks', 'submit_graph_commands',
    'write_current_input_buffers', 'write_previous_input_buffers',
    'write_phase_instance_buffers', 'write_work_item_buffers',
    'indexed_data', 'non_indexed_data', 'indexed_cpu_metadata',
    'non_indexed_cpu_metadata', 'non_indexed_gpu_metadata', 'indexed_gpu_metadata',
    'indexed_batch_sets', 'non_indexed_batch_sets', 'collect_screenshots',
    'entity_sync', 'render thread', 'no_camera_clear_pass',
)
SPAN_NAME = '(?:' + '|'.join(re.escape(name) for name in FIELDLESS_SPANS) + ')'
SPAN_PREFIX = '(?:' + SPAN_NAME + ': )?'
# Full formatting: optional exact UTC timestamp, level, bounded reviewed
# fieldless spans, target and message. No whitespace/case folding or fuzzy time.
TIMESTAMP = r'(?:(?:[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z) )?'
LOGGER = r'([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*): (.+)'
PREFIX = re.compile(TIMESTAMP + 'ERROR ' + SPAN_PREFIX + LOGGER)
# All-level envelopes are used for framing, never to upgrade severity.
ENVELOPE = re.compile(TIMESTAMP + r'(TRACE|DEBUG| ?INFO| ?WARN|ERROR) ' + SPAN_PREFIX + LOGGER)
PANIC_LINE = re.compile(r"thread '[^'\r\n]{0,256}' panicked at [^\r\n]{1,4096}:[0-9]{1,10}:[0-9]{1,10}:")
SYSTEM_PANIC = re.compile(r'Encountered a panic (?:in system|when applying buffers for system) `[A-Za-z0-9_:<>, ()\[\]]{1,512}`!')
PANIC_NOTE = 'note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace'
# S screen_capture::finish_batch_capture prints these fixed status values. Framing
# only: this never changes diagnostic runtime/release acceptance flags.
BATCH_POSTLUDE = frozenset(('Batch capture complete: status 0', 'Batch capture complete: status 1'))
LABEL = re.compile(r'\bwith\s+[\"\']|\b(?:label|name)\s*[:=]?\s*[\"\']', re.I)
# Rust's derived Debug escapes every string; the full literal struct grammar
# prevents a quote/newline/field suffix from becoming a separate record. This
# is a framing-only exception, and none of these adapter values is exported.
RUST_DEBUG_STRING = r'"(?:[^"\\\r\n\x00-\x1f]|\\(?:["\\nrt0]|u\{[0-9a-f]{1,6}\}))*"'
ADAPTER_INFO = re.compile(
    r'AdapterInfo \{ name: ' + RUST_DEBUG_STRING +
    r', vendor: (0|[1-9][0-9]{0,9}), device: (0|[1-9][0-9]{0,9}), '
    r'device_type: (?:Other|IntegratedGpu|DiscreteGpu|VirtualGpu|Cpu), driver: ' + RUST_DEBUG_STRING +
    r', driver_info: ' + RUST_DEBUG_STRING + r', backend: (?:Noop|Vulkan|Metal|Dx12|Gl|BrowserWebGpu) \}')


TARGETS = {
    'wgpu_hal::auxil::dxgi::result': 'dxgi_result',
    'wgpu_hal::auxil::dxgi::exception': 'dxgi_exception',
    'wgpu_hal::auxil::dxgi::factory': 'dxgi_factory',
    'wgpu_hal::dx12': 'dx12',
    'wgpu_core::present': 'core_present',
}
# Descriptions are finite literals passed to HResult::into_device_result in the
# locked wgpu-hal sources. The logger is result.rs, NOT the call-site module.
OPERATIONS = {
    'Present': 'present', 'Signal fence': 'signal_fence',
    'QueryVideoMemoryInfo': 'query_video_memory_info',
    'GraphicsCommandList::close': 'graphics_command_list_close',
    'Create command list': 'create_command_list', 'SetName': 'set_name',
    'Descriptor heap creation': 'descriptor_heap_creation',
    'CPU descriptor heap creation': 'cpu_descriptor_heap_creation',
    'Idle fence creation': 'idle_fence_creation', 'Zero buffer creation': 'zero_buffer_creation',
    'Command signature creation': 'command_signature_creation', 'Signal': 'signal',
    'Set event': 'set_event', 'Map buffer': 'map_buffer', 'Map': 'map',
    'Command allocator creation': 'command_allocator_creation',
    'Root signature creation': 'root_signature_creation',
    'Query heap creation': 'query_heap_creation', 'Fence creation': 'fence_creation',
    'Queue creation': 'queue_creation', 'Device creation': 'device_creation',
    'Placed buffer creation': 'placed_buffer_creation',
    'Placed texture creation': 'placed_texture_creation',
    'Placed acceleration structure creation': 'placed_acceleration_structure_creation',
    'Committed buffer creation': 'committed_buffer_creation',
    'Committed texture creation': 'committed_texture_creation',
    'Committed acceleration structure creation': 'committed_acceleration_structure_creation',
    'Failed to create global GPU-Visible Sampler Descriptor Heap': 'sampler_descriptor_heap_creation',
    'DxcCreateInstance': 'dxc_create_instance', 'GetOutput': 'get_output', 'Compile': 'compile',
    'Root signature serialization': 'root_signature_serialization',
    'GetDebugInterface': 'get_debug_interface', 'debug_interface1': 'debug_interface1',
    'create_factory4': 'create_factory4', 'create_factory_media': 'create_factory_media',
    'CreateEventA': 'create_event', 'MakeWindowAssociation': 'make_window_association',
    'SetMaximumFrameLatency': 'set_maximum_frame_latency',
    'Failed to get swapchain buffer': 'get_swapchain_buffer',
}
STATIC = {
    ('wgpu_hal::dx12::device', 'Wait failed!'): 'device_wait_failed',
    ('wgpu_hal::dx12::descriptor', 'Failed to allocate a handle form a fixed size heap'): 'fixed_descriptor_heap_exhausted',
    ('wgpu_hal::dx12::suballocation', 'DX12 gpu-allocator: No Compatible Memory Type Found'): 'no_compatible_memory_type',
    ('wgpu_hal::dx12::suballocation', 'DX12 gpu-allocator: Invalid Allocation Creation Description'): 'invalid_allocation_description',
    ('wgpu_core::device::resource', "wgpu-core feature 'trace' is not enabled"): 'core_trace_feature_disabled',
}
# ID/name pairs from locked windows 0.58.0 Direct3D12 constants. Categories are
# reported as observed, never inferred from the ID or treated as a removal cause.
MESSAGE_IDS = {
    'DEVICE_REMOVAL_PROCESS_AT_FAULT': 232,
    'DEVICE_REMOVAL_PROCESS_POSSIBLY_AT_FAULT': 233,
    'DEVICE_REMOVAL_PROCESS_NOT_AT_FAULT': 234,
    'COMMAND_ALLOCATOR_CONTENTION': 540, 'COMMAND_ALLOCATOR_RESET': 541,
    'COMMAND_ALLOCATOR_RESET_BUNDLE': 542, 'COMMAND_ALLOCATOR_CANNOT_RESET': 543,
    'COMMAND_LIST_OPEN': 544, 'COMMAND_LIST_CLOSED': 547,
    'COMMAND_ALLOCATOR_SYNC': 552, 'COMMAND_LIST_SYNC': 553,
    'COMMAND_LIST_OUTOFMEMORY': 903,
    'COMMAND_LIST_MULTIPLE_SWAPCHAIN_BUFFER_REFERENCES': 904,
    'COMMAND_LIST_TOO_MANY_SWAPCHAIN_REFERENCES': 905,
}
CATEGORIES = frozenset(('APPLICATION_DEFINED', 'MISCELLANEOUS', 'INITIALIZATION', 'CLEANUP',
    'COMPILATION', 'STATE_CREATION', 'STATE_SETTING', 'STATE_GETTING', 'RESOURCE_MANIPULATION', 'EXECUTION', 'SHADER'))
DISPOSITIONS = ('recognized', 'unknown_target', 'unknown_template', 'malformed_prefix',
    'unsafe_label_or_path', 'unsafe_token', 'oversized', 'ambiguous_prior_content',
    'invalid_utf8', 'unsupported_control', 'formatter_boundary_rejection',
    'unknown_hresult', 'unknown_d3d12_metadata', 'ambiguous_stream_framing', 'shader_source_content')
TEXT_REJECTIONS = ('invalid_utf8', 'unsupported_control', 'formatter_boundary_rejection')
FRAMING_REJECTIONS = TEXT_REJECTIONS + ('oversized_line', 'unsafe_label_syntax',
    'unframed_continuation', 'unknown_device_error_template', 'unknown_exception_metadata',
    'shader_body_control', 'unsupported_shader_framing', 'unterminated_shader_block')
require = present.require
canonical = present.canonical


def _decode_line(raw):
    """Remove reviewed SGR without joining identifier pieces across formatting.

    Full formats the target and its delimiter as separate dimmed values, so a
    reset/start run occurs exactly at the target end. Check visible boundaries,
    not the literal `m` inside a preceding escape. The colon exception applies
    only to the complete logger target's parsed delimiter, never internal `::`.
    """
    if raw.endswith(b'\n'):
        raw = raw[:-1]
        if raw.endswith(b'\r'): raw = raw[:-1]
    try: original = raw.decode('utf-8', errors='strict')
    except UnicodeDecodeError: return None, 'invalid_utf8'
    text = SGR.sub('', original)
    if any(unicodedata.category(char).startswith('C') for char in text):
        return None, 'unsupported_control'
    envelope = ENVELOPE.fullmatch(text)
    delimiters = set()
    if envelope:
        delimiters.add(envelope.end(2))
        span_start = envelope.end(1) + 1
        span_region = text[span_start:envelope.start(2)]
        for span in re.finditer(SPAN_NAME + ':', span_region):
            delimiters.update((span_start + span.end() - 1, span_start + span.end()))
    removed = 0
    for run in SGR_RUN.finditer(original):
        visible_offset = run.start() - removed
        removed += run.end() - run.start()
        if not 0 < visible_offset < len(text): continue
        before, after = text[visible_offset-1], text[visible_offset]
        if re.fullmatch(r'[A-Za-z0-9_:]', before) and re.fullmatch(r'[A-Za-z0-9_:]', after):
            if visible_offset not in delimiters:
                return None, 'formatter_boundary_rejection'
    return text, None


def _text(raw):
    """Compatibility helper; the scan also accounts for the fixed rejection reason."""
    return _decode_line(raw)[0]


def _debug_strings_valid(message):
    for string in re.finditer(RUST_DEBUG_STRING, message):
        for escape in re.finditer(r'\\(?:["\\nrt0]|u\{[0-9a-f]{1,6}\})', string[0]):
            if not escape[0].startswith('\\u{'): continue
            digits = escape[0][3:-1]; code = int(digits, 16)
            if (digits != format(code, 'x') or code > 0x10FFFF or 0xD800 <= code <= 0xDFFF
                    or code in (0, 9, 10, 13) or chr(code).isprintable()): return False
    return True


def _adapter_info_frame(envelope):
    if not envelope or envelope[1].strip() != 'INFO' or envelope[2] != 'bevy_render::renderer': return False
    match = ADAPTER_INFO.fullmatch(envelope[3])
    return bool(match and all(int(number) <= 2**32 - 1 for number in match.groups())
                and _debug_strings_valid(envelope[3]))


def _canonical_ansi_envelope(raw, text, envelope):
    """Recognize default Full prefix bytes, not arbitrary-stream authorship.

    DefaultVisitor's message EscapeGuard escapes payload ESC. This exact prefix
    therefore separates messages only for that locked path; it is not an
    authentication mechanism for other writers or unsanitized Debug fields.
    """
    if envelope is None: return False
    timestamp = text[:envelope.start(1)]
    if not re.fullmatch(r'[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z ', timestamp): return False
    level = envelope[1]
    colors = {'TRACE': '35', 'DEBUG': '34', ' INFO': '32', ' WARN': '33', 'ERROR': '31'}
    if level not in colors: return False
    prefix = '\x1b[2m' + timestamp[:-1] + '\x1b[0m \x1b[' + colors[level] + 'm' + level + '\x1b[0m '
    span_region = text[envelope.end(1)+1:envelope.start(2)]
    if span_region:
        name = span_region[:-2]
        if name not in FIELDLESS_SPANS: return False
        prefix += '\x1b[1m' + name + '\x1b[0m\x1b[2m:\x1b[0m '
    prefix += '\x1b[2m' + envelope[2] + '\x1b[0m\x1b[2m:\x1b[0m '
    encoded = prefix.encode('ascii')
    return raw.startswith(encoded) and b'\x1b' not in raw[len(encoded):]


NAGA_HEADER = re.compile(r'Naga generated shader for ' + RUST_DEBUG_STRING + r' at (?:Vertex|Fragment|Compute|Task|Mesh):')


def _naga_header(envelope):
    return bool(envelope and envelope[1] == ' INFO' and envelope[2] == 'wgpu_hal::dx12::device'
                and NAGA_HEADER.fullmatch(envelope[3]) and _debug_strings_valid(envelope[3]))


def _target(target):
    if target in TARGETS: return TARGETS[target]
    for prefix, category in (('wgpu_hal::dx12::', 'dx12_descendant'), ('wgpu_core::device::', 'core_device_descendant')):
        if target.startswith(prefix): return category
    return 'unrecognized'


def _unsafe(message):
    if LABEL.search(message) or present.PATH.search(message) or present.URL.search(message):
        return 'unsafe_label_or_path'
    if any(pattern.search(message) for pattern in (present.CREDENTIAL, present.BEARER, present.JWT, present.API_KEY)):
        return 'unsafe_token'
    return None


def _hresult_match(display):
    # windows-result::Error Display is HRESULT alone, or message + " (HRESULT)".
    # A quote/bracket/newline or embedded code makes dynamic text ambiguous.
    match = re.fullmatch(r'(?:([^\[\]\"\'\r\n]+) \()?((?:0x)[0-9A-F]{8})(?(1)\))', display)
    if match is None or (match[1] and ('0x' in match[1] or '(' in match[1] or ')' in match[1])):
        return None
    return match if int(match[2], 16) & 0x80000000 else None


def _hresult(display):
    match = _hresult_match(display)
    return match[2] if match else None


def classify(text):
    match = PREFIX.fullmatch(text)
    if match is None: return 'unrecognized', 'malformed_prefix', {}
    target, message = match.groups(); category = _target(target)
    if category == 'unrecognized': return category, 'unknown_target', {}
    static = STATIC.get((target, message))
    if static: return category, 'recognized', {'operation': static}
    unsafe = _unsafe(message)
    if unsafe: return category, unsafe, {}
    if target == 'wgpu_hal::auxil::dxgi::result':
        for description, operation in OPERATIONS.items():
            prefix = description + ' failed: '
            if message.startswith(prefix):
                code = _hresult(message[len(prefix):])
                return category, 'recognized' if code else 'unknown_hresult', ({'operation': operation, 'hresult': code} if code else {})
    if target == 'wgpu_hal::dx12::command' and message.startswith('ID3D12CommandAllocator::Reset() failed with '):
        code = _hresult(message[len('ID3D12CommandAllocator::Reset() failed with '):])
        return category, 'recognized' if code else 'unknown_hresult', ({'operation': 'command_allocator_reset', 'hresult': code} if code else {})
    # WAIT_EVENT Debug includes its tuple name. Do not mistake its decimal value
    # or alternate lower-hex Debug form for an HRESULT; export no wait number.
    if target == 'wgpu_hal::dx12::device' and re.fullmatch(r'Unexpected wait status: 0xWAIT_EVENT\([0-9]{1,10}\)', message):
        number = int(message.removeprefix('Unexpected wait status: 0xWAIT_EVENT(')[:-1])
        if number <= 2**32 - 1: return category, 'recognized', {'operation': 'unexpected_device_wait_status'}
    if target == 'wgpu_hal::dx12' and re.fullmatch(r'Unexpected wait status: 0xWAIT_EVENT\([0-9a-f]{1,8}\)', message):
        return category, 'recognized', {'operation': 'unexpected_surface_wait_status'}
    if target == 'wgpu_hal::auxil::dxgi::exception':
        # The relay has no trusted delimiter around driver prose/object names.
        # Only a whole metadata-only message is eligible. Prose plus a trailer
        # stays unknown even when the apparent ID/name pair is recognized.
        match = re.fullmatch(r'\[ ([A-Z_]+) (ERROR|CORRUPTION) #([0-9]{1,4}): ([A-Z_]+)\]', message)
        if match and match[1] in CATEGORIES and MESSAGE_IDS.get(match[4]) == int(match[3]) and str(int(match[3])) == match[3]:
            return category, 'recognized', {'operation': 'd3d12_validation_message', 'message_category': match[1],
                'message_severity': match[2], 'message_id': int(match[3]), 'message_name': match[4]}
        return category, 'unknown_d3d12_metadata', {}
    return category, 'unknown_template', {}


def _actual_message(text, facts):
    """Retain input slices only; redact the ENTIRE variable driver-message field.

    A hash/span identifies the original physical record. This is not a rebuilt
    explanation or a guessed Windows message, and never exposes arbitrary prose.
    """
    match = PREFIX.fullmatch(text)
    if match is None: return None
    target, message = match.groups()
    redacted = False
    if ((target, message) in STATIC or
            facts.get('operation') == 'd3d12_validation_message'):
        selected = message
    elif 'hresult' in facts:
        prefixes = tuple(description + ' failed: ' for description in OPERATIONS)
        prefixes += ('ID3D12CommandAllocator::Reset() failed with ',)
        prefix = next((value for value in prefixes if message.startswith(value)), None)
        if prefix is None: return None
        display = message[len(prefix):]; parts = _hresult_match(display)
        if parts is None: return None
        if parts[1] is None:
            selected = message
        else:
            # Keep the exact input prefix and suffix, replacing all message text.
            selected = message[:len(prefix)] + '[redacted-driver-prose]' + display[parts.end(1):]
            redacted = True
    else:
        return None
    selected, truncated = present._shorten(selected, MAX_ACTUAL_ERROR_BYTES)
    return {'text': selected, 'redacted': redacted, 'truncated': truncated}


def _span(line):
    return {key: line[key] for key in ('start_byte', 'byte_length', 'sha256')}


def _scan(reader):
    digest = hashlib.sha256(); first, last = [], deque(maxlen=8)
    nearby = deque(maxlen=NEAR_LINES); counts, omitted = Counter(), Counter()
    total = all_markers = post_markers = oversized = 0
    boundary = None; quarantine = False; quarantine_start = None; framing_active = False
    near_counts = Counter(); text_rejections = Counter(); adapter_frames = 0
    framing_rejections = Counter(); first_rejection = None
    shader_open = False; shader_start = None; shader_blocks = shader_lines = shader_markers = 0
    outside_marker_seen = False
    actual = deque(maxlen=MAX_ACTUAL_ERRORS); actual_eligible = 0; actual_no_template = 0

    def reject(reason, start):
        nonlocal quarantine, quarantine_start, first_rejection
        framing_rejections[reason] += 1
        if not quarantine: quarantine_start, first_rejection = start, reason
        quarantine = True

    def retain(number, row):
        counts[row['disposition']] += 1; nearby.append((number, row))
        if len(first) < 8: first.append(row)
        else:
            if len(last) == 8: omitted[last[0]['disposition']] += 1
            last.append(row)
    for number, line in enumerate(present._read_lines(reader, digest)):
        total = line['start_byte'] + line['byte_length']
        oversized += int(line['oversized'])
        legacy_text = OLD_ANSI.sub('', line['raw'].decode('utf-8', errors='replace'))
        marked = bool(MARKER.search(legacy_text))
        all_markers += int(marked)
        text, text_rejection = (None, None) if line['oversized'] else _decode_line(line['raw'])
        if text_rejection: text_rejections[text_rejection] += 1
        literal = line['raw']
        if literal.endswith(b'\n'): literal = literal[:-1].removesuffix(b'\r')
        is_fatal = not line['oversized'] and literal == present.FATAL_HEADER.encode('ascii')
        envelope = ENVELOPE.fullmatch(text) if text else None
        canonical_envelope = bool(text is not None and _canonical_ansi_envelope(line['raw'], text, envelope))
        if shader_open and not canonical_envelope:
            # Naga's raw multiline source is private message content. Its plain
            # ERROR/header-shaped lines cannot become a logger or fatal fact.
            shader_lines += 1; shader_markers += int(marked)
            if line['oversized']: reject('oversized_line', line['start_byte'])
            elif text_rejection: reject(text_rejection, line['start_byte'])
            elif b'\x1b' in line['raw']: reject('shader_body_control', line['start_byte'])
            if marked:
                retain(number, {**_span(line), 'target_category': 'shader_source',
                                'disposition': 'shader_source_content', 'facts': {}})
            continue
        if shader_open: shader_open = False
        shader_header = bool(boundary is None and not outside_marker_seen and not quarantine
                             and canonical_envelope and _naga_header(envelope))
        if shader_header:
            shader_open = True; shader_start = line['start_byte']; shader_blocks += 1
        if boundary is None and marked: outside_marker_seen = True
        if envelope and _target(envelope[2]) != 'unrecognized': framing_active = True
        # A visible continuation can invalidate an earlier apparently complete
        # HRESULT or logger. Check the FULL stream, including after a possibly
        # forged fatal heading. Never revive tentative facts after a hazard.
        structural_postlude = bool(text is not None and (
            not text or is_fatal or text in BATCH_POSTLUDE or PANIC_LINE.fullmatch(text) or SYSTEM_PANIC.fullmatch(text)
            or (boundary is not None and (text == 'Caused by:' or text == PANIC_NOTE
                or (text.startswith('  ') and text.strip() in present.CAUSES)))))
        adapter_frame = _adapter_info_frame(envelope)
        adapter_frames += int(adapter_frame)
        unsafe_label = LABEL.search(legacy_text) is not None and not adapter_frame and not shader_header
        reasons = []
        if line['oversized']: reasons.append('oversized_line')
        elif text_rejection: reasons.append(text_rejection)
        if unsafe_label: reasons.append('unsafe_label_syntax')
        if (envelope and envelope[2] == 'wgpu_hal::dx12::device'
                and envelope[3].startswith('Naga generated shader') and not shader_header):
            reasons.append('unsupported_shader_framing')
        if framing_active and text is not None and not envelope and not structural_postlude:
            reasons.append('unframed_continuation')
        if envelope and envelope[1] == 'ERROR' and _target(envelope[2]) != 'unrecognized':
            if classify(text)[1] != 'recognized': reasons.append('unknown_device_error_template')
        if envelope and envelope[2] == 'wgpu_hal::auxil::dxgi::exception':
            if classify('ERROR ' + envelope[2] + ': ' + envelope[3])[1] != 'recognized':
                reasons.append('unknown_exception_metadata')
        hazard = bool(reasons)
        for reason in reasons: reject(reason, line['start_byte'])
        if is_fatal and boundary is None:
            boundary = {**_span(line), 'state': 'ambiguous_prior_content' if quarantine else 'exact_literal_observed'}
            near_counts = Counter(row['disposition'] for n, row in nearby if number - n <= NEAR_LINES
                                  and line['start_byte'] - row['start_byte'] <= NEAR_BYTES)
            continue
        if boundary is not None:
            post_markers += int(marked); continue
        if not marked: continue
        category, facts = 'unrecognized', {}
        if line['oversized']: disposition = 'oversized'
        elif text_rejection: disposition = text_rejection
        elif unsafe_label: disposition = 'unsafe_label_or_path'
        elif hazard and envelope and envelope[1] == 'ERROR':
            category, disposition, facts = classify(text)
        elif hazard and envelope and envelope[2] == 'wgpu_hal::auxil::dxgi::exception':
            category, disposition, facts = _target(envelope[2]), 'unknown_d3d12_metadata', {}
        elif hazard: disposition = 'ambiguous_stream_framing'
        elif quarantine: disposition = 'ambiguous_prior_content'
        else: category, disposition, facts = classify(text)
        if disposition in ('unsafe_label_or_path', 'unsafe_token', 'unknown_hresult', 'unknown_d3d12_metadata', 'unknown_template'):
            if not quarantine: reject('unknown_device_error_template', line['start_byte'])
        row = {**_span(line), 'target_category': category, 'disposition': disposition, 'facts': facts}
        if disposition == 'recognized':
            actual_message = _actual_message(text, facts)
            if actual_message is None: actual_no_template += 1
            else:
                actual_eligible += 1
                actual.append({**_span(line), **actual_message})
        retain(number, row)
    if shader_open: reject('unterminated_shader_block', shader_start)
    rows = first + list(last)
    if quarantine:
        # Revoke previously tentative facts, including rows already evicted by
        # the cap and counts captured before a forged fatal. No raw text needed.
        for counter in (counts, omitted, near_counts):
            counter['ambiguous_stream_framing'] += counter['recognized']
            counter['recognized'] = 0
        for row in rows:
            if row['disposition'] == 'recognized':
                row['disposition'] = 'ambiguous_stream_framing'; row['facts'] = {}
        if boundary: boundary['state'] = 'ambiguous_stream_framing'
    pre_total = sum(counts.values())
    actual_counts = {'selected': len(actual), 'omitted_by_cap': actual_eligible - len(actual),
                     'no_reviewed_text_template': actual_no_template,
                     'rejected_record': pre_total - actual_eligible - actual_no_template,
                     'revoked_by_framing': 0}
    if quarantine:
        actual.clear()
        actual_counts = {key: 0 for key in actual_counts}
        actual_counts['revoked_by_framing'] = pre_total
    result = {'schema_version': 2, 'kind': 'structured_device_error_observations',
        'record': {'sha256': digest.hexdigest(), 'bytes': total}, 'fatal_boundary': boundary,
        'scope': 'before_first_exact_fatal_literal_or_eof', 'rows': rows,
        'actual_error_excerpts': list(actual), 'actual_text_counts': actual_counts,
        'actual_text_policy': 'original_message_slices_with_complete_driver_prose_redaction',
        'marker_lines_total': all_markers, 'pre_fatal_marker_lines': sum(counts.values()),
        'fatal_boundary_marker_lines': int(boundary is not None), 'post_fatal_marker_lines': post_markers,
        'disposition_counts': {key: counts[key] for key in DISPOSITIONS},
        'omitted_rows': sum(omitted.values()), 'omitted_disposition_counts': {key: omitted[key] for key in DISPOSITIONS},
        'nearby_marker_lines': sum(near_counts.values()) if boundary else None,
        'nearby_disposition_counts': {key: near_counts[key] for key in DISPOSITIONS} if boundary else None,
        'oversized_lines': oversized, 'quarantine_start_byte': quarantine_start,
        'text_rejection_counts': {key: text_rejections[key] for key in TEXT_REJECTIONS},
        'framing_only_adapter_info_records': adapter_frames,
        'framing_rejection_counts': {key: framing_rejections[key] for key in FRAMING_REJECTIONS},
        'quarantine_first_reason': first_rejection,
        'shader_blocks': {'opened': shader_blocks, 'body_lines': shader_lines,
                          'body_marker_lines': shader_markers, 'unterminated': shader_open},
        'marker_accounting': 'legacy_regex_over_bounded_physical_line_prefixes',
        'classification_complete': bool(boundary and not quarantine and not oversized and not omitted
                                        and sum(counts.values()) == counts['recognized']),
        'producer_authenticated': False, 'raw_message_text_exported': False,
        'sanitized_actual_message_text_exported': bool(actual), 'arbitrary_message_text_exported': False,
        'root_cause': 'not_established', 'runtime_accepted': False, 'release_authorized': False,
        'limits': {'stream_bytes': MAX_STREAM, 'line_bytes': MAX_LINE, 'export_bytes': MAX_PUBLIC,
                   'actual_error_excerpts': MAX_ACTUAL_ERRORS, 'actual_error_bytes': MAX_ACTUAL_ERROR_BYTES,
                   'selected_rows': MAX_ROWS, 'first_rows': 8, 'last_rows': 8, 'nearby_lines': NEAR_LINES, 'nearby_bytes': NEAR_BYTES}}
    require(all_markers == result['pre_fatal_marker_lines'] + int(boundary is not None) + post_markers, 'marker accounting mismatch')
    require(len(rows) + result['omitted_rows'] == result['pre_fatal_marker_lines'], 'row accounting mismatch')
    require(sum(actual_counts.values()) == pre_total, 'actual-text accounting mismatch')
    require(len(canonical(result)) <= MAX_PUBLIC, 'device facts exceed export byte limit')
    return result


def project_stream(path, *, expected_record):
    """Same bounded independent-file and complete-byte binding boundary as P2."""
    require(isinstance(expected_record, dict) and set(expected_record) == {'sha256', 'bytes'}
            and isinstance(expected_record['sha256'], str) and re.fullmatch(r'[0-9a-f]{64}', expected_record['sha256']) is not None
            and type(expected_record['bytes']) is int and 0 <= expected_record['bytes'] <= MAX_STREAM, 'invalid expected stream record')
    path = Path(path).absolute()
    for part in (*reversed(path.parents), path):
        details = part.lstat()
        require(not stat.S_ISLNK(details.st_mode)
                and not getattr(details, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT,
                'linked runtime stream or reparse point rejected')
    before = details
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and before.st_size == expected_record['bytes'], 'invalid bounded runtime stream')
    with path.open('rb') as reader:
        opened = os.fstat(reader.fileno())
        require((opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino), 'runtime stream changed while opening')
        value = _scan(reader); after = os.fstat(reader.fileno())
    require((opened.st_size, opened.st_mtime_ns, opened.st_ctime_ns) == (after.st_size, after.st_mtime_ns, after.st_ctime_ns), 'runtime stream changed while reading')
    require(value['record'] == expected_record, 'runtime stream differs from expected record')
    return value
