#!/usr/bin/env python3
"""Read-only native runtime provenance, deliberately not rights or build approval.

Only bounded rustc print queries and the installed Microsoft vswhere are run.
No build, application launch, network request, installation, tracing or agreement
acceptance occurs. Candidate libraries/tools do not prove their actual use.
Raw command output and notice bytes stay private. The public projection contains
only fixed labels, validated identities, sizes and hashes, never host paths.
"""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import ntpath
import os
from pathlib import Path, PureWindowsPath, PurePosixPath
import re
import stat
import subprocess
import sys
import time

TARGET = 'x86_64-pc-windows-msvc'
TOOLCHAIN = '1.93.0'
RUST_COMMIT = '254b59607d4417e9dffbc307138ae5c86280fe4c'
LLVM_VERSION = '21.1.8'
KIND = 'native_runtime_facts_not_approval'
PRIVATE_NAME = 'runtime-facts-private.json'
PROJECTION_NAME = 'runtime-facts.json'
MAX_PUBLIC = 512 * 1024
MAX_PRIVATE = 2 * 1024 * 1024
MAX_QUERY = 256 * 1024
MAX_FILE = 256 * 1024 * 1024
MAX_NOTICE = 16 * 1024 * 1024
MAX_TOTAL = 1024 * 1024 * 1024
MAX_FILES = 1024
MAX_ENTRIES = 4096
MAX_VS = 8
MAX_VERSIONS = 16
MAX_RLIBS = 256
MAX_TRACE = 64 * 1024 * 1024
MAX_LINK_LINE = 256 * 1024
MAX_LINK_ARGS = 8192
UNKNOWN = 'not_established'
STATUSES = {'observed', 'missing', 'unreadable', 'rejected', 'not_established'}
QUERY_IDS = {'rustc-version', 'rustc-sysroot', 'rustc-default-cfg', 'rustc-recipe-cfg', 'vswhere'}
NOTICE_PATHS = {
    'copyright_library': 'share/doc/rust/COPYRIGHT-library.html',
    'copyright_html': 'share/doc/rust/COPYRIGHT.html',
    'copyright': 'share/doc/rust/COPYRIGHT',
    'license_mit': 'share/doc/rust/LICENSE-MIT',
    'license_apache': 'share/doc/rust/LICENSE-APACHE',
    'license_mit_text': 'share/doc/rust/licenses/MIT.txt',
    'license_apache_text': 'share/doc/rust/licenses/Apache-2.0.txt',
    'license_unicode': 'share/doc/rust/licenses/Unicode-3.0.txt',
    'license_bsd2': 'share/doc/rust/licenses/BSD-2-Clause.txt',
    'license_llvm_exception': 'share/doc/rust/licenses/LLVM-exception.txt',
    'license_ncsa': 'share/doc/rust/licenses/NCSA.txt',
    'license_isc': 'share/doc/rust/licenses/ISC.txt',
    'license_gpl2': 'share/doc/rust/licenses/GPL-2.0-only.txt',
    'license_gpl3': 'share/doc/rust/licenses/GPL-3.0-or-later.txt',
    'license_cc_by_sa': 'share/doc/rust/licenses/CC-BY-SA-4.0.txt',
    'license_gcc_exception': 'share/doc/rust/licenses/GCC-exception-3.1.txt',
    'license_ofl': 'share/doc/rust/licenses/OFL-1.1.txt',
    'source_copyright': 'lib/rustlib/src/rust/COPYRIGHT',
    'source_license_mit': 'lib/rustlib/src/rust/LICENSE-MIT',
    'source_license_apache': 'lib/rustlib/src/rust/LICENSE-APACHE',
    'source_unicode_license': 'lib/rustlib/src/rust/library/core/src/unicode/UNICODE-LICENSE.txt',
}
VC_LIBS = ('libcmt.lib', 'msvcrt.lib', 'libvcruntime.lib', 'vcruntime.lib',
           'libcpmt.lib', 'msvcprt.lib', 'oldnames.lib', 'concrt.lib')
SDK_LIBS = {'ucrt': ('ucrt.lib', 'libucrt.lib'),
            'um': ('kernel32.lib', 'advapi32.lib', 'bcrypt.lib', 'user32.lib',
                   'ws2_32.lib', 'shell32.lib', 'ole32.lib', 'oleaut32.lib',
                   'userenv.lib', 'ntdll.lib', 'dbghelp.lib', 'd3d12.lib', 'dxgi.lib')}
TERMS_NAMES = ('license.txt', 'license.rtf', 'license.htm', 'license.html', 'eula.txt', 'eula.rtf',
               'redist.txt', 'redist.rtf', 'redist.htm', 'redist.html')
LINK_REASONS = {'not_requested', 'missing_inputs', 'missing_fingerprint', 'ambiguous_fingerprint',
                'no_matching_command', 'ambiguous_command', 'unparseable_command', 'unresolved_linker',
                'missing_linker', 'output_identity_unavailable', 'output_hash_mismatch', 'observed'}
LIBRARY_ORIGINS = {'rust_target_libraries', 'cargo_release_deps', 'vc_runtime_x64',
                   'sdk_ucrt_x64', 'sdk_um_x64', 'unclassified'}
TRACE_COUNTERS = ('total_lines', 'link_module_lines', 'recognized_info_lines', 'quoted_command_lines',
                  'parsed_command_lines', 'rejected_command_lines', 'output_switch_commands',
                  'matching_output_commands')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(value):
    return (json.dumps(value, ensure_ascii=True, sort_keys=True, indent=2) + '\n').encode('ascii')


def no_links(path):
    raw = str(path)
    require(not raw.replace('/', '\\').startswith('\\\\'), 'UNC and device paths rejected')
    if re.match(r'^[A-Za-z]:', raw):
        require(':' not in raw[2:], 'alternate data stream rejected')
    for part in (Path(path), *Path(path).parents):
        try:
            info = part.lstat()
        except FileNotFoundError:
            continue
        require(not stat.S_ISLNK(info.st_mode) and not getattr(info, 'st_file_attributes', 0) & 0x400,
                'linked or reparse path rejected')


def file_record(path, maximum=MAX_FILE):
    path = Path(path)
    no_links(path)
    before = path.stat()
    require(stat.S_ISREG(before.st_mode) and 0 <= before.st_size <= maximum, 'file budget or type rejected')
    digest = hashlib.sha256()
    size = 0
    with path.open('rb') as stream:
        while block := stream.read(1024 * 1024):
            size += len(block)
            require(size <= maximum, 'file grew past budget')
            digest.update(block)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns, before.st_ino) ==
            (after.st_size, after.st_mtime_ns, after.st_ino) and size == before.st_size,
            'file changed while hashing')
    return {'sha256': digest.hexdigest(), 'bytes': size}


def valid_record(value):
    return (type(value) is dict and set(value) == {'sha256', 'bytes'}
            and isinstance(value['sha256'], str) and re.fullmatch('[0-9a-f]{64}', value['sha256'])
            and type(value['bytes']) is int and 0 <= value['bytes'] <= MAX_FILE)


def safe_token(value, maximum=160, pattern=r'[A-Za-z0-9_.+-]+'):
    require(isinstance(value, str) and 0 < len(value) <= maximum and re.fullmatch(pattern, value), 'unsafe identity')
    return value


def version(value):
    return safe_token(value, 64, r'[0-9]+(?:\.[0-9]+){1,5}')


def children(path):
    """Nonrecursive, bounded, no symlink/junction traversal."""
    no_links(path)
    if not path.is_dir():
        return []
    result = []
    with os.scandir(path) as items:
        for item in items:
            require(len(result) < MAX_ENTRIES, 'directory entry budget exceeded')
            result.append(Path(item.path))
    return sorted(result, key=lambda item: item.name.lower())


