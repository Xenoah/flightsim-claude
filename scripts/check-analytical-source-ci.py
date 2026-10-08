#!/usr/bin/env python3
"""Read-only GitHub CI gate for one exact analytical qualification source.

Only the latest CI run returned for this exact SHA on main's push event can
qualify. Ancestor success, PR success and another repository cannot qualify.
This is source CI evidence, never runtime, review or publication authorization.
No polling, redirects or unbounded pagination: dispatch after CI has succeeded.
Requires GH_TOKEN (or GITHUB_TOKEN) with read access to Actions.
"""
from __future__ import annotations

import argparse
import http.client
import json
import os
from pathlib import Path
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


API_ROOT = 'https://api.github.com'
WORKFLOW_NAME = 'CI'
WORKFLOW_PATH = '.github/workflows/ci.yml'
MAX_RUNS = 100
MAX_RESPONSE_BYTES = 2 * 1024 * 1024
MAX_RECEIPT_BYTES = 2048
REQUEST_TIMEOUT = 15
MAX_REQUESTS = 3
TOTAL_TIMEOUT = 45
MAX_ID = 2**63 - 1


class GateError(ValueError):
    """Only static, safe descriptions belong in these errors."""


def require(condition, message):
    if not condition:
        raise GateError(message)


def positive_id(value):
    return type(value) is int and 1 <= value <= MAX_ID


def validate_inputs(repository, source_sha):
    require(isinstance(repository, str) and len(repository) <= 140
            and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9-]{0,38}/[A-Za-z0-9_.-]{1,100}', repository)
            and repository.split('/')[1] not in ('.', '..'), 'invalid repository identity')
    require(isinstance(source_sha, str) and re.fullmatch(r'[0-9a-f]{40}', source_sha),
            'source must be one full lowercase commit SHA')


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'GitHub response contains duplicate fields')
        result[key] = value
    return result


def invalid_constant(_value):
    raise GateError('GitHub response contains a non-JSON number')


class NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise GateError('GitHub API redirect refused')


class GitHubReader:
    """GET only, fixed official origin, bounded calls, bodies and read deadline."""

    def __init__(self, token):
        require(isinstance(token, str) and 0 < len(token) <= 4096
                and re.fullmatch(r'[!-~]+', token), 'missing or invalid GitHub token')
        self._token = token
        self._opener = urllib.request.build_opener(NoRedirects())
        self._deadline = time.monotonic() + TOTAL_TIMEOUT
        self._requests = 0

    def get(self, path):
        require(isinstance(path, str) and path.startswith('/repos/')
                and not any(c in path for c in ('\r', '\n', '#', '\\')),
                'invalid GitHub API request path')
        self._requests += 1
        remaining = self._deadline - time.monotonic()
        require(self._requests <= MAX_REQUESTS and remaining > 0,
                'GitHub API request budget exceeded')
        request = urllib.request.Request(API_ROOT + path, method='GET', headers={
            'Authorization': 'Bearer ' + self._token,
            'Accept': 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28',
            'User-Agent': 'flightsim-analytical-source-ci',
        })
        try:
            with self._opener.open(request, timeout=min(REQUEST_TIMEOUT, remaining)) as response:
                require(response.status == 200, 'GitHub API did not return success')
                require(response.headers.get_content_type() == 'application/json',
                        'GitHub API did not return JSON')
                data = bytearray()
                while True:
                    require(time.monotonic() < self._deadline, 'GitHub API deadline exceeded')
                    # read1 performs at most one underlying read, so a trickle
                    # cannot hide an expired total deadline inside read(n).
                    chunk = response.read1(min(65536, MAX_RESPONSE_BYTES + 1 - len(data)))
                    if not chunk:
                        break
                    data.extend(chunk)
                    require(len(data) <= MAX_RESPONSE_BYTES, 'GitHub API response too large')
                require(time.monotonic() < self._deadline, 'GitHub API deadline exceeded')
            value = json.loads(data.decode('utf-8'), object_pairs_hook=unique_object,
                               parse_constant=invalid_constant)
            require(isinstance(value, dict), 'GitHub API response must be an object')
            return value
        except GateError:
            raise
        except (OSError, urllib.error.URLError, http.client.HTTPException, ValueError, RecursionError):
            # Never echo exception bodies, remote messages, URLs or credentials.
            raise GateError('GitHub API request or JSON decoding failed') from None


