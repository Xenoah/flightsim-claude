#!/usr/bin/env python3
"""A bounded, sanitized excerpt of the first exact Surface::present fatal block.

This is selected error text, not a root-cause determination or acceptance. It
does not export general log/environment lines, panic locations, or backtraces.
The complete input is hashed, including omitted lines and oversized line tails.

Source references (Cargo.lock versions, inspected from the registry archives):
* wgpu 27.0.1, backend/wgpu_core.rs:348,351-378,3858: fatal header/error tree.
* wgpu-core 27.0.3, present.rs:43-54, device/mod.rs:296-335: displayed causes.
* wgpu-core 27.0.3, resource.rs:83-92: user-controlled resource labels.
* wgpu-hal 27.0.4, dx12/mod.rs:1589 and auxil/dxgi/result.rs:11-25:
  preceding ERROR "Present failed: {err}" and HRESULT-to-device conversion.
* windows-result 0.2.0, error.rs:221-230, hresult.rs:144-147: Display text/code.

Unknown indented causes retain sanitized text and are explicitly unrecognized.
Recognizing text does not authenticate its producer. Conservative redaction can
remove useful detail; the unchanged private stream remains the source of truth.
"""
from __future__ import annotations

from collections import deque
import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import unicodedata

MAX_STREAM = 64 * 1024 * 1024
MAX_LINE = 64 * 1024
MAX_PUBLIC = 16 * 1024
MAX_ERRORS = 4
MAX_ERROR_BYTES = 512
MAX_CAUSE_LINES = 8
MAX_CAUSE_BYTES = 2048
MAX_NEAR_LINES = 8
MAX_NEAR_BYTES = 4096
MAX_HEADER_BLANKS = 2
FATAL_HEADER = 'Error in Surface::present: Validation Error'
FATAL_LINE_SHA256 = '4cbeb207d73b1fb2f2a49d74979523ee282e8a364e8e2c99bd388fd6a3a8e18a'

CAUSES = {
    'Surface is invalid': 'SurfaceError::Invalid',
    'Surface is not configured for presentation': 'SurfaceError::NotConfigured',
    'Surface image is already acquired': 'SurfaceError::AlreadyAcquired',
    'Texture has been destroyed': 'SurfaceError::TextureDestroyed',
    'Parent device is lost': 'DeviceError::Lost',
    'Not enough memory left.': 'DeviceError::OutOfMemory',
    'Out of memory': 'hal::DeviceError::OutOfMemory',
    'Device is lost': 'hal::DeviceError::Lost',
    'Unexpected error variant (driver implementation is at fault)': 'hal::DeviceError::Unexpected',
    'Surface is lost': 'hal::SurfaceError::Lost',
    'Surface is outdated, needs to be re-created': 'hal::SurfaceError::Outdated',
}
HRESULT_ENUMS = frozenset((
    'E_OUTOFMEMORY', 'E_INVALIDARG', 'E_FAIL', 'E_ACCESSDENIED', 'E_UNEXPECTED',
    'DXGI_ERROR_DEVICE_RESET', 'DXGI_ERROR_DEVICE_REMOVED', 'DXGI_ERROR_DEVICE_HUNG',
    'DXGI_ERROR_DRIVER_INTERNAL_ERROR', 'DXGI_ERROR_INVALID_CALL',
    'DXGI_ERROR_WAS_STILL_DRAWING', 'DXGI_ERROR_NOT_CURRENTLY_AVAILABLE',
    'DXGI_ERROR_UNSUPPORTED', 'DXGI_ERROR_ACCESS_LOST', 'DXGI_ERROR_SESSION_DISCONNECTED',
))
ANSI = re.compile(r'\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\))')
LOG_START = re.compile(r'^(?:\[?\d{4}-\d\d-\d\d[T ][0-9:.+Z-]+\]?\s+)?\[?(?:TRACE|DEBUG|INFO|WARN|WARNING|ERROR|FATAL)\b', re.I)
ERROR_LOG = re.compile(
    r'^(?:\d{4}-\d\d-\d\dT[0-9:.+Z-]+\s+)?ERROR\s+'
    r'(?:(wgpu_hal::auxil::dxgi::result|wgpu_hal::dx12|wgpu_core::present|wgpu_core::device):\s+)?(.+)$')
