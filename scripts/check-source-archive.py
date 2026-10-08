#!/usr/bin/env python3
"""Check exact commit source archive contents without extracting them.

A local pass does not attest GitHub-hosted bytes or authorize publication.
"""
from __future__ import annotations
import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import stat
import subprocess
import tarfile
import tempfile
import zipfile

DENIED_PATH = 'assets/aircraft/light_single.glb'
DENIED_SHA256 = '8fc91894ea3f54d4226c3a30545de1947fa8a9fb0e013bb4bfec0b5e298effd9'
MAX_TOTAL = 512 * 1024 * 1024
MAX_MEMBERS = 20000
REQUIRED = ('LICENSE-MIT', 'LICENSE-APACHE', 'ATTRIBUTION.md',
            'docs/release/asset-rights-manifest.json',
            'assets/aircraft/swift_sport.glb', 'assets/aircraft/swift_sport.json')

def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def safe_name(value: str) -> str:
    name = value.removesuffix('/')
    parts = name.split('/')
    if not name or '\\' in name or '\x00' in name or name.startswith('/') or any(p in ('', '.', '..') for p in parts) or ':' in parts[0]:
        raise ValueError('unsafe archive member path')
    return name

def read_members(data: bytes) -> dict[str, bytes]:
    members = {}
    seen = set()
    total = 0
    def add(name, size, regular, directory, read):
        nonlocal total
        name = safe_name(name)
        if name in seen:
            raise ValueError('duplicate archive member')
        seen.add(name)
        if len(seen) > MAX_MEMBERS or size < 0 or size > MAX_TOTAL:
            raise ValueError('archive member limit exceeded')
        if directory:
            if size != 0 or not regular:
                raise ValueError('invalid or nonempty archive directory')
            return
        if not regular:
            raise ValueError('non-regular archive member')
        total += size
        if total > MAX_TOTAL:
            raise ValueError('archive size limit exceeded')
        payload = read()
        if len(payload) != size:
            raise ValueError('archive member size mismatch')
        members[name] = payload
    source = io.BytesIO(data)
    if zipfile.is_zipfile(source):
        with zipfile.ZipFile(source) as archive:
            for info in archive.infolist():
                mode = info.external_attr >> 16
                kind = stat.S_IFMT(mode)
                regular = kind == 0 or kind == (stat.S_IFDIR if info.is_dir() else stat.S_IFREG)
                add(info.filename, info.file_size, regular, info.is_dir(), lambda i=info: archive.read(i))
    else:
        source.seek(0)
        with tarfile.open(fileobj=source, mode='r:*') as archive:
            for info in archive:
                add(info.name, info.size, info.isfile() or info.isdir(), info.isdir(), lambda i=info: archive.extractfile(i).read())
    if not members:
        raise ValueError('empty archive')
    return members

def normalized_members(data: bytes) -> dict[str, bytes]:
    raw = read_members(data)
    roots = {PurePosixPath(name).parts[0] for name in raw}
    if len(roots) != 1 or any('/' not in name for name in raw):
        raise ValueError('archive must have exactly one enclosing root directory')
    return {name.split('/', 1)[1]: payload for name, payload in raw.items()}

def reject_identical_model(members: dict[str, bytes], depth: int = 0) -> None:
    for name, payload in members.items():
        if sha(payload) == DENIED_SHA256:
            raise ValueError('unresolved model bytes in archive')
        if name.lower().endswith(('.zip', '.tar', '.tar.gz', '.tgz')):
            if depth >= 3:
                raise ValueError('nested archive depth exceeded')
            reject_identical_model(read_members(payload), depth + 1)

def committed_archive(repo: Path, commit: str, fmt: str) -> tuple[str, bytes]:
    # Source-local info attributes and global/system attribute files must never
    # define the expected publication payload. A fresh bare repository reads
    # only committed .gitattributes while borrowing immutable object bytes.
    environment = {key: value for key, value in os.environ.items()
                   if not key.upper().startswith('GIT_')}
    environment.update(GIT_ATTR_NOSYSTEM='1', GIT_CONFIG_NOSYSTEM='1',
                       GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull)
    git = ['git', '-c', 'core.attributesFile=' + os.devnull]
    full_commit = subprocess.check_output(
        [*git, '-C', str(repo), 'rev-parse', '--verify', commit + '^{commit}'],
        text=True, env=environment).strip()
    objects = subprocess.check_output(
        [*git, '-C', str(repo), 'rev-parse', '--path-format=absolute', '--git-path', 'objects'],
        text=True, env=environment).strip()
    if '\n' in objects or '\r' in objects or not Path(objects).is_dir():
        raise ValueError('invalid source object directory')
    with tempfile.TemporaryDirectory(prefix='flightsim-source-export-') as temporary:
        root = Path(temporary)
        template = root / 'empty-template'
        template.mkdir()
        bare = root / 'archive.git'
        subprocess.check_call([*git, 'init', '--quiet', '--bare',
                               '--template=' + str(template), str(bare)], env=environment)
        (bare / 'objects/info/alternates').write_text(Path(objects).as_posix() + '\n', encoding='utf-8')
        payload = subprocess.check_output(
            [*git, '-C', str(bare), 'archive', '--format=' + fmt,
             '--prefix=source/', full_commit], env=environment)
    return full_commit, payload

def verify(repo: Path, commit: str, supplied: bytes | None = None, fmt: str = 'zip') -> dict:
    full_commit, expected_bytes = committed_archive(repo, commit, fmt)
    expected = normalized_members(expected_bytes)
    if DENIED_PATH in expected:
        raise ValueError('commit export policy still includes unresolved model')
    if any(path not in expected for path in REQUIRED):
        raise ValueError('required source or notice missing')
    observed_bytes = expected_bytes if supplied is None else supplied
    observed = normalized_members(observed_bytes)
    reject_identical_model(observed)
    if observed.keys() != expected.keys():
        raise ValueError('archive member set differs from reviewed commit export')
    for name in expected:
        if observed[name] != expected[name]:
            raise ValueError('archive member bytes differ from reviewed commit export: ' + name)
    rows = [{'path': name, 'bytes': len(data), 'sha256': sha(data)} for name, data in sorted(observed.items())]
    return {'schema_version': 1, 'status': 'source_archive_contents_verified', 'commit': full_commit,
            'evidence_scope': 'local_git_archive' if supplied is None else 'supplied_archive_bytes_origin_not_attested',
            'archive_sha256': sha(observed_bytes), 'archive_bytes': len(observed_bytes),
            'member_count': len(rows), 'members_sha256': sha(json.dumps(rows, sort_keys=True, separators=(',', ':')).encode()),
            'excluded_path': DENIED_PATH, 'identical_model_bytes_found': False,
            'hosted_origin_attested': False, 'publication_authorized': False}

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--commit', required=True)
    parser.add_argument('--archive', type=Path)
    parser.add_argument('--format', choices=('zip', 'tar'), default='zip')
    args = parser.parse_args()
    result = verify(args.repo, args.commit, args.archive.read_bytes() if args.archive else None, args.format)
    print(json.dumps(result, indent=2))
if __name__ == '__main__':
    main()