def numeric_directories(path):
    found = []
    for child in children(path):
        if re.fullmatch(r'[0-9]+(?:\.[0-9]+){1,5}', child.name):
            no_links(child)
            if child.is_dir():
                require(len(found) < MAX_VERSIONS, 'installed version budget exceeded')
                found.append(child)
    return found


def cfg(text):
    lines = text.splitlines()
    require(len(lines) <= 512 and all(len(line) <= 512 for line in lines), 'cfg budget exceeded')
    panic = [re.fullmatch(r'panic="(abort|unwind)"', line) for line in lines if line.startswith('panic=')]
    require(len(panic) == 1 and panic[0], 'missing or ambiguous panic cfg')
    require('target_arch="x86_64"' in lines and 'target_os="windows"' in lines
            and 'target_env="msvc"' in lines, 'wrong target cfg')
    return {'status': 'observed', 'panic_strategy': panic[0].group(1),
            'crt_static': 'target_feature="crt-static"' in lines}


def unknown_cfg():
    return {'status': UNKNOWN, 'panic_strategy': None, 'crt_static': None}


def validate_recipe_args(args):
    """Only settings for cfg queries, never arbitrary rustc output/build flags."""
    if args is None:
        return None
    require(type(args) in (list, tuple) and len(args) <= 12, 'recipe flag budget exceeded')
    args = list(args)
    index = 0
    seen = set()
    while index < len(args):
        require(index + 1 < len(args), 'unpaired recipe flag')
        flag, value = args[index:index + 2]
        require(flag in ('-D', '-C') and isinstance(value, str), 'unsupported recipe flag')
        if flag == '-D':
            key = 'warnings'
            require(value == 'warnings', 'unsupported lint setting')
        else:
            key = value.split('=', 1)[0]
            require(value in ('panic=abort', 'panic=unwind', 'target-feature=+crt-static',
                              'target-feature=-crt-static'), 'unsupported cfg setting')
        require(key not in seen, 'duplicate recipe setting')
        seen.add(key)
        index += 2
    return args


def file_versions(path):
    """Use the Windows fixed version resource, without executing the candidate."""
    if os.name != 'nt':
        return {'file_version': None, 'product_version': None}
    from ctypes import wintypes
    no_links(path)
    library = ctypes.WinDLL('version', use_last_error=True)
    library.GetFileVersionInfoSizeW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(wintypes.DWORD)]
    library.GetFileVersionInfoSizeW.restype = wintypes.DWORD
    library.GetFileVersionInfoW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID]
    library.GetFileVersionInfoW.restype = wintypes.BOOL
    library.VerQueryValueW.argtypes = [wintypes.LPCVOID, wintypes.LPCWSTR, ctypes.POINTER(wintypes.LPVOID), ctypes.POINTER(wintypes.UINT)]
    library.VerQueryValueW.restype = wintypes.BOOL
    ignored = wintypes.DWORD()
    size = library.GetFileVersionInfoSizeW(str(path), ctypes.byref(ignored))
    if not 0 < size <= MAX_QUERY:
        return {'file_version': None, 'product_version': None}
    buffer = ctypes.create_string_buffer(size)
    require(library.GetFileVersionInfoW(str(path), 0, size, buffer), 'unreadable version resource')
    pointer, length = wintypes.LPVOID(), wintypes.UINT()
    require(library.VerQueryValueW(buffer, '\\', ctypes.byref(pointer), ctypes.byref(length))
            and length.value >= 52, 'missing fixed version resource')
    words = ctypes.cast(pointer, ctypes.POINTER(wintypes.DWORD))
    require(words[0] == 0xFEEF04BD, 'invalid fixed version resource')
    def formatted(high, low):
        return '.'.join(map(str, (high >> 16, high & 65535, low >> 16, low & 65535)))
    return {'file_version': formatted(words[2], words[3]), 'product_version': formatted(words[4], words[5])}


class Collector:
    def __init__(self, private, env):
        self.private = Path(private)
        # os.environ uppercases keys on Windows; copying it into a plain dict
        # loses case-insensitive lookup. Keep the same canonical key spelling
        # for discovery, subprocesses and replay, including synthetic inputs.
        # https://docs.python.org/3/library/os.html#os.environ
        self.env = {}
        for key, value in env.items():
            require(isinstance(key, str) and isinstance(value, str), 'invalid environment entry')
            key = key.upper()
            require(key not in self.env or self.env[key] == value, 'conflicting environment key aliases')
            self.env[key] = value
        # https://rust-lang.github.io/rustup/environment-variables.html
        # A missing toolchain is an unknown fact, never an implicit download.
        self.env['RUSTUP_AUTO_INSTALL'] = '0'
        self.files, self.queries = [], []
        self.total = 0

    def observe(self, path, *, copy=False, versions=False):
        """Missing/inaccessible facts remain unknown; unsafe inputs fail closed."""
        path = Path(path)
        result = {'status': 'missing', 'record': None, 'evidence_id': None,
                  'file_version': None, 'product_version': None}
        try:
            no_links(path)
            if not path.exists():
                return result
            record = file_record(path, MAX_NOTICE if copy else MAX_FILE)
            require(len(self.files) < MAX_FILES and self.total + record['bytes'] <= MAX_TOTAL,
                    'aggregate file budget exceeded')
            identity = 'file-' + str(len(self.files)).zfill(4)
            snapshot = None
            if copy:
                snapshot = identity + '.bin'
                # Bounded exact original bytes, private only.
                data = path.read_bytes()
                require(len(data) == record['bytes'] and hashlib.sha256(data).hexdigest() == record['sha256'],
                        'notice changed before snapshot')
                (self.private / snapshot).write_bytes(data)
            resource = file_versions(path) if versions else {'file_version': None, 'product_version': None}
            require(file_record(path, MAX_NOTICE if copy else MAX_FILE) == record, 'file changed during observation')
            self.files.append({'id': identity, 'source': str(path.absolute()), 'snapshot': snapshot,
                               'record': record, **resource})
            self.total += record['bytes']
            return {'status': 'observed', 'record': record, 'evidence_id': identity, **resource}
        except (PermissionError, OSError):
            result['status'] = 'unreadable'
            return result

    def query(self, identity, command):
        require(identity in QUERY_IDS and identity not in [row['id'] for row in self.queries], 'invalid query ID')
        out = self.private / (identity + '.stdout')
        err = self.private / (identity + '.stderr')
        outcome, code = 'launch_error', None
        with out.open('xb') as stdout, err.open('xb') as stderr:
            try:
                # No shell, global environment edits, compiler wrappers or flags
                # that produce artifacts. Kill only this read-only query on limit.
                process = subprocess.Popen(command, stdout=stdout, stderr=stderr,
                                           env=self.env, stdin=subprocess.DEVNULL)
            except OSError:
                pass
            else:
                end = time.monotonic() + 30
                while process.poll() is None:
                    if out.stat().st_size > MAX_QUERY or err.stat().st_size > MAX_QUERY or time.monotonic() >= end:
                        process.kill()
                        process.wait(timeout=10)
                        outcome = 'bounded_failure'
                        break
                    time.sleep(0.05)
                else:
                    outcome = 'succeeded' if process.returncode == 0 else 'failed'
                code = process.returncode
        require(out.stat().st_size <= MAX_QUERY and err.stat().st_size <= MAX_QUERY, 'query output budget exceeded')
        row = {'id': identity, 'command': list(command), 'outcome': outcome, 'exit_code': code,
               'stdout': file_record(out, MAX_QUERY), 'stderr': file_record(err, MAX_QUERY)}
        self.queries.append(row)
        return out.read_text(encoding='utf-8-sig') if outcome == 'succeeded' else None

    def terms(self, root, sdk_version=None):
        rows = []
        # Only these known directories, not a recursive SDK or VS scan.
        locations = [('root', ''), ('licenses', 'Licenses'), ('redist', 'Redist'),
                     ('licenses_english', 'Licenses/1033'), ('ide_english', 'Common7/IDE/1033')]
        if sdk_version is not None:
            version(sdk_version)
            locations.extend([('version_licenses', 'Licenses/' + sdk_version),
                              ('version_redist', 'Redist/' + sdk_version)])
        for directory_id, relative in locations:
            for child in children(root / relative):
                if child.name.lower() in TERMS_NAMES:
                    rows.append({'location': directory_id, 'name': child.name.lower(),
                                 **self.observe(child, copy=True)})
        return rows