PRIVATE_LOCATION = re.compile(r"\bpanicked\b|^stack backtrace:|^\d+:\s|^at\s|^note:\s", re.I)
LOCATION_ONLY = re.compile(r'^(?:[A-Za-z]:[\\/]|\\\\|/|(?:[\w.-]+[\\/])*[\w.-]+\.(?:rs|py|cpp|cc|c|h|js|ts):[0-9])')
ENVIRONMENT = re.compile(r'^(?:(?:export\s+|set\s+)?[A-Za-z_][A-Za-z0-9_.-]{0,63}\s*=|(?:environment(?: variables)?|env)\s*[:{]|[\[{])', re.I)
# Resource labels are unconstrained user strings, not technical evidence.
QUOTED = re.compile(r'"(?:\\.|[^"\\])*"|(?<![A-Za-z])\'(?:\\.|[^\'\\])*\'')
RESOURCE_LABEL = re.compile(r"\bwith\s+[\"']", re.I)
URL = re.compile(r'(?i)\b(?:[a-z][a-z0-9+.-]{1,20}://|www\.)[^\s<>"\']+')
# Unquoted paths can contain spaces, commas, parentheses and semicolons. Their
# beginning and end can both be ambiguous: suppress the entire selected line.
PATH = re.compile(r"(?<![A-Za-z0-9_:])(?:[A-Za-z]:(?!:)|[\\/]|\.\.?[\\/]|~[\\/])|[^\s\"']+[\\/]")
CREDENTIAL = re.compile(
    r'(?i)\b(?:password|passwd|passphrase|pwd|secret|(?:access|refresh|session)[ _-]?token|'
    r'token|api[ _-]?key|client[ _-]?secret|authorization|cookie|credentials?|'
    r'connection[ _-]?string|private[ _-]?key|session[ _-]?(?:id|key)|'
    r'[A-Za-z0-9_-]*(?:token|secret|password|passwd|api[_-]?key|access[_-]?key|credential)[A-Za-z0-9_-]*)\s*[=:]\s*[^\r\n,;]*')
BEARER = re.compile(r'(?i)\b(?:Bearer|Basic)\s+[A-Za-z0-9._~+/=-]+')
JWT = re.compile(r'(?<![A-Za-z0-9_-])[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(?![A-Za-z0-9_-])')
API_KEY = re.compile(r'(?i)\b(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{8,}|(?:gh[pousr]_|github_pat_)[A-Za-z0-9_]{8,}|AKIA[A-Z0-9]{16}|xox[baprs]-[A-Za-z0-9-]{8,})\b')
TOKEN = re.compile(r'(?<![A-Za-z0-9_])[A-Za-z0-9_!@#$%^&*+=./~?`|-]{16,}(?![A-Za-z0-9_])')
NUMBER = re.compile(r'(?<![A-Za-z0-9_])(?:0[xX][0-9A-Fa-f]+|-?[0-9]+)(?![A-Za-z0-9_])')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True) + '\n').encode('ascii')


def _clean(text):
    text = ANSI.sub('', text)
    # Join around formatting controls before matching paths and credentials.
    return ''.join(char for char in text if not unicodedata.category(char).startswith('C'))


def _token(match):
    value = match.group()
    if value.rstrip('.') in HRESULT_ENUMS:
        return value
    counts = {char: value.count(char) for char in set(value)}
    entropy = -sum((count / len(value)) * math.log2(count / len(value)) for count in counts.values())
    if len(value) >= 24 or entropy >= 3.4:
        return '[redacted-token]'
    return value


