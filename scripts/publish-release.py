#!/usr/bin/env python3
"""Stage, verify and publish an existing tag. Never creates or moves Git tags.

The draft is also the early write-permission check, before native builds start.
Recovery accepts only artifacts from the same tag commit with successful gates.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
from urllib.error import HTTPError
from urllib.parse import quote, urlencode, urlsplit
from urllib.request import Request, urlopen


TARGETS = (
    'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu',
    'aarch64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin',
)
INSTALLERS = ('install.ps1', 'install.sh', 'install-release.ps1', 'install-release.sh')
BUILD_JOBS = {'windows-x86_64', 'linux-x86_64', 'linux-aarch64',
              'macos-x86_64', 'macos-aarch64', 'Public model download sources',
              'Release interface checks / windows'}


class GitHub:
    def __init__(self, repo, token, api=None):
        if not re.fullmatch(r'[\w.-]+/[\w.-]+', repo):
            raise RuntimeError('Invalid repository')
        self.api = (api or os.getenv('GITHUB_API_URL', 'https://api.github.com')).rstrip('/')
        self.base = self.api + '/repos/' + repo
        self.token = token

    def request(self, method, path, value=None, *, file=None, missing=False):
        url = path if path.startswith(('https://', 'http://')) else self.base + path
        # API-provided asset URLs are authenticated too: never send credentials
        # to arbitrary hosts. Local HTTP is used only by executable contract tests.
        allowed = {urlsplit(self.api).netloc, 'uploads.github.com'}
        if urlsplit(url).netloc not in allowed:
            raise RuntimeError('Untrusted release upload URL')
        headers = {'Authorization': 'Bearer ' + self.token,
                   'Accept': 'application/vnd.github+json',
                   'X-GitHub-Api-Version': '2022-11-28',
                   'User-Agent': 'localvox-release'}
        if file is not None:
            headers.update({'Content-Type': 'application/octet-stream',
                            'Content-Length': str(file.stat().st_size)})
            data = file.open('rb')
        else:
            headers['Content-Type'] = 'application/json'
            data = None if value is None else json.dumps(value).encode()
        try:
            with urlopen(Request(url, data=data, method=method, headers=headers), timeout=120) as response:
                body = response.read()
                return json.loads(body) if body else None
        except HTTPError as error:
            try:
                if missing and error.code == 404:
                    return None
                try:
                    message = json.loads(error.read()).get('message', 'request rejected')
                except (ValueError, AttributeError):
                    message = 'request rejected'
                raise RuntimeError(f'GitHub {method} {urlsplit(url).path}: HTTP {error.code}: {message}') from None
            finally:
                error.close()
        finally:
            if file is not None:
                data.close()

    def pages(self, path, key=None):
        result = []
        for page in range(1, 101):
            values = self.request('GET', path + ('&' if '?' in path else '?') +
                                  urlencode({'per_page': 100, 'page': page}))
            values = values[key] if key else values
            result.extend(values)
            if len(values) < 100:
                return result
        raise RuntimeError('GitHub pagination exceeded its safety bound')


def tag_commit(api, tag):
    if not re.fullmatch(r'v\d[\w.-]*', tag):
        raise RuntimeError('Invalid release tag')
    obj = api.request('GET', '/git/ref/tags/' + quote(tag, safe=''), missing=True)
    if obj is None:
        raise RuntimeError('Release tag must already exist on GitHub')
    obj = obj['object']
    for _ in range(10):
        if obj['type'] == 'commit':
            if not re.fullmatch(r'[0-9a-f]{40}', obj['sha']):
                raise RuntimeError('Invalid tag commit')
            return obj['sha']
        if obj['type'] != 'tag':
            break
        obj = api.request('GET', '/git/tags/' + obj['sha'])['object']
    raise RuntimeError('Release tag does not resolve to a commit')


def verify_source_run(api, run_id, sha):
    if not str(run_id).isdigit():
        raise RuntimeError('Invalid source run ID')
    run = api.request('GET', '/actions/runs/' + str(run_id))
    if (run['head_sha'] != sha or run['status'] != 'completed' or
            run['path'] != '.github/workflows/build-binaries.yml'):
        raise RuntimeError('Source run must be a completed binary build of the release commit')
    jobs = api.pages('/actions/runs/' + str(run_id) + '/jobs', 'jobs')
    passed = {j['name'] for j in jobs if j['conclusion'] == 'success'}
    if not BUILD_JOBS <= passed:
        raise RuntimeError('Source run is missing successful build/interface gates: ' +
                           ', '.join(sorted(BUILD_JOBS - passed)))
    failed = [j['name'] for j in jobs if j['name'] != 'Publish GitHub Release' and
              j['conclusion'] not in ('success', 'skipped')]
    if failed:
        raise RuntimeError('Source run has failed gates: ' + ', '.join(failed))
    artifacts = api.pages('/actions/runs/' + str(run_id) + '/artifacts', 'artifacts')
    ready = {a['name'] for a in artifacts if not a['expired'] and a['size_in_bytes'] > 0}
    if not {'localvox-' + t for t in TARGETS} <= ready:
        raise RuntimeError('Source run is missing unexpired native artifacts')


def draft(api, tag, sha):
    # GET /releases/tags/:tag is documented for published releases; the
    # authenticated list also includes drafts, which must survive retries.
    matches = [r for r in api.pages('/releases') if r['tag_name'] == tag]
    if len(matches) > 1:
        raise RuntimeError('Multiple drafts/releases use the requested tag')
    release = matches[0] if matches else None
    marker = '<!-- localvox-release-sha:' + sha + ' -->'
    if release is None:
        # The tag was verified above. Omitting target_commitish avoids asking
        # GITHUB_TOKEN to recreate an older workflow-changing commit's tag.
        release = api.request('POST', '/releases', {
            'tag_name': tag, 'name': tag, 'draft': True,
            'generate_release_notes': True, 'body': marker,
        })
    else:
        if not release['draft']:
            raise RuntimeError('Release is already published; public assets will not be overwritten')
        if marker not in (release.get('body') or ''):
            raise RuntimeError('Existing draft does not belong to the verified release commit')
        # A previous draft alone is not evidence of this job token's write access.
        release = api.request('PATCH', '/releases/' + str(release['id']),
                              {'body': release['body']})
    if not release['draft'] or release['tag_name'] != tag or marker not in (release.get('body') or ''):
        raise RuntimeError('GitHub returned an unexpected release draft')
    return release


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def checked_assets(folder):
    names = set(INSTALLERS)
    for target in TARGETS:
        archive = 'localvox-' + target + ('.zip' if 'windows' in target else '.tar.gz')
        names.update((archive, archive + '.sha256'))
        path = folder / archive
        checksum = folder / (archive + '.sha256')
        if not path.is_file() or not checksum.is_file():
            raise RuntimeError('Missing archive/checksum: ' + archive)
        parts = checksum.read_text(encoding='utf-8').strip().split()
        if len(parts) != 2 or parts[1].lstrip('*') != archive or parts[0] != digest(path):
            raise RuntimeError('Archive checksum mismatch: ' + archive)
    if {p.name for p in folder.iterdir()} != names:
        raise RuntimeError('Release must contain exactly five archives, checksums and four installers')
    paths = sorted(folder.iterdir())
    if any(not p.is_file() or p.stat().st_size == 0 for p in paths):
        raise RuntimeError('Empty release asset')
    return paths


def publish(api, release, paths):
    endpoint = '/releases/' + str(release['id'])
    existing = api.pages(endpoint + '/assets')
    by_name = {a['name']: a for a in existing}
    if len(by_name) != len(existing) or set(by_name) - {p.name for p in paths}:
        raise RuntimeError('Unexpected assets in release draft')
    expected = {}
    for path in paths:
        expected[path.name] = ('sha256:' + digest(path), path.stat().st_size)
        asset = by_name.get(path.name)
        if asset and asset['state'] != 'uploaded':
            api.request('DELETE', '/releases/assets/' + str(asset['id']))
            asset = None
        if asset:
            if (asset.get('digest'), asset['size']) != expected[path.name]:
                raise RuntimeError('Existing draft asset differs: ' + path.name)
        else:
            api.request('POST', release['upload_url'].split('{')[0] + '?' +
                        urlencode({'name': path.name}), file=path)
    actual = api.pages(endpoint + '/assets')
    if len(actual) != len(expected) or any(
            a['state'] != 'uploaded' or (a.get('digest'), a['size']) != expected.get(a['name'])
            for a in actual):
        raise RuntimeError('Uploaded release assets failed remote checksum/size verification')
    result = api.request('PATCH', endpoint, {'draft': False, 'make_latest': 'true'})
    if result['draft']:
        raise RuntimeError('GitHub did not publish the release')
    return result['html_url']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('preflight', 'publish'))
    parser.add_argument('--repo', default=os.getenv('GITHUB_REPOSITORY'))
    parser.add_argument('--tag', required=True)
    parser.add_argument('--sha')
    parser.add_argument('--source-run')
    parser.add_argument('--assets', type=Path)
    args = parser.parse_args()
    token = os.getenv('GH_TOKEN')
    if not token:
        raise RuntimeError('GH_TOKEN is required')
    api = GitHub(args.repo or '', token)
    sha = tag_commit(api, args.tag)
    if args.sha and args.sha != sha:
        raise RuntimeError('Release tag does not match the built commit')
    if args.source_run:
        verify_source_run(api, args.source_run, sha)
    elif not args.sha:
        raise RuntimeError('Provide the built commit or a verified source run')
    paths = checked_assets(args.assets) if args.action == 'publish' and args.assets else None
    if args.action == 'publish' and paths is None:
        raise RuntimeError('--assets is required for publication')
    release = draft(api, args.tag, sha)
    if args.action == 'publish':
        print(publish(api, release, paths))
    else:
        print(f'Release write preflight passed: {args.tag} @ {sha}')
        if os.getenv('GITHUB_OUTPUT'):
            with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
                stream.write(f'tag={args.tag}\nsha={sha}\n')


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print('Release delivery failed: ' + str(error), file=sys.stderr)
        sys.exit(1)