def compiler_identity(raw):
    identity = {'status': UNKNOWN, 'release': None, 'commit': None, 'host': None, 'llvm': None,
                'matches_required_identity': False}
    if raw is not None:
        fields = {}
        for label in ('release', 'commit-hash', 'host', 'LLVM version'):
            matches = re.findall(r'^' + re.escape(label) + r': (.+)$', raw, re.M)
            require(len(matches) == 1, 'incomplete compiler identity')
            fields[label] = matches[0]
        identity = {'status': 'observed', 'release': version(fields['release']),
                    'commit': safe_token(fields['commit-hash'], 40, '[0-9a-f]{40}'),
                    'host': safe_token(fields['host']), 'llvm': version(fields['LLVM version']),
                    'matches_required_identity': fields == {'release': TOOLCHAIN, 'commit-hash': RUST_COMMIT,
                                                           'host': TARGET, 'LLVM version': LLVM_VERSION}}
    return identity


def rust_facts(collector, prefix, recipe_args):
    identity = compiler_identity(collector.query('rustc-version', [*prefix, '-vV']))
    default, recipe = unknown_cfg(), unknown_cfg()
    for query_id, flags in [('rustc-default-cfg', []), ('rustc-recipe-cfg', recipe_args)]:
        if flags is None:
            continue
        raw = collector.query(query_id, [*prefix, '--print', 'cfg', '--target', TARGET, *flags])
        observed = cfg(raw) if raw is not None else unknown_cfg()
        if query_id == 'rustc-default-cfg':
            default = observed
        else:
            recipe = observed
    notices, rlibs = [], []
    rlib_status = UNKNOWN
    sysroot_status = UNKNOWN
    raw = collector.query('rustc-sysroot', [*prefix, '--print', 'sysroot'])
    if raw is not None:
        require(len(raw) <= 4096 and len(raw.splitlines()) == 1, 'invalid sysroot response')
        root = Path(raw.strip())
        require(root.is_absolute(), 'sysroot must be absolute')
        no_links(root)
        if root.is_dir():
            sysroot_status = 'observed'
            notices = [{'id': name, **collector.observe(root / relative, copy=True)}
                       for name, relative in sorted(NOTICE_PATHS.items())]
            library_directory = root / 'lib/rustlib' / TARGET / 'lib'
            no_links(library_directory)
            rlib_status = 'observed' if library_directory.is_dir() else 'missing'
            for path in children(library_directory):
                if path.suffix.lower() == '.rlib':
                    require(len(rlibs) < MAX_RLIBS, 'rlib count exceeded')
                    safe_token(path.name, 160, r'lib[A-Za-z0-9_]+-[0-9a-f]+\.rlib')
                    rlibs.append({'name': path.name, **collector.observe(path)})
        else:
            sysroot_status = 'missing'
    return {'identity': identity, 'sysroot_status': sysroot_status, 'notices': notices, 'target_rlibs': rlibs,
            'target_rlibs_status': rlib_status,
            'target_defaults': default, 'recipe_cfg_probe': recipe,
            'recipe_settings_supplied': recipe_args is not None,
            'actual_compiler_invocation_observed': False, 'actual_build_panic_strategy': None,
            'actual_build_crt_static': None, 'rlibs_prove_linked_inclusion': False}


def microsoft_facts(collector):
    result = {'vswhere': None, 'visual_studio_candidates': [], 'sdk_candidates': [],
              'installed_candidates_exhaustive': False, 'selected_linker': UNKNOWN, 'selected_sdk': UNKNOWN,
              'static_contributions': UNKNOWN, 'licensed_product_entitlement': UNKNOWN,
              'applicable_terms_acceptance': UNKNOWN, 'dynamic_module_coverage': UNKNOWN}
    # This is the official installed location, not a PATH lookup/download.
    program = collector.env.get('PROGRAMFILES(X86)')
    if not program:
        return result
    root = Path(program)
    require(root.is_absolute(), 'invalid ProgramFiles root')
    no_links(root)
    vswhere = root / 'Microsoft Visual Studio/Installer/vswhere.exe'
    result['vswhere'] = collector.observe(vswhere, versions=True)
    if result['vswhere']['status'] == 'observed':
        raw = collector.query('vswhere', [str(vswhere), '-all', '-prerelease', '-products', '*', '-format', 'json', '-utf8'])
        if raw is not None:
            installations = json.loads(raw)
            require(type(installations) is list and len(installations) <= MAX_VS, 'VS instance budget exceeded')
            seen = set()
            for installation in installations:
                require(type(installation) is dict, 'invalid VS instance')
                install = Path(installation['installationPath'])
                require(install.is_absolute() and str(install).lower() not in seen, 'invalid or duplicate VS root')
                seen.add(str(install).lower())
                no_links(install)
                anchor = install / 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt'
                row = {'product': safe_token(installation['productId']),
                       'version': version(installation['installationVersion']),
                       'default_toolset_hint': collector.observe(anchor, copy=True),
                       'terms_candidates': collector.terms(install), 'toolsets': []}
                for toolset in numeric_directories(install / 'VC/Tools/MSVC'):
                    tool = {'version': version(toolset.name), 'linkers': [], 'runtime_libraries': [],
                            'terms_candidates': collector.terms(toolset)}
                    for host in ('Hostx64', 'Hostx86'):
                        tool['linkers'].append({'host': host.lower(), 'target': 'x64',
                                                **collector.observe(toolset / 'bin' / host / 'x64/link.exe', versions=True)})
                    tool['runtime_libraries'] = [{'name': name, **collector.observe(toolset / 'lib/x64' / name)}
                                                 for name in VC_LIBS]
                    row['toolsets'].append(tool)
                result['visual_studio_candidates'].append(row)
    # Installed candidates under the standard SDK layout. This does not infer
    # the SDK selected by rustc/link.exe or treat environment hints as proof.
    sdk = root / 'Windows Kits/10'
    for directory in numeric_directories(sdk / 'Lib'):
        libraries = []
        for family, names in SDK_LIBS.items():
            libraries.extend({'family': family, 'name': name,
                              **collector.observe(directory / family / 'x64' / name)} for name in names)
        result['sdk_candidates'].append({'version': version(directory.name), 'architecture': 'x64',
                                         'libraries': libraries, 'terms_candidates': collector.terms(sdk, directory.name)})
    return result


def debug_command_tokens(text):
    """Parse the bounded quoted-only Windows Command Debug format from rustc.

    This is NOT shell parsing and never executes the trace. Unknown escaping or
    the different Unix environment-prefix format is rejected, not guessed.
    Rust 1.93.0's Windows std::process::Command Debug prints each regular
    OsString with Debug quoting, separated by spaces; the MSVC linker appends
    the output path to one /OUT: argument. This is not CreateProcess quoting.
    """
    require(len(text) <= MAX_LINK_LINE, 'link command line budget exceeded')
    result, offset = [], 0
    escapes = {'\\': '\\', '"': '"', 'n': '\n', 'r': '\r', 't': '\t', '0': '\0'}
    while offset < len(text):
        require(len(result) < MAX_LINK_ARGS and text[offset] == '"', 'unsupported command debug format')
        offset += 1
        token = []
        while offset < len(text) and text[offset] != '"':
            char = text[offset]; offset += 1
            if char == '\\':
                require(offset < len(text), 'unterminated debug escape')
                escaped = text[offset]; offset += 1
                if escaped == 'u':
                    match = re.match(r'\{([0-9a-fA-F]{1,6})\}', text[offset:])
                    require(match is not None, 'invalid Unicode debug escape')
                    code = int(match[1], 16)
                    require(code <= 0x10ffff and not 0xd800 <= code <= 0xdfff, 'invalid Unicode scalar')
                    token.append(chr(code)); offset += len(match[0])
                else:
                    require(escaped in escapes, 'unknown debug escape')
                    token.append(escapes[escaped])
            else:
                token.append(char)
        require(offset < len(text), 'unterminated debug token')
        offset += 1
        value = ''.join(token)
        require(len(value) <= 16384 and not any(ord(char) < 32 or ord(char) == 127 for char in value), 'unsafe debug token')
        result.append(value)
        if offset < len(text):
            require(text[offset] == ' ', 'invalid debug token separator')
            while offset < len(text) and text[offset] == ' ':
                offset += 1
    require(result, 'empty debug command')
    return result