def _number(match):
    value = match.group()
    if len(value) > 11:
        return '[redacted-number]'
    number = int(value, 16 if value.lower().startswith('0x') else 10)
    return value if -(2**31) <= number <= 2**32 - 1 else '[redacted-number]'


def sanitize(text):
    """Redact before shortening, so a cap cannot expose a credential prefix."""
    text = _clean(text)
    label = RESOURCE_LABEL.search(text)
    if label:
        # ResourceErrorIdent inserts label contents verbatim. Delimiter/newline
        # injection prevents reliable parsing; never export its uncertain tail.
        text = text[:label.start()] + '[redacted-label-content]'
    text = QUOTED.sub(lambda match: "'[redacted-string]'", text)
    for pattern, replacement in (
        (URL, '[redacted-url]'), (CREDENTIAL, '[redacted-credential]'),
        (BEARER, '[redacted-credential]'), (JWT, '[redacted-token]'),
        (API_KEY, '[redacted-token]'),
    ):
        text = pattern.sub(replacement, text)
    path = PATH.search(text)
    if path:
        return '[redacted-path-bearing-line]'
    text = TOKEN.sub(_token, text)
    return NUMBER.sub(_number, text)


def _shorten(text, limit):
    # Bound escaped JSON bytes as well as UTF-8 bytes, including truncation text.
    size = lambda value: len(json.dumps(value, ensure_ascii=True).encode('ascii'))
    if size(text) <= limit:
        return text, False
    marker = ' [truncated]'
    low, high = 0, len(text)
    while low < high:
        middle = (low + high + 1) // 2
        if size(text[:middle] + marker) <= limit:
            low = middle
        else:
            high = middle - 1
    return text[:low] + marker, True


def _row(line, text, *, limit, category=None):
    clean = sanitize(text)
    selected, truncated = _shorten(clean, limit)
    result = {'start_byte': line['start_byte'], 'byte_length': line['byte_length'],
              'sha256': line['sha256'], 'text': selected,
              'redacted': clean != text, 'truncated': truncated}
    if category is not None:
        result['source_variant'] = category
    return result


def _read_lines(reader, digest):
    """Never request more than 64 KiB, never retain an oversized-line tail."""
    offset = 0
    while True:
        chunk = reader.readline(MAX_LINE)
        if not chunk:
            return
        prefix = chunk
        length = len(chunk)
        line_hash = hashlib.sha256(chunk)
        digest.update(chunk)
        require(offset + length <= MAX_STREAM, 'runtime stream exceeds byte limit')
        while not chunk.endswith(b'\n'):
            chunk = reader.readline(MAX_LINE)
            if not chunk:
                break
            length += len(chunk)
            require(offset + length <= MAX_STREAM, 'runtime stream exceeds byte limit')
            digest.update(chunk)
            line_hash.update(chunk)
        yield {'start_byte': offset, 'byte_length': length, 'sha256': line_hash.hexdigest(),
               'oversized': length > MAX_LINE, 'raw': prefix}
        offset += length


def _relevant_error(text):
    match = ERROR_LOG.fullmatch(text)
    if match is None:
        return None
    target, message = match.groups()
    relevant = (message.startswith('Present failed: ') or message in CAUSES
                or message.startswith('Surface::present: '))
    if not relevant:
        return None
    return 'ERROR ' + (target + ': ' if target else '') + message


def _cause_variant(text):
    if text in CAUSES:
        return CAUSES[text]
    if text.startswith('Present failed: '):
        return 'dx12_present_error_text'
    return 'unrecognized'


