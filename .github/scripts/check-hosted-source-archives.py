#!/usr/bin/env python3
"""Read public GitHub snapshots and verify their bytes with the source checker.

No archive extraction, credentials, publication, or application launch occurs.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import time
import urllib.error
import urllib.request

REPOSITORY = 'Xenoah/flightsim-claude'
REPOSITORY_ID = 1318879072
API = 'https://api.github.com/repos/' + REPOSITORY
MAX_ARCHIVE = 64 * 1024 * 1024
MAX_JSON = 1024 * 1024
MAX_REPORT = 16 * 1024
TRANSFER_SECONDS = 120
SHA = re.compile(r'[0-9a-f]{40}')
TAG = re.compile(r'v[0-9][0-9A-Za-z.+-]{0,99}')
# Independently reviewed checker from 7d8c877; a new checker requires new review.
CHECKER_SHA256 = '25c88911e4db103225e813d0d8aaa33d1d6d6d4aad97255b515b6be1ab4b1ccc'
EVIDENCE_FILES = (
    '.github/scripts/check-hosted-source-archives.py',
    '.github/workflows/hosted-source-archives.yml',
    '.github/hosted-source-archive-target.json',
)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        return None


def fetch(url: str, limit: int, redirect_url: str | None = None) -> tuple[bytes, str]:
    """Follow only the one exact expected GitHub-to-codeload HTTPS redirect."""
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    started = time.monotonic()
    current = url
    for attempt in range(2):
        request = urllib.request.Request(current, headers={
            'User-Agent': 'flightsim-hosted-source-archive-check',
            'Accept': 'application/vnd.github+json' if current.startswith(API) else '*/*',
            'Accept-Encoding': 'identity',
        })
        try:
            response = opener.open(request, timeout=15)
        except urllib.error.HTTPError as error:
            with error:
                if (attempt != 0 or redirect_url is None or error.code != 302
                        or error.headers.get('Location') != redirect_url):
                    raise ValueError('GitHub request failed or unexpected redirect') from None
            current = redirect_url
            continue
        with response:
            if response.status != 200 or response.geturl() != current:
                raise ValueError('unexpected GitHub response origin or status')
            if response.headers.get('Content-Encoding', 'identity') != 'identity':
                raise ValueError('unexpected HTTP content encoding')
            declared = response.headers.get('Content-Length')
            if declared is not None and (not declared.isascii() or not declared.isdigit()
                                         or not 0 < int(declared) <= limit):
                raise ValueError('response content length exceeds limit or is invalid')
            body = bytearray()
            while True:
                if time.monotonic() - started > TRANSFER_SECONDS:
                    raise ValueError('GitHub transfer time limit exceeded')
                block = response.read1(min(65536, limit + 1 - len(body)))
                if time.monotonic() - started > TRANSFER_SECONDS:
                    raise ValueError('GitHub transfer time limit exceeded')
                if not block:
                    break
                body.extend(block)
                if len(body) > limit:
                    raise ValueError('actual response bytes exceed limit')
            if not body or (declared is not None and len(body) != int(declared)):
                raise ValueError('empty or truncated GitHub response')
            return bytes(body), current
    raise ValueError('GitHub redirect limit exceeded')


def metadata(path: str = '') -> dict:
    data, _ = fetch(API + path, MAX_JSON)
    value = json.loads(data)
    if not isinstance(value, dict):
        raise ValueError('expected GitHub metadata object')
    return value


def public_repository() -> None:
    value = metadata()
    if (value.get('id') != REPOSITORY_ID or value.get('full_name') != REPOSITORY
            or value.get('private') is not False or value.get('visibility') != 'public'):
        raise ValueError('repository identity or public visibility changed')


def resolve_tag(tag: str, commit: str) -> list[str]:
    value = metadata('/git/ref/tags/' + tag)
    if value.get('ref') != 'refs/tags/' + tag:
        raise ValueError('exact tag ref was not returned')
    target = value.get('object', {})
    chain = []
    for _ in range(5):
        object_sha = target.get('sha', '')
        if not isinstance(object_sha, str) or SHA.fullmatch(object_sha) is None:
            raise ValueError('invalid tag object identity')
        chain.append(object_sha)
        if target.get('type') == 'commit':
            if object_sha != commit:
                raise ValueError('tag does not resolve to expected commit')
            return chain
        if target.get('type') != 'tag':
            raise ValueError('tag target is not a commit or annotated tag')
        value = metadata('/git/tags/' + object_sha)
        if value.get('sha') != object_sha:
            raise ValueError('annotated tag identity mismatch')
        target = value.get('object', {})
    raise ValueError('annotated tag depth exceeded')


def evidence_request(root: Path, evidence_commit: str) -> tuple[dict, dict]:
    if SHA.fullmatch(evidence_commit) is None:
        raise ValueError('invalid evidence commit')
    head = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
    if head != evidence_commit:
        raise ValueError('evidence checkout differs from workflow commit')
    hashes = {}
    for name in EVIDENCE_FILES:
        committed = subprocess.check_output(['git', '-C', str(root), 'show', evidence_commit + ':' + name])
        if (root / name).read_bytes() != committed:
            raise ValueError('evidence working files differ from workflow commit')
        hashes[name] = hashlib.sha256(committed).hexdigest()
    target = json.loads((root / EVIDENCE_FILES[2]).read_bytes())
    if (not isinstance(target, dict) or set(target) != {'schema_version', 'repository', 'commit', 'tree', 'tag'}
            or target['schema_version'] != 1 or target['repository'] != REPOSITORY
            or not isinstance(target['commit'], str) or SHA.fullmatch(target['commit']) is None
            or not isinstance(target['tree'], str) or SHA.fullmatch(target['tree']) is None
            or not isinstance(target['tag'], str) or (target['tag'] and TAG.fullmatch(target['tag']) is None)):
        raise ValueError('invalid reviewed source target')
    return target, {'commit': evidence_commit, 'files_sha256': hashes}


def verify(repo: Path, commit: str, tag: str = '', expected_tree: str | None = None) -> dict:
    if SHA.fullmatch(commit) is None or (tag and TAG.fullmatch(tag) is None):
        raise ValueError('expected full lowercase commit SHA and optional bounded release tag')
    checker_path = repo / 'scripts/check-source-archive.py'
    committed_checker = subprocess.check_output(
        ['git', '-C', str(repo), 'show', commit + ':scripts/check-source-archive.py'])
    if hashlib.sha256(committed_checker).hexdigest() != CHECKER_SHA256:
        raise ValueError('target content checker differs from reviewed checker')
    if checker_path.read_bytes() != committed_checker:
        raise ValueError('working content checker differs from expected commit')
    spec = importlib.util.spec_from_file_location('source_archive', checker_path)
    checker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(checker)
    local = subprocess.check_output(
        ['git', '-C', str(repo), 'rev-parse', '--verify', commit + '^{commit}'], text=True).strip()
    tree = subprocess.check_output(
        ['git', '-C', str(repo), 'rev-parse', '--verify', commit + '^{tree}'], text=True).strip()
    if local != commit or SHA.fullmatch(tree) is None:
        raise ValueError('local source identity mismatch')
    if expected_tree is not None and tree != expected_tree:
        raise ValueError('local source tree differs from reviewed target')
    public_repository()
    remote = metadata('/git/commits/' + commit)
    if remote.get('sha') != commit or remote.get('tree', {}).get('sha') != tree:
        raise ValueError('GitHub commit/tree differs from local source')
    tag_chain = resolve_tag(tag, commit) if tag else None
    refs = [commit] + (['refs/tags/' + tag] if tag else [])
    rows = []
    for ref in refs:
        for extension, transport_format, expected_format in (
                ('zip', 'zip', 'zip'), ('tar.gz', 'tar.gz', 'tar')):
            requested = f'https://github.com/{REPOSITORY}/archive/{ref}.{extension}'
            destination = f'https://codeload.github.com/{REPOSITORY}/{transport_format}/{ref}'
            payload, final_url = fetch(requested, MAX_ARCHIVE, destination)
            # Do not accept the other supported archive format under a wrong URL.
            if not payload.startswith(b'PK\x03\x04' if extension == 'zip' else b'\x1f\x8b'):
                raise ValueError('hosted archive format mismatch')
            result = checker.verify(repo, commit, supplied=payload, fmt=expected_format)
            rows.append({
                'ref': ref, 'format': extension, 'requested_url': requested, 'final_url': final_url,
                'archive_bytes': result['archive_bytes'], 'archive_sha256': result['archive_sha256'],
                'member_count': result['member_count'], 'members_sha256': result['members_sha256'],
            })
    if tag and resolve_tag(tag, commit) != tag_chain:
        raise ValueError('tag changed during hosted archive verification')
    public_repository()
    if len({row['members_sha256'] for row in rows}) != 1:
        raise ValueError('hosted archive content manifests differ')
    report = {
        'schema_version': 1, 'status': 'github_hosted_source_archives_verified',
        'repository': REPOSITORY, 'repository_id': REPOSITORY_ID, 'visibility': 'public',
        'commit': commit, 'tree': tree, 'tag': tag or None, 'tag_object_chain': tag_chain,
        'verified_at_utc': datetime.now(timezone.utc).isoformat(),
        'transport': 'anonymous_https_github_exact_redirect', 'archives': rows,
        'excluded_path': checker.DENIED_PATH, 'identical_model_bytes_found': False,
        'checker_sha256': checker.sha((repo / 'scripts/check-source-archive.py').read_bytes()),
        'hosted_origin_attested': True, 'publication_authorized': False,
    }
    # Fixed keys and validated values only; never export archives, paths or responses.
    if len(json.dumps(report).encode()) > MAX_REPORT:
        raise ValueError('text evidence exceeds limit')
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--evidence-commit', required=True)
    output = parser.add_mutually_exclusive_group(required=True)
    output.add_argument('--report', type=Path)
    output.add_argument('--github-output', type=Path)
    args = parser.parse_args()
    target, evidence = evidence_request(Path(__file__).resolve().parents[2], args.evidence_commit)
    if args.github_output:
        with args.github_output.open('a', encoding='utf-8') as output:
            output.write('source_commit=' + target['commit'] + '\n')
        return
    report = verify(args.repo, target['commit'], target['tag'], target['tree'])
    report['evidence_source'] = evidence
    encoded = json.dumps(report, indent=2) + '\n'
    if len(encoded.encode()) > MAX_REPORT:
        raise ValueError('formatted text evidence exceeds limit')
    # Non-overwriting success-only receipt. Failed runs cannot publish stale success.
    with args.report.open('x', encoding='utf-8') as output:
        output.write(encoded)
    print(encoded, end='')


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        # HTTP exception text can contain response material. Keep failure output bounded.
        raise SystemExit('Hosted archive verification failed (' + type(error).__name__ + ')') from None