def validate_run(run, repository, source_sha, workflow_id):
    require(isinstance(run, dict), 'invalid source CI run shape')
    require(positive_id(run.get('id')) and positive_id(run.get('run_attempt')),
            'invalid source CI run identity')
    require(type(run.get('workflow_id')) is int and run['workflow_id'] == workflow_id,
            'source CI workflow ID differs')
    require(run.get('name') == WORKFLOW_NAME and run.get('path') == WORKFLOW_PATH,
            'source CI workflow name or path differs')
    require(run.get('head_sha') == source_sha, 'source CI did not test the exact source SHA')
    require(run.get('head_branch') == 'main' and run.get('event') == 'push',
            'source CI must be a main push run')
    require(run.get('pull_requests') == [], 'source CI must not be a pull request run')
    base, head = run.get('repository'), run.get('head_repository')
    require(isinstance(base, dict) and isinstance(head, dict)
            and base.get('full_name') == repository and head.get('full_name') == repository
            and positive_id(base.get('id')) and positive_id(head.get('id'))
            and base['id'] == head['id'], 'source CI belongs to a different repository')


def check_source_ci(reader, repository, source_sha):
    validate_inputs(repository, source_sha)
    prefix = '/repos/' + repository + '/actions'
    workflow = reader.get(prefix + '/workflows/ci.yml')
    require(isinstance(workflow, dict) and positive_id(workflow.get('id'))
            and workflow.get('name') == WORKFLOW_NAME and workflow.get('path') == WORKFLOW_PATH
            and workflow.get('state') == 'active', 'expected active CI workflow was not found')
    workflow_id = workflow['id']
    query = urllib.parse.urlencode({'head_sha': source_sha, 'branch': 'main', 'event': 'push',
                                    'per_page': MAX_RUNS, 'page': 1})
    page = reader.get(prefix + '/workflows/' + str(workflow_id) + '/runs?' + query)
    require(isinstance(page, dict), 'invalid source CI runs response')
    runs, total = page.get('workflow_runs'), page.get('total_count')
    require(isinstance(runs, list) and type(total) is int and 0 <= total <= MAX_RUNS
            and len(runs) == total, 'source CI run results exceed the bounded complete page')
    require(bool(runs), 'no CI main push run exists for the exact source SHA')
    for run in runs:
        validate_run(run, repository, source_sha, workflow_id)
    require(len({run['id'] for run in runs}) == len(runs), 'duplicate source CI run IDs')
    latest = max(runs, key=lambda run: run['id'])
    # Re-read the selected run once; a newer attempt in flight invalidates this
    # snapshot instead of falling back to an older successful attempt or run.
    run = reader.get(prefix + '/runs/' + str(latest['id']))
    validate_run(run, repository, source_sha, workflow_id)
    require(run['id'] == latest['id'] and run['run_attempt'] == latest['run_attempt'],
            'source CI run changed during verification')
    require(run.get('status') == 'completed' and run.get('conclusion') == 'success',
            'exact source CI has not completed successfully')
    return {
        'schema_version': 1, 'kind': 'analytical-source-ci', 'source_sha': source_sha,
        'repository': repository, 'head_repository': repository, 'repository_id': run['repository']['id'],
        'run_id': run['id'], 'run_attempt': run['run_attempt'], 'workflow_id': workflow_id,
        'workflow_name': WORKFLOW_NAME, 'workflow_path': WORKFLOW_PATH,
        'head_branch': 'main', 'event': 'push', 'status': 'completed', 'conclusion': 'success',
        'release_authorized': False,
    }


def write_receipt(output, receipt):
    payload = json.dumps(receipt, sort_keys=True, separators=(',', ':'), ensure_ascii=True) + '\n'
    require(len(payload.encode('ascii')) <= MAX_RECEIPT_BYTES, 'source CI receipt exceeds its bound')
    # Exclusive creation prevents stale output reuse and symlink replacement.
    # The caller must use a fresh output path and stop when this tool fails.
    with output.open('x', encoding='ascii', newline='\n') as stream:
        stream.write(payload)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', required=True, help='Exact GitHub OWNER/REPOSITORY')
    parser.add_argument('--source-sha', required=True, help='Exact source commit, 40 lowercase hex digits')
    parser.add_argument('--output', required=True, type=Path, help='New source-ci.json receipt path')
    args = parser.parse_args(argv)
    try:
        validate_inputs(args.repository, args.source_sha)
        require(not args.output.exists() and not args.output.is_symlink(), 'source CI output already exists')
        reader = GitHubReader(os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN', ''))
        receipt = check_source_ci(reader, args.repository, args.source_sha)
        write_receipt(args.output, receipt)
    except GateError as exc:
        print('source CI gate: ' + str(exc), file=sys.stderr)
        return 1
    except OSError:
        print('source CI gate: could not create new receipt', file=sys.stderr)
        return 1
    print('Exact source CI verified; release_authorized=false')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