def _scan(reader):
    digest = hashlib.sha256()
    pending = deque(maxlen=MAX_NEAR_LINES)
    result = {
        'schema_version': 1, 'kind': 'sanitized_surface_present_error_excerpt',
        'record': None, 'fatal_header': None, 'error_lines': [],
        'cause_block': {'state': 'fatal_header_absent', 'lines': [], 'stop_reason': 'eof',
                        'truncated': False, 'unrecognized_lines': 0},
        'fatal_header_count': 0, 'omitted_fatal_blocks': 0, 'oversized_lines': 0,
        'omitted_nearby_error_lines': 0,
        'limits': {'stream_bytes': MAX_STREAM, 'line_bytes': MAX_LINE,
                   'export_bytes': MAX_PUBLIC, 'error_lines': MAX_ERRORS,
                   'error_line_bytes': MAX_ERROR_BYTES, 'cause_lines': MAX_CAUSE_LINES,
                   'cause_bytes': MAX_CAUSE_BYTES, 'nearby_lines': MAX_NEAR_LINES,
                   'nearby_bytes': MAX_NEAR_BYTES, 'header_blank_lines': MAX_HEADER_BLANKS},
        'root_cause': 'not_established', 'runtime_accepted': False, 'release_authorized': False,
    }
    block = result['cause_block']
    phase = 'before'
    blocked_label_before_header = False
    total = blank_count = cause_bytes = cause_lines = 0

    def stop(reason, *, truncated=False):
        nonlocal phase
        phase = 'done'
        block['stop_reason'] = reason
        block['truncated'] |= truncated

    for line_number, line in enumerate(_read_lines(reader, digest)):
        total = line['start_byte'] + line['byte_length']
        result['oversized_lines'] += int(line['oversized'])
        # Fatal matching is deliberately exact, including no ANSI/log prefix.
        raw_text = line['raw']
        if raw_text.endswith(b'\n'):
            raw_text = raw_text[:-1]
            if raw_text.endswith(b'\r'):
                raw_text = raw_text[:-1]
        original = raw_text.decode('utf-8', errors='replace')
        is_header = not line['oversized'] and original == FATAL_HEADER
        if blocked_label_before_header:
            result['fatal_header_count'] += int(is_header)
            continue
        if is_header:
            result['fatal_header_count'] += 1
            if result['fatal_header'] is not None:
                if phase != 'done':
                    stop('another_fatal_header', truncated=True)
                continue
            result['fatal_header'] = _row(line, FATAL_HEADER, limit=512)
            selected = [item for item in pending if line_number - item[0] <= MAX_NEAR_LINES
                        and line['start_byte'] - item[1]['start_byte'] <= MAX_NEAR_BYTES]
            result['error_lines'] = [item[1] for item in selected[-MAX_ERRORS:]]
            result['omitted_nearby_error_lines'] = max(0, len(selected) - MAX_ERRORS)
            phase = 'header'
            block['state'] = 'caused_by_header_missing'
            pending.clear()
            continue
        text = _clean(original)
        if phase == 'before':
            relevant = None if line['oversized'] else _relevant_error(text)
            if RESOURCE_LABEL.search(text):
                pending.clear()
                blocked_label_before_header = True
                block['state'] = 'source_label_content_omitted'
                stop('ambiguous_resource_label_before_header', truncated=True)
                continue
            if relevant is not None:
                pending.append((line_number, _row(line, relevant, limit=MAX_ERROR_BYTES)))
            elif text and not PRIVATE_LOCATION.search(text.strip()):
                pending.clear()
            continue
        if phase == 'done':
            continue
        if line['oversized']:
            stop('oversized_line', truncated=True)
            continue
        if phase == 'header':
            if original == '':
                blank_count += 1
                if blank_count > MAX_HEADER_BLANKS:
                    stop('header_blank_limit', truncated=True)
            elif original == 'Caused by:':
                block['state'] = 'cause_lines_absent'
                phase = 'cause'
                blank_count = 0
            else:
                stop('nonmatching_caused_by_header')
            continue
        stripped = text.strip()
        if not stripped:
            # A blank after the tree is its terminator; never jump across it.
            stop('blank_line')
            continue
        if RESOURCE_LABEL.search(stripped):
            block['state'] = 'source_label_content_omitted'
            stop('ambiguous_resource_label', truncated=True)
            continue
        if PRIVATE_LOCATION.search(stripped) or LOCATION_ONLY.match(stripped):
            stop('private_location_or_backtrace')
            continue
        if LOG_START.match(stripped) or ENVIRONMENT.match(stripped):
            stop('interleaved_log_or_environment')
            continue
        indent = len(original) - len(original.lstrip(' '))
        if not 2 <= indent <= 16 or indent % 2 or original[indent:][:1].isspace():
            stop('non_cause_line')
            continue
        if cause_lines == MAX_CAUSE_LINES:
            stop('cause_line_limit', truncated=True)
            continue
        if cause_bytes + line['byte_length'] > MAX_CAUSE_BYTES:
            stop('cause_byte_limit', truncated=True)
            continue
        category = _cause_variant(stripped)
        row = _row(line, stripped, limit=MAX_CAUSE_BYTES, category=category)
        # Redaction expansion and JSON escaping must also fit the block budget.
        used = sum(len(json.dumps(item['text'], ensure_ascii=True).encode('ascii')) for item in block['lines'])
        left = MAX_CAUSE_BYTES - used
        if left < len('" [truncated]"'):
            stop('cause_export_byte_limit', truncated=True)
            continue
        row['text'], shortened = _shorten(row['text'], left)
        row['truncated'] |= shortened
        block['truncated'] |= row['truncated']
        block['lines'].append(row)
        block['state'] = 'selected_sanitized_text'
        block['unrecognized_lines'] += int(category == 'unrecognized')
        cause_lines += 1
        cause_bytes += line['byte_length']
        if shortened:
            stop('cause_export_byte_limit', truncated=True)
    result['record'] = {'sha256': digest.hexdigest(), 'bytes': total}
    result['omitted_fatal_blocks'] = result['fatal_header_count'] - int(result['fatal_header'] is not None)
    require(len(canonical(result)) <= MAX_PUBLIC, 'present excerpt exceeds export byte limit')
    return result