def same_path(first, second):
    return os.path.normcase(os.path.abspath(first)) == os.path.normcase(os.path.abspath(second))


def library_path_origin(value, collector, executable):
    """Only canonical known layouts become origin labels; no host path export."""
    path = Path(value)
    if not path.is_absolute():
        return {'origin': 'unclassified', 'version': None}
    if same_path(path, executable.parent / 'deps'):
        return {'origin': 'cargo_release_deps', 'version': None}
    for observed in collector.files:
        source = Path(observed['source'])
        if source.name.endswith('.rlib') and source.parent.name == 'lib' and source.parent.parent.name == TARGET:
            if same_path(path, source.parent):
                return {'origin': 'rust_target_libraries', 'version': TOOLCHAIN}
        if source.name.lower() == 'link.exe' and len(source.parents) >= 4:
            toolset = source.parents[3]
            if re.fullmatch(r'[0-9]+(?:\.[0-9]+){1,5}', toolset.name) and same_path(path, toolset / 'lib/x64'):
                return {'origin': 'vc_runtime_x64', 'version': toolset.name}
        if source.suffix.lower() == '.lib' and len(source.parents) >= 4 and source.parent.name.lower() == 'x64':
            family, sdk_version, libdir = source.parents[1], source.parents[2], source.parents[3]
            if family.name in SDK_LIBS and libdir.name.lower() == 'lib' and same_path(path, source.parent):
                return {'origin': 'sdk_' + family.name + '_x64', 'version': version(sdk_version.name)}
    return {'origin': 'unclassified', 'version': None}


def final_link_facts(collector, trace=None, executable=None):
    """Bind one logged constructed final-app command to an audited executable.

    The caller must first audit the frozen build/executable with the existing
    recipe auditor. A log before exec is not a process-execution witness. The
    Cargo deps output must be byte-identical to the audited final executable.
    """
    result = {'status': UNKNOWN, 'reason': 'not_requested', 'trace': None, 'executable': None,
              'fingerprint': None, 'link_output': None, 'linker': None,
              'trace_observation': None,
              'installed_candidate_evidence_id': None,
              'constructed_final_command_observed': False, 'linker_process_execution_observed': False,
              'requested_libraries': [], 'rlib_input_names': [], 'explicit_library_paths': [],
              'actual_static_membership': UNKNOWN, 'implicit_sdk_selection': UNKNOWN}
    if trace is None and executable is None:
        return result
    require(trace is not None and executable is not None, 'trace and audited executable are required together')
    trace, executable = Path(trace), Path(executable)
    require(trace.is_absolute() and executable.is_absolute() and executable.name == 'flightsim-app.exe'
            and executable.parent.name == 'release' and executable.parent.parent.name == TARGET,
            'invalid audited executable layout')
    result['trace'] = collector.observe(trace)
    result['executable'] = collector.observe(executable)
    if result['trace']['status'] != 'observed' or result['executable']['status'] != 'observed':
        result['reason'] = 'missing_inputs'; return result
    require(result['trace']['record']['bytes'] <= MAX_TRACE, 'link trace budget exceeded')
    fingerprints = []
    for root in children(executable.parent / '.fingerprint'):
        if re.fullmatch('flightsim-app-[0-9a-f]{16}', root.name):
            no_links(root)
            path = root / 'bin-flightsim-app.json'
            no_links(path)
            if path.is_file():
                fingerprints.append(path)
    if len(fingerprints) != 1:
        result['reason'] = 'missing_fingerprint' if not fingerprints else 'ambiguous_fingerprint'; return result
    fingerprint = fingerprints[0]
    result['fingerprint'] = collector.observe(fingerprint, copy=True)
    if result['fingerprint']['status'] != 'observed':
        result['reason'] = 'missing_fingerprint'; return result
    suffix = fingerprint.parent.name.removeprefix('flightsim-app-')
    deps_output = executable.parent / 'deps' / ('flightsim_app-' + suffix + '.exe')
    matches, malformed = [], False
    observation = dict.fromkeys(TRACE_COUNTERS, 0)
    with trace.open('r', encoding='utf-8-sig') as stream:
        while line := stream.readline(MAX_LINK_LINE + 2):
            require(len(line) <= MAX_LINK_LINE + 1, 'trace line budget exceeded')
            observation['total_lines'] += 1
            if 'rustc_codegen_ssa::back::link' in line:
                observation['link_module_lines'] += 1
            match = re.fullmatch(r' *INFO rustc_codegen_ssa::back::link:? (.*)', line.rstrip('\r\n'))
            if match is None:
                continue
            observation['recognized_info_lines'] += 1
            if not match[1].startswith('"'):
                continue
            observation['quoted_command_lines'] += 1
            try:
                args = debug_command_tokens(match[1])
            except ValueError:
                observation['rejected_command_lines'] += 1
                malformed = True; continue
            observation['parsed_command_lines'] += 1
            outputs = [arg[5:] for arg in args[1:] if arg.upper().startswith('/OUT:')]
            if outputs:
                observation['output_switch_commands'] += 1
            if any(Path(path).is_absolute() and (same_path(path, executable) or same_path(path, deps_output)) for path in outputs):
                observation['matching_output_commands'] += 1
                if len(outputs) != 1:
                    malformed = True
                else:
                    matches.append((args, Path(outputs[0])))
            require(len(matches) <= 16, 'matching command budget exceeded')
    # Counts diagnose unsupported prefixes/quoting without publishing any
    # command text, path, environment, or inferred explanation of a mismatch.
    result['trace_observation'] = observation
    if malformed:
        result['reason'] = 'unparseable_command'; return result
    if len(matches) != 1:
        result['reason'] = 'no_matching_command' if not matches else 'ambiguous_command'; return result
    args, output = matches[0]
    result['link_output'] = collector.observe(output)
    if result['link_output']['status'] != 'observed':
        result['reason'] = 'output_identity_unavailable'; return result
    if result['link_output']['record'] != result['executable']['record']:
        result['reason'] = 'output_hash_mismatch'; return result
    linker = Path(args[0])
    if not linker.is_absolute() or linker.name.lower() != 'link.exe':
        result['reason'] = 'unresolved_linker'; return result
    installed = [row['id'] for row in collector.files if same_path(row['source'], linker)]
    result['linker'] = collector.observe(linker, versions=True)
    if result['linker']['status'] != 'observed':
        result['reason'] = 'missing_linker'; return result
    libraries, rlibs, origins = set(), set(), {}
    for arg in args[1:]:
        if arg.upper().startswith('/LIBPATH:'):
            origin = library_path_origin(arg[9:], collector, executable)
            origins[(origin['origin'], origin['version'])] = origin
        elif arg.upper().startswith('/DEFAULTLIB:'):
            name = arg[12:]
            if not name.lower().endswith('.lib'):
                name += '.lib'
            libraries.add(safe_token(name.lower(), 160, r'[A-Za-z0-9_.+-]+\.lib'))
        elif not arg.startswith('/') and arg.lower().endswith('.lib'):
            libraries.add(safe_token(ntpath.basename(arg).lower(), 160, r'[A-Za-z0-9_.+-]+\.lib'))
        elif arg.endswith('.rlib'):
            rlibs.add(safe_token(ntpath.basename(arg), 160, r'lib[A-Za-z0-9_]+-[0-9a-f]+\.rlib'))
    require(len(libraries) <= 1024 and len(rlibs) <= 1024 and len(origins) <= 128, 'link input budget exceeded')
    result.update(status='observed', reason='observed', constructed_final_command_observed=True,
                  installed_candidate_evidence_id=installed[0] if len(installed) == 1 else None,
                  requested_libraries=sorted(libraries), rlib_input_names=sorted(rlibs),
                  explicit_library_paths=[origins[key] for key in sorted(origins, key=lambda item: (item[0], item[1] or ''))])
    return result


