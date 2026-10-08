#!/usr/bin/env python3
"""P3: bounded structural facts from private pre-present ERROR-marked lines.

No input text is exported. Source-shaped records are observations, not proof of
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
NEAR_LINES, NEAR_BYTES = 8, 4096
# Identical marker semantics and prefix limit to the old runtime scanner. A
# marker is NOT a parsed severity. The exact fatal heading is counted separately.
MARKER = re.compile(r'\bERROR\b|\bfatal\b', re.I)
OLD_ANSI = re.compile(r'\x1b\[[0-?]*[ -/]*[@-~]')
SGR = re.compile(r'\x1b\[(?:0|1|2|3[0-7]|9[0-7])m')
# Default tracing-subscriber Full format, without dynamic spans. No optional
# target, fuzzy timestamp, arbitrary whitespace, case folding or prefixed prose.
PREFIX = re.compile(r'(?:(?:[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z) )?ERROR ([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*): (.+)')
# All-level envelopes are used ONLY for framing, never to upgrade severity.
ENVELOPE = re.compile(PREFIX.pattern.replace('ERROR ([A-Za-z_]', r'(TRACE|DEBUG| ?INFO| ?WARN|ERROR) ([A-Za-z_]'))
PANIC_LINE = re.compile(r"thread '[^'\r\n]{0,256}' panicked at [^\r\n]{1,4096}:[0-9]{1,10}:[0-9]{1,10}:")
SYSTEM_PANIC = re.compile(r'Encountered a panic (?:in system|when applying buffers for system) `[A-Za-z0-9_:<>, ()\[\]]{1,512}`!')
PANIC_NOTE = 'note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace'
LABEL = re.compile(r'\bwith\s+[\"\']|\b(?:label|name)\s*[:=]?\s*[\"\']', re.I)
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
    'malformed_encoding_or_control', 'unknown_hresult', 'unknown_d3d12_metadata', 'ambiguous_stream_framing')
require = present.require
canonical = present.canonical


def _text(raw):
    """Only CRLF/LF framing and formatter SGR are removed, never other controls."""
    if raw.endswith(b'\n'):
        raw = raw[:-1]
        if raw.endswith(b'\r'): raw = raw[:-1]
    try: text = raw.decode('utf-8', errors='strict')
    except UnicodeDecodeError: return None
    for match in SGR.finditer(text):
        if match.start() and match.end() < len(text) and re.fullmatch(r'[A-Za-z0-9_:]', text[match.start()-1]) and re.fullmatch(r'[A-Za-z0-9_:]', text[match.end()]):
            return None
    text = SGR.sub('', text)
    if any(unicodedata.category(char).startswith('C') for char in text): return None
    return text


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


def _hresult(display):
    # windows-result::Error Display is HRESULT alone, or message + " (HRESULT)".
    # A quote/bracket/newline or embedded code makes dynamic text ambiguous.
    match = re.fullmatch(r'(?:([^\[\]\"\'\r\n]+) \()?((?:0x)[0-9A-F]{8})(?(1)\))', display)
    if match is None or (match[1] and ('0x' in match[1] or '(' in match[1] or ')' in match[1])):
        return None
    return match[2] if int(match[2], 16) & 0x80000000 else None


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


def _span(line):
    return {key: line[key] for key in ('start_byte', 'byte_length', 'sha256')}


def _scan(reader):
    digest = hashlib.sha256(); first, last = [], deque(maxlen=8)
    nearby = deque(maxlen=NEAR_LINES); counts, omitted = Counter(), Counter()
    total = all_markers = post_markers = oversized = 0
    boundary = None; quarantine = False; quarantine_start = None; framing_active = False
    near_counts = Counter()
    for number, line in enumerate(present._read_lines(reader, digest)):
        total = line['start_byte'] + line['byte_length']
        oversized += int(line['oversized'])
        legacy_text = OLD_ANSI.sub('', line['raw'].decode('utf-8', errors='replace'))
        marked = bool(MARKER.search(legacy_text))
        all_markers += int(marked)
        text = None if line['oversized'] else _text(line['raw'])
        literal = line['raw']
        if literal.endswith(b'\n'): literal = literal[:-1].removesuffix(b'\r')
        is_fatal = not line['oversized'] and literal == present.FATAL_HEADER.encode('ascii')
        envelope = ENVELOPE.fullmatch(text) if text else None
        if envelope and _target(envelope[2]) != 'unrecognized': framing_active = True
        # A visible continuation can invalidate an earlier apparently complete
        # HRESULT or logger. Check the FULL stream, including after a possibly
        # forged fatal heading. Never revive tentative facts after a hazard.
        structural_postlude = bool(text is not None and (
            not text or is_fatal or PANIC_LINE.fullmatch(text) or SYSTEM_PANIC.fullmatch(text)
            or (boundary is not None and (text == 'Caused by:' or text == PANIC_NOTE
                or (text.startswith('  ') and text.strip() in present.CAUSES)))))
        hazard = line['oversized'] or text is None or LABEL.search(legacy_text) is not None
        if framing_active and not envelope and not structural_postlude: hazard = True
        if envelope and envelope[1] == 'ERROR' and _target(envelope[2]) != 'unrecognized':
            if classify(text)[1] != 'recognized': hazard = True
        if envelope and envelope[2] == 'wgpu_hal::auxil::dxgi::exception':
            # D3D12 object names need not be quoted. No prose/name trailer is
            # eligible, regardless of the outer ERROR/WARN/INFO level.
            if classify('ERROR ' + envelope[2] + ': ' + envelope[3])[1] != 'recognized': hazard = True
        if hazard:
            if not quarantine: quarantine_start = line['start_byte']
            quarantine = True
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
        elif text is None: disposition = 'malformed_encoding_or_control'
        elif LABEL.search(legacy_text): disposition = 'unsafe_label_or_path'
        elif hazard and envelope and envelope[1] == 'ERROR':
            category, disposition, facts = classify(text)
        elif hazard and envelope and envelope[2] == 'wgpu_hal::auxil::dxgi::exception':
            category, disposition, facts = _target(envelope[2]), 'unknown_d3d12_metadata', {}
        elif hazard: disposition = 'ambiguous_stream_framing'
        elif quarantine: disposition = 'ambiguous_prior_content'
        else: category, disposition, facts = classify(text)
        if disposition in ('unsafe_label_or_path', 'unsafe_token', 'unknown_hresult', 'unknown_d3d12_metadata', 'unknown_template'):
            if not quarantine: quarantine_start = line['start_byte']
            quarantine = True
        row = {**_span(line), 'target_category': category, 'disposition': disposition, 'facts': facts}
        counts[disposition] += 1; nearby.append((number, row))
        if len(first) < 8: first.append(row)
        else:
            if len(last) == 8: omitted[last[0]['disposition']] += 1
            last.append(row)
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
    result = {'schema_version': 1, 'kind': 'structured_device_error_observations',
        'record': {'sha256': digest.hexdigest(), 'bytes': total}, 'fatal_boundary': boundary,
        'scope': 'before_first_exact_fatal_literal_or_eof', 'rows': rows,
        'marker_lines_total': all_markers, 'pre_fatal_marker_lines': sum(counts.values()),
        'fatal_boundary_marker_lines': int(boundary is not None), 'post_fatal_marker_lines': post_markers,
        'disposition_counts': {key: counts[key] for key in DISPOSITIONS},
        'omitted_rows': sum(omitted.values()), 'omitted_disposition_counts': {key: omitted[key] for key in DISPOSITIONS},
        'nearby_marker_lines': sum(near_counts.values()) if boundary else None,
        'nearby_disposition_counts': {key: near_counts[key] for key in DISPOSITIONS} if boundary else None,
        'oversized_lines': oversized, 'quarantine_start_byte': quarantine_start,
        'marker_accounting': 'legacy_regex_over_bounded_physical_line_prefixes',
        'classification_complete': bool(boundary and not quarantine and not oversized and not omitted
                                        and sum(counts.values()) == counts['recognized']),
        'producer_authenticated': False, 'raw_message_text_exported': False,
        'root_cause': 'not_established', 'runtime_accepted': False, 'release_authorized': False,
        'limits': {'stream_bytes': MAX_STREAM, 'line_bytes': MAX_LINE, 'export_bytes': MAX_PUBLIC,
                   'selected_rows': MAX_ROWS, 'first_rows': 8, 'last_rows': 8, 'nearby_lines': NEAR_LINES, 'nearby_bytes': NEAR_BYTES}}
    require(all_markers == result['pre_fatal_marker_lines'] + int(boundary is not None) + post_markers, 'marker accounting mismatch')
    require(len(rows) + result['omitted_rows'] == result['pre_fatal_marker_lines'], 'row accounting mismatch')
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