def project_stream(path, *, expected_record):
    """Project one independent regular file, rejecting stale/full-byte bindings.

    expected_record must be exactly {'sha256': lower-case hex, 'bytes': int}.
    No excerpt is returned unless the complete stream matches this record.
    Caller decides where an authorized projection is stored; this API only reads.
    """
    require(isinstance(expected_record, dict) and set(expected_record) == {'sha256', 'bytes'}
            and isinstance(expected_record['sha256'], str)
            and re.fullmatch(r'[0-9a-f]{64}', expected_record['sha256']) is not None
            and type(expected_record['bytes']) is int and 0 <= expected_record['bytes'] <= MAX_STREAM,
            'invalid expected stream record')
    path = Path(path).absolute()
    # Inspect ancestors first so a Windows junction is rejected before a child
    # lookup traverses it. lstat also detects dangling links without resolving.
    for part in (*reversed(path.parents), path):
        details = part.lstat()
        require(not stat.S_ISLNK(details.st_mode)
                and not getattr(details, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT,
                'linked runtime stream or reparse point rejected')
    before = details
    require(stat.S_ISREG(before.st_mode) and before.st_nlink == 1
            and before.st_size == expected_record['bytes'], 'invalid bounded runtime stream')
    with path.open('rb') as reader:
        opened = os.fstat(reader.fileno())
        require((opened.st_dev, opened.st_ino) == (before.st_dev, before.st_ino), 'runtime stream changed while opening')
        value = _scan(reader)
        after = os.fstat(reader.fileno())
    require((opened.st_size, opened.st_mtime_ns, opened.st_ctime_ns)
            == (after.st_size, after.st_mtime_ns, after.st_ctime_ns), 'runtime stream changed while reading')
    require(value['record'] == expected_record, 'runtime stream differs from expected record')
    return value