class ReplayCollector(Collector):
    """Read-only re-enumeration using byte-bound queries, without subprocesses."""
    def __init__(self, private, manifest):
        root = manifest['discovery_roots']['program_files_x86']
        super().__init__(private, {'PROGRAMFILES(X86)': root} if root is not None else {})
        self.expected_files = manifest['files']
        self.expected_queries = {row['id']: row for row in manifest['queries']}

    def observe(self, path, *, copy=False, versions=False):
        observed = super().observe(path, copy=False, versions=versions)
        if observed['status'] == 'observed':
            row = self.files[-1]
            row['snapshot'] = row['id'] + '.bin' if copy else None
            index = len(self.files) - 1
            require(index < len(self.expected_files) and row == self.expected_files[index],
                    'discovered source path or version changed')
        return observed

    def query(self, identity, command):
        row = self.expected_queries.get(identity)
        require(row is not None and row['command'] == command, 'replayed query differs')
        self.queries.append(row)
        return ((self.private / (identity + '.stdout')).read_text(encoding='utf-8-sig')
                if row['outcome'] == 'succeeded' else None)


def validate_origins(manifest, records, query_text):
    """Bind labels to derived paths even when only offline receipts are checked."""
    used = set()
    def pure(value):
        require(isinstance(value, str) and 0 < len(value) <= 4096
                and not any(ord(char) < 32 for char in value), 'invalid private path')
        path = PureWindowsPath(value) if re.match(r'^[A-Za-z]:[\\/]|^\\\\', value) else PurePosixPath(value)
        require(path.is_absolute(), 'private discovery path is not absolute')
        require(not value.replace('/', '\\').startswith('\\\\')
                and (not re.match(r'^[A-Za-z]:', value) or ':' not in value[2:]), 'unsafe private discovery path')
        return path
    def bind(observed, expected):
        if observed is not None and observed['status'] == 'observed':
            row = records[observed['evidence_id']]
            require(pure(row['source']) == expected, 'observation label differs from discovered source path')
            used.add(row['id'])
    def terms(rows, root, sdk_version=None):
        locations = {'root': root, 'licenses': root / 'Licenses', 'redist': root / 'Redist',
                     'licenses_english': root / 'Licenses/1033', 'ide_english': root / 'Common7/IDE/1033'}
        if sdk_version is not None:
            locations.update(version_licenses=root / 'Licenses' / sdk_version,
                             version_redist=root / 'Redist' / sdk_version)
        for row in rows:
            require(row['location'] in locations, 'invalid versioned terms location')
            # Windows matching is case-insensitive. POSIX synthetic fixtures use
            # the exact lower-case names accepted by the test discovery layout.
            expected = locations[row['location']] / row['name']
            if row['status'] == 'observed':
                source = pure(records[row['evidence_id']]['source'])
                require(str(source).lower() == str(expected).lower(), 'terms candidate differs from source path')
                used.add(row['evidence_id'])
    raw_root = query_text('rustc-sysroot')
    rust = manifest['rust']
    if raw_root is None:
        require(not rust['notices'] and not rust['target_rlibs'], 'unobserved sysroot has file identities')
    else:
        require(len(raw_root.splitlines()) == 1, 'invalid raw sysroot')
        root = pure(raw_root.strip())
        for row in rust['notices']:
            bind(row, root / NOTICE_PATHS[row['id']])
        for row in rust['target_rlibs']:
            bind(row, root / 'lib/rustlib' / TARGET / 'lib' / row['name'])
    microsoft = manifest['microsoft']
    raw_program = manifest['discovery_roots']['program_files_x86']
    if raw_program is None:
        require(microsoft['vswhere'] is None and not microsoft['visual_studio_candidates'] and not microsoft['sdk_candidates'],
                'missing Microsoft root has candidates')
    else:
        program = pure(raw_program)
        bind(microsoft['vswhere'], program / 'Microsoft Visual Studio/Installer/vswhere.exe')
        installations = []
        if microsoft['vswhere'] is not None and microsoft['vswhere']['status'] == 'observed':
            raw = query_text('vswhere')
            installations = json.loads(raw) if raw is not None else []
        require(len(installations) == len(microsoft['visual_studio_candidates']), 'VS instance membership differs')
        for install, row in zip(installations, microsoft['visual_studio_candidates']):
            root = pure(install['installationPath'])
            bind(row['default_toolset_hint'], root / 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt')
            terms(row['terms_candidates'], root)
            for toolset in row['toolsets']:
                toolroot = root / 'VC/Tools/MSVC' / toolset['version']
                terms(toolset['terms_candidates'], toolroot)
                for linker in toolset['linkers']:
                    host = {'hostx64': 'Hostx64', 'hostx86': 'Hostx86'}[linker['host']]
                    bind(linker, toolroot / 'bin' / host / 'x64/link.exe')
                for library in toolset['runtime_libraries']:
                    bind(library, toolroot / 'lib/x64' / library['name'])
        for sdk in microsoft['sdk_candidates']:
            root = program / 'Windows Kits/10'
            terms(sdk['terms_candidates'], root, sdk['version'])
            for library in sdk['libraries']:
                bind(library, root / 'Lib' / sdk['version'] / library['family'] / 'x64' / library['name'])
    inputs, link = manifest['linker_inputs'], manifest['final_link']
    if inputs['trace'] is None or inputs['executable'] is None:
        require(inputs == {'trace': None, 'executable': None} and link['reason'] == 'not_requested', 'incomplete linker inputs')
    else:
        bind(link['trace'], pure(inputs['trace']))
        executable = pure(inputs['executable'])
        bind(link['executable'], executable)
        if link['fingerprint'] is not None and link['fingerprint']['status'] == 'observed':
            row = records[link['fingerprint']['evidence_id']]
            fingerprint = pure(row['source'])
            require(fingerprint.name == 'bin-flightsim-app.json' and re.fullmatch('flightsim-app-[0-9a-f]{16}', fingerprint.parent.name)
                    and fingerprint.parent.parent == executable.parent / '.fingerprint', 'foreign app fingerprint')
            bind(link['fingerprint'], fingerprint)
            if link['link_output'] is not None and link['link_output']['status'] == 'observed':
                output = pure(records[link['link_output']['evidence_id']]['source'])
                suffix = fingerprint.parent.name.removeprefix('flightsim-app-')
                require(output in (executable, executable.parent / 'deps' / ('flightsim_app-' + suffix + '.exe')), 'foreign final linker output')
                bind(link['link_output'], output)
        if link['linker'] is not None and link['linker']['status'] == 'observed':
            source = pure(records[link['linker']['evidence_id']]['source'])
            require(source.name.lower() == 'link.exe', 'unexpected linker executable')
            bind(link['linker'], source)
            if link['installed_candidate_evidence_id'] is not None:
                require(pure(records[link['installed_candidate_evidence_id']]['source']) == source,
                        'logged linker differs from installed candidate path')
    require(used == set(records), 'unreferenced private observations')


def public_observation(value):
    require(type(value) is dict and set(value) == {'status', 'record', 'evidence_id', 'file_version', 'product_version'},
            'invalid file observation')
    require(value['status'] in STATUSES, 'invalid file status')
    require((value['status'] == 'observed') == (value['record'] is not None), 'invalid observed file binding')
    if value['record'] is not None:
        require(valid_record(value['record']) and re.fullmatch('file-[0-9]{4}', value['evidence_id']), 'invalid file binding')
    else:
        require(value['evidence_id'] is None, 'missing file has evidence ID')
        require(value['file_version'] is None and value['product_version'] is None, 'unobserved file has version')
    for key in ('file_version', 'product_version'):
        if value[key] is not None:
            version(value[key])
    return value


def validate_projection(value):
    """Closed schema plus token-only strings protects the public export boundary."""
    require(type(value) is dict and set(value) == {'schema_version', 'kind', 'source_sha', 'target', 'requested_toolchain',
            'native_windows', 'rust', 'microsoft', 'private_manifest', 'query_bindings', 'release_authorized',
            'native_runtime_coverage_complete', 'dependency_review_approved', 'final_link'}, 'unknown projection field')
    require(type(value['schema_version']) is int and value['schema_version'] == 1 and value['kind'] == KIND
            and value['target'] == TARGET and value['requested_toolchain'] == TOOLCHAIN
            and value['native_windows'] is True, 'invalid runtime projection identity')
    require(re.fullmatch('[0-9a-f]{40}', value['source_sha']), 'invalid source identity')
    require(all(value[key] is False for key in ('release_authorized', 'native_runtime_coverage_complete', 'dependency_review_approved')),
            'runtime facts cannot grant approval')
    require(valid_record(value['private_manifest']), 'invalid private binding')
    require(type(value['query_bindings']) is list and len(value['query_bindings']) <= len(QUERY_IDS), 'invalid query bindings')
    seen = set()
    for row in value['query_bindings']:
        require(set(row) == {'id', 'outcome', 'exit_code', 'stdout', 'stderr'} and row['id'] in QUERY_IDS
                and row['id'] not in seen and row['outcome'] in ('succeeded', 'failed', 'launch_error', 'bounded_failure')
                and (row['exit_code'] is None or type(row['exit_code']) is int)
                and valid_record(row['stdout']) and valid_record(row['stderr'])
                and row['stdout']['bytes'] <= MAX_QUERY and row['stderr']['bytes'] <= MAX_QUERY, 'invalid query binding')
        seen.add(row['id'])
    rust = value['rust']
    require(set(rust) == {'identity', 'sysroot_status', 'notices', 'target_rlibs', 'target_rlibs_status', 'target_defaults', 'recipe_cfg_probe',
                         'recipe_settings_supplied', 'actual_compiler_invocation_observed', 'actual_build_panic_strategy',
                         'actual_build_crt_static', 'rlibs_prove_linked_inclusion'}, 'unknown Rust field')
    ident = rust['identity']
    require(set(ident) == {'status', 'release', 'commit', 'host', 'llvm', 'matches_required_identity'} and ident['status'] in ('observed', UNKNOWN),
            'invalid Rust identity')
    require(type(ident['matches_required_identity']) is bool, 'invalid Rust match flag')
    if ident['status'] == 'observed':
        version(ident['release']); version(ident['llvm']); safe_token(ident['host'])
        require(re.fullmatch('[0-9a-f]{40}', ident['commit']), 'invalid Rust commit')
        require(ident['matches_required_identity'] == (ident['release'] == TOOLCHAIN and ident['commit'] == RUST_COMMIT
                and ident['host'] == TARGET and ident['llvm'] == LLVM_VERSION), 'inconsistent Rust identity match')
    else:
        require(all(ident[key] is None for key in ('release', 'commit', 'host', 'llvm')) and ident['matches_required_identity'] is False,
                'unknown compiler cannot match')
    require(rust['sysroot_status'] in STATUSES and rust['target_rlibs_status'] in STATUSES and type(rust['recipe_settings_supplied']) is bool
            and rust['actual_compiler_invocation_observed'] is False and rust['rlibs_prove_linked_inclusion'] is False,
            'unsupported Rust inference')
    require(rust['actual_build_panic_strategy'] is None and rust['actual_build_crt_static'] is None, 'cfg probe is not an actual build invocation')
    for key in ('target_defaults', 'recipe_cfg_probe'):
        row = rust[key]
        require(set(row) == {'status', 'panic_strategy', 'crt_static'}, 'unknown cfg field')
        require(row == unknown_cfg() or (row['status'] == 'observed' and row['panic_strategy'] in ('abort', 'unwind')
                and type(row['crt_static']) is bool), 'invalid cfg')
    require(rust['recipe_settings_supplied'] or rust['recipe_cfg_probe'] == unknown_cfg(), 'unsupplied recipe cfg')
    require(len(rust['notices']) <= len(NOTICE_PATHS) and len(rust['target_rlibs']) <= MAX_RLIBS, 'Rust projection budget exceeded')
    for rows, key in ((rust['notices'], 'id'), (rust['target_rlibs'], 'name')):
        seen = set()
        for row in rows:
            label = row[key]
            require(label not in seen, 'duplicate Rust file')
            seen.add(label)
            if key == 'id':
                require(label in NOTICE_PATHS, 'unknown Rust notice')
            else:
                safe_token(label, 160, r'lib[A-Za-z0-9_]+-[0-9a-f]+\.rlib')
            public_observation({k: v for k, v in row.items() if k != key})
    ms = value['microsoft']
    require(set(ms) == {'vswhere', 'visual_studio_candidates', 'sdk_candidates', 'installed_candidates_exhaustive',
                       'selected_linker', 'selected_sdk', 'static_contributions', 'licensed_product_entitlement',
                       'applicable_terms_acceptance', 'dynamic_module_coverage'}, 'unknown Microsoft field')
    require(ms['installed_candidates_exhaustive'] is False and all(ms[key] == UNKNOWN for key in
            ('selected_linker', 'selected_sdk', 'static_contributions', 'licensed_product_entitlement',
             'applicable_terms_acceptance', 'dynamic_module_coverage')), 'unsupported Microsoft inference')
    if ms['vswhere'] is not None:
        public_observation(ms['vswhere'])
    def terms(rows):
        require(type(rows) is list and len(rows) <= 7 * len(TERMS_NAMES), 'terms budget exceeded')
        for row in rows:
            require(row['location'] in ('root', 'licenses', 'redist', 'version_licenses', 'version_redist', 'licenses_english', 'ide_english')
                    and row['name'] in TERMS_NAMES, 'unknown terms candidate')
            public_observation({k: v for k, v in row.items() if k not in ('location', 'name')})
    require(len(ms['visual_studio_candidates']) <= MAX_VS and len(ms['sdk_candidates']) <= MAX_VERSIONS, 'Microsoft budget exceeded')
    for install in ms['visual_studio_candidates']:
        require(set(install) == {'product', 'version', 'default_toolset_hint', 'terms_candidates', 'toolsets'}, 'unknown VS field')
        safe_token(install['product']); version(install['version']); terms(install['terms_candidates'])
        public_observation(install['default_toolset_hint'])
        require(len(install['toolsets']) <= MAX_VERSIONS, 'toolset budget exceeded')
        for toolset in install['toolsets']:
            require(set(toolset) == {'version', 'linkers', 'runtime_libraries', 'terms_candidates'}, 'unknown toolset field')
            version(toolset['version']); terms(toolset['terms_candidates'])
            require(len(toolset['linkers']) <= 2 and len(toolset['runtime_libraries']) <= len(VC_LIBS), 'VC file budget exceeded')
            for linker in toolset['linkers']:
                require(linker['host'] in ('hostx64', 'hostx86') and linker['target'] == 'x64', 'invalid linker architecture')
                public_observation({k: v for k, v in linker.items() if k not in ('host', 'target')})
            for library in toolset['runtime_libraries']:
                require(library['name'] in VC_LIBS, 'unexpected VC library')
                public_observation({k: v for k, v in library.items() if k != 'name'})
    for sdk in ms['sdk_candidates']:
        require(set(sdk) == {'version', 'architecture', 'libraries', 'terms_candidates'} and sdk['architecture'] == 'x64', 'invalid SDK field')
        version(sdk['version']); terms(sdk['terms_candidates'])
        require(len(sdk['libraries']) <= sum(map(len, SDK_LIBS.values())), 'SDK file budget exceeded')
        for library in sdk['libraries']:
            require(library['family'] in SDK_LIBS and library['name'] in SDK_LIBS[library['family']], 'unexpected SDK library')
            public_observation({k: v for k, v in library.items() if k not in ('family', 'name')})
    link = value['final_link']
    require(set(link) - {'trace_observation'} == {'status', 'reason', 'trace', 'executable', 'fingerprint', 'link_output', 'linker',
                         'installed_candidate_evidence_id',
                         'constructed_final_command_observed', 'linker_process_execution_observed',
                         'requested_libraries', 'rlib_input_names', 'explicit_library_paths',
                         'actual_static_membership', 'implicit_sdk_selection'}, 'unknown final link field')
    require(link['status'] in ('observed', UNKNOWN) and link['reason'] in LINK_REASONS
            and link['constructed_final_command_observed'] is (link['status'] == 'observed')
            and (link['reason'] == 'observed') == (link['status'] == 'observed')
            and link['linker_process_execution_observed'] is False
            and link['actual_static_membership'] == UNKNOWN and link['implicit_sdk_selection'] == UNKNOWN,
            'unsupported final link inference')
    for key in ('trace', 'executable', 'fingerprint', 'link_output', 'linker'):
        if link[key] is not None:
            public_observation(link[key])
        if link['status'] == 'observed':
            require(link[key] is not None and link[key]['status'] == 'observed', 'missing final link binding')
    # Optional only for compatibility with already sealed schema-v1 captures.
    # New captures always carry null (not scanned) or this closed count record.
    if 'trace_observation' in link:
        observation = link['trace_observation']
        scanned = link['reason'] not in ('not_requested', 'missing_inputs', 'missing_fingerprint', 'ambiguous_fingerprint')
        require((observation is not None) == scanned, 'inconsistent trace observation availability')
        if observation is not None:
            require(type(observation) is dict and set(observation) == set(TRACE_COUNTERS)
                    and all(type(count) is int and 0 <= count <= MAX_TRACE for count in observation.values()),
                    'invalid trace observation')
            require(all(link[key] is not None and link[key]['status'] == 'observed'
                        for key in ('trace', 'executable', 'fingerprint')), 'unbound trace observation')
            require(link['trace']['record']['bytes'] >= observation['total_lines'] >= observation['link_module_lines']
                    >= observation['recognized_info_lines'] >= observation['quoted_command_lines']
                    == observation['parsed_command_lines'] + observation['rejected_command_lines']
                    and observation['parsed_command_lines'] >= observation['output_switch_commands']
                    >= observation['matching_output_commands'], 'inconsistent trace observation counts')
            if link['status'] == 'observed':
                require(observation['matching_output_commands'] == 1 and observation['rejected_command_lines'] == 0,
                        'observed command lacks unique parsed output')
    for key, pattern in (('requested_libraries', r'[A-Za-z0-9_.+-]+\.lib'),
                         ('rlib_input_names', r'lib[A-Za-z0-9_]+-[0-9a-f]+\.rlib')):
        require(type(link[key]) is list and len(link[key]) <= 1024 and sorted(set(link[key])) == link[key], 'invalid link input list')
        for item in link[key]:
            safe_token(item, 160, pattern)
    require(type(link['explicit_library_paths']) is list and len(link['explicit_library_paths']) <= 128, 'library path budget exceeded')
    for row in link['explicit_library_paths']:
        require(set(row) == {'origin', 'version'} and row['origin'] in LIBRARY_ORIGINS, 'unknown library origin')
        if row['version'] is not None:
            version(row['version'])
    if link['status'] == 'observed':
        require(link['executable']['record'] == link['link_output']['record'], 'final output differs')
        candidate_id = link['installed_candidate_evidence_id']
        if candidate_id is not None:
            candidates = [row for install in ms['visual_studio_candidates'] for toolset in install['toolsets']
                          for row in toolset['linkers'] if row['evidence_id'] == candidate_id and row['status'] == 'observed']
            require(len(candidates) == 1 and all(candidates[0][key] == link['linker'][key]
                    for key in ('record', 'file_version', 'product_version')), 'logged linker lacks matching installed candidate')
    else:
        require(not link['requested_libraries'] and not link['rlib_input_names'] and not link['explicit_library_paths']
                and link['installed_candidate_evidence_id'] is None, 'unknown command cannot project inputs')
    def tokens(item):
        if isinstance(item, str):
            safe_token(item)
        elif type(item) is dict:
            for key, child in item.items():
                safe_token(key); tokens(child)
        elif type(item) is list:
            for child in item:
                tokens(child)
        else:
            require(item is None or type(item) in (bool, int), 'unsupported JSON scalar')
    tokens(value)
    require(len(canonical(value)) <= MAX_PUBLIC, 'public projection budget exceeded')
    return value


def collect_runtime_facts(private, *, source_sha, env=None, recipe_cfg_args=None, linker_trace=None, audited_executable=None):
    """Create a fresh private directory and return its validated public projection."""
    require(sys.platform == 'win32', 'native Windows is required')
    require(re.fullmatch('[0-9a-f]{40}', source_sha), 'invalid source SHA')
    recipe_cfg_args = validate_recipe_args(recipe_cfg_args)
    private = Path(private)
    no_links(private)
    require(not private.exists(), 'fresh private directory required')
    private.mkdir(parents=True)
    collector = Collector(private, os.environ if env is None else env)
    rust = rust_facts(collector, ['rustc', '+' + TOOLCHAIN], recipe_cfg_args)
    microsoft = microsoft_facts(collector)
    final_link = final_link_facts(collector, linker_trace, audited_executable)
    manifest = {'schema_version': 1, 'source_sha': source_sha, 'recipe_cfg_args': recipe_cfg_args,
                'files': collector.files, 'queries': collector.queries, 'rust': rust, 'microsoft': microsoft,
                'final_link': final_link,
                'discovery_roots': {'program_files_x86': collector.env.get('PROGRAMFILES(X86)')},
                'linker_inputs': {'trace': str(linker_trace) if linker_trace is not None else None,
                                  'executable': str(audited_executable) if audited_executable is not None else None}}
    data = canonical(manifest)
    require(len(data) <= MAX_PRIVATE, 'private manifest budget exceeded')
    (private / PRIVATE_NAME).write_bytes(data)
    value = project(private)
    (private / PROJECTION_NAME).write_bytes(canonical(value))
    return value


def project(private, *, recheck_installed=True):
    """Reproduce the only public projection from the bounded private manifest."""
    private = Path(private)
    file_record(private / PRIVATE_NAME, MAX_PRIVATE)
    raw = (private / PRIVATE_NAME).read_bytes()
    manifest = json.loads(raw)
    require(canonical(manifest) == raw, 'noncanonical private manifest')
    value = {'schema_version': 1, 'kind': KIND, 'source_sha': manifest['source_sha'], 'target': TARGET,
             'requested_toolchain': TOOLCHAIN, 'native_windows': True, 'rust': manifest['rust'], 'microsoft': manifest['microsoft'],
             'final_link': manifest['final_link'],
             'private_manifest': file_record(private / PRIVATE_NAME, MAX_PRIVATE),
             'query_bindings': [{key: item[key] for key in ('id', 'outcome', 'exit_code', 'stdout', 'stderr')}
                                for item in manifest['queries']],
             'release_authorized': False, 'native_runtime_coverage_complete': False, 'dependency_review_approved': False}
    validate_projection(value)
    validate_private(private, value, recheck_installed=recheck_installed)
    return value


def validate_private(private, projection, *, recheck_installed=True):
    """Revalidate byte bindings. Requires original files unless snapshots suffice.

    The default detects changed rlibs/linkers/libraries as well as raw evidence.
    Offline revalidation may explicitly skip installed files; it then verifies
    only captured receipts/snapshots and cannot assert current machine identity.
    """
    validate_projection(projection)
    private = Path(private)
    require(file_record(private / PRIVATE_NAME, MAX_PRIVATE) == projection['private_manifest'], 'private manifest changed')
    raw = (private / PRIVATE_NAME).read_bytes()
    manifest = json.loads(raw)
    require(canonical(manifest) == raw and set(manifest) == {'schema_version', 'source_sha', 'recipe_cfg_args', 'files', 'queries', 'rust', 'microsoft',
                                                         'final_link', 'discovery_roots', 'linker_inputs'}
            and type(manifest['schema_version']) is int and manifest['schema_version'] == 1, 'invalid private manifest')
    validate_recipe_args(manifest['recipe_cfg_args'])
    require(manifest['source_sha'] == projection['source_sha'] and manifest['rust'] == projection['rust']
            and manifest['microsoft'] == projection['microsoft'] and manifest['final_link'] == projection['final_link'],
            'projection differs from private facts')
    require(set(manifest['discovery_roots']) == {'program_files_x86'}
            and set(manifest['linker_inputs']) == {'trace', 'executable'}, 'unknown private discovery input')
    require(type(manifest['files']) is list and len(manifest['files']) <= MAX_FILES, 'private file budget exceeded')
    seen, total = set(), 0
    records = {}
    for row in manifest['files']:
        require(set(row) == {'id', 'source', 'snapshot', 'record', 'file_version', 'product_version'}
                and re.fullmatch('file-[0-9]{4}', row['id']) and row['id'] not in seen and valid_record(row['record']), 'invalid private file')
        seen.add(row['id']); total += row['record']['bytes']; records[row['id']] = row
        require(total <= MAX_TOTAL and isinstance(row['source'], str) and len(row['source']) <= 4096, 'private file budget exceeded')
        if row['snapshot'] is not None:
            require(row['snapshot'] == row['id'] + '.bin' and row['record']['bytes'] <= MAX_NOTICE, 'invalid private snapshot')
            require(file_record(private / row['snapshot'], MAX_NOTICE) == row['record'], 'private notice snapshot changed')
        if recheck_installed:
            path = Path(row['source'])
            require(path.is_absolute() and file_record(path) == row['record'], 'installed file changed')
    def bindings(item):
        if type(item) is dict:
            if 'evidence_id' in item and item['evidence_id'] is not None:
                row = records.get(item['evidence_id'])
                require(row is not None and all(item[key] == row[key] for key in ('record', 'file_version', 'product_version')),
                        'unbound file observation')
            for child in item.values():
                bindings(child)
        elif type(item) is list:
            for child in item:
                bindings(child)
    bindings(projection)
    require(len(manifest['queries']) == len(projection['query_bindings']), 'query binding count differs')
    for row, binding in zip(manifest['queries'], projection['query_bindings']):
        require(set(row) == {*binding, 'command'} and {key: row[key] for key in binding} == binding, 'query projection differs')
        for stream in ('stdout', 'stderr'):
            require(file_record(private / (row['id'] + '.' + stream), MAX_QUERY) == row[stream], 'private query output changed')
    queries = {row['id']: row for row in manifest['queries']}
    prefix = ['rustc', '+' + TOOLCHAIN]
    expected = {'rustc-version': [*prefix, '-vV'], 'rustc-sysroot': [*prefix, '--print', 'sysroot'],
                'rustc-default-cfg': [*prefix, '--print', 'cfg', '--target', TARGET]}
    recipe_args = manifest['recipe_cfg_args']
    if recipe_args is not None:
        expected['rustc-recipe-cfg'] = [*prefix, '--print', 'cfg', '--target', TARGET, *recipe_args]
    if projection['microsoft']['vswhere'] is not None and projection['microsoft']['vswhere']['status'] == 'observed':
        vswhere = records[projection['microsoft']['vswhere']['evidence_id']]['source']
        expected['vswhere'] = [vswhere, '-all', '-prerelease', '-products', '*', '-format', 'json', '-utf8']
    require(set(queries) == set(expected) and all(queries[key]['command'] == command for key, command in expected.items()),
            'unexpected private query command')
    def query_text(identity):
        if queries[identity]['outcome'] != 'succeeded':
            return None
        require(queries[identity]['exit_code'] == 0, 'successful query has nonzero exit code')
        return (private / (identity + '.stdout')).read_text(encoding='utf-8-sig')
    require(compiler_identity(query_text('rustc-version')) == projection['rust']['identity'], 'compiler facts differ from raw output')
    for query_id, key in (('rustc-default-cfg', 'target_defaults'), ('rustc-recipe-cfg', 'recipe_cfg_probe')):
        raw_cfg = query_text(query_id) if query_id in queries else None
        require((cfg(raw_cfg) if raw_cfg is not None else unknown_cfg()) == projection['rust'][key], 'cfg differs from raw output')
    if 'vswhere' in queries:
        raw_vs = query_text('vswhere')
        installed = json.loads(raw_vs) if raw_vs is not None else []
        require(type(installed) is list and len(installed) == len(projection['microsoft']['visual_studio_candidates']), 'VS observations differ')
        for raw_install, observed in zip(installed, projection['microsoft']['visual_studio_candidates']):
            require(raw_install['productId'] == observed['product'] and raw_install['installationVersion'] == observed['version'],
                    'VS identity differs from raw output')
    validate_origins(manifest, records, query_text)
    if recheck_installed:
        replay = ReplayCollector(private, manifest)
        require(rust_facts(replay, prefix, recipe_args) == manifest['rust'], 'Rust discoveries changed')
        require(microsoft_facts(replay) == manifest['microsoft'], 'Microsoft discoveries changed')
        replay_link = final_link_facts(replay, manifest['linker_inputs']['trace'], manifest['linker_inputs']['executable'])
        if 'trace_observation' not in manifest['final_link']:
            del replay_link['trace_observation']
        require(replay_link == manifest['final_link'], 'final link observations changed')
        require(replay.files == manifest['files'] and len(replay.queries) == len(manifest['queries']), 'private discovery membership changed')
    return projection


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--private', type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--recipe-warnings-only', action='store_true', help='Probe -D warnings separately from target defaults; does not prove actual compiler invocation')
    parser.add_argument('--linker-trace', type=Path, help='Optional private analytic build.stderr containing Rust linker info logs')
    parser.add_argument('--audited-executable', type=Path, help='Optional frozen flightsim-app.exe already verified by the existing build auditor')
    parser.add_argument('--validate', action='store_true')
    args = parser.parse_args()
    try:
        if args.validate:
            path = args.private / PROJECTION_NAME
            require(file_record(path, MAX_PUBLIC)['bytes'] <= MAX_PUBLIC, 'projection budget exceeded')
            raw = path.read_bytes(); value = json.loads(raw)
            require(canonical(value) == raw and value['source_sha'] == args.source_sha, 'noncanonical or wrong source projection')
            validate_private(args.private, value)
        else:
            collect_runtime_facts(args.private, source_sha=args.source_sha,
                                  recipe_cfg_args=['-D', 'warnings'] if args.recipe_warnings_only else None,
                                  linker_trace=args.linker_trace, audited_executable=args.audited_executable)
        print('Runtime facts bound; coverage and release approval remain false.')
        return 0
    except (ValueError, OSError, KeyError, TypeError, UnicodeError, subprocess.SubprocessError):
        print('Runtime fact collection or revalidation failed; raw evidence remains private.', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
