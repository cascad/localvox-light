"""Executable release protocol regressions. Local HTTP only; no native builds."""
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('publish_release', ROOT / 'scripts/publish-release.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
SHA = '2' * 40
TAG = 'v0.2.11'


class PublicationContracts(unittest.TestCase):
    def setUp(self):
        (ROOT / 'target').mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / 'target', prefix='publication-http-')
        self.addCleanup(self.temp.cleanup)
        self.folder = Path(self.temp.name)
        self.calls = []
        self.current = None
        self.assets = []
        self.deny = None
        self.bad_upload = False
        self.tag = {'object': {'type': 'commit', 'sha': SHA}}
        self.run = {'head_sha': SHA, 'path': '.github/workflows/build-binaries.yml',
                    'status': 'completed'}
        self.jobs = [{'name': name, 'conclusion': 'success'} for name in release.BUILD_JOBS]
        self.jobs.append({'name': 'Publish GitHub Release', 'conclusion': 'failure'})
        self.artifacts = [{'name': 'localvox-' + t, 'size_in_bytes': 10, 'expired': False}
                          for t in release.TARGETS]
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self): self.handle_request()
            def do_POST(self): self.handle_request()
            def do_PATCH(self): self.handle_request()
            def do_DELETE(self): self.handle_request()

            def handle_request(self):
                path = urlsplit(self.path).path.removeprefix('/repos/example/localvox')
                body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
                value = json.loads(body) if body and path != '/upload' else body
                owner.calls.append((self.command, path, value))
                status, result = 200, None
                if owner.deny == (self.command, path):
                    status, result = 403, {'message': 'Resource not accessible by integration'}
                elif path.startswith('/git/ref/tags/'):
                    status, result = (200, owner.tag) if owner.tag else (404, {})
                elif path.startswith('/git/tags/'):
                    result = {'object': {'type': 'commit', 'sha': SHA}}
                elif path == '/actions/runs/123': result = owner.run
                elif path == '/actions/runs/123/jobs': result = {'jobs': owner.jobs}
                elif path == '/actions/runs/123/artifacts': result = {'artifacts': owner.artifacts}
                elif path == '/releases/tags/' + TAG:
                    status, result = ((200, owner.current) if owner.current and not owner.current['draft']
                                      else (404, {}))
                elif path == '/releases' and self.command == 'GET':
                    result = [owner.current] if owner.current else []
                elif path == '/releases' and self.command == 'POST':
                    owner.current = dict(value, id=1, html_url='https://github.com/example/localvox/releases/tag/' + TAG,
                                         upload_url=owner.url + '/repos/example/localvox/upload{?name,label}')
                    status, result = 201, owner.current
                elif path == '/releases/1' and self.command == 'PATCH':
                    owner.current.update(value)
                    result = owner.current
                elif path == '/releases/1/assets': result = owner.assets
                elif path == '/upload':
                    name = parse_qs(urlsplit(self.path).query)['name'][0]
                    asset = {'id': len(owner.assets) + 1, 'name': name, 'size': len(body),
                             'digest': 'sha256:' + hashlib.sha256(body).hexdigest(), 'state': 'uploaded'}
                    if owner.bad_upload: asset['digest'] = 'sha256:wrong'
                    owner.assets.append(asset)
                    status, result = 201, asset
                elif path.startswith('/releases/assets/') and self.command == 'DELETE':
                    owner.assets[:] = [a for a in owner.assets if a['id'] != int(path.rsplit('/', 1)[1])]
                    status = 204
                else:
                    status, result = 404, {'message': 'Unexpected request'}
                data = b'' if result is None else json.dumps(result).encode()
                self.send_response(status)
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)

        self.server = HTTPServer(('127.0.0.1', 0), Handler)
        self.url = 'http://127.0.0.1:' + str(self.server.server_port)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.close_server)
        self.api = release.GitHub('example/localvox', 'fixture-token', self.url)

    def close_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)

    def files(self):
        for name in release.INSTALLERS:
            (self.folder / name).write_bytes(b'installer fixture')
        for target in release.TARGETS:
            name = 'localvox-' + target + ('.zip' if 'windows' in target else '.tar.gz')
            file = self.folder / name
            file.write_bytes(('archive fixture ' + target).encode())
            (self.folder / (name + '.sha256')).write_text(release.digest(file) + '  ' + name + '\n')
        return release.checked_assets(self.folder)

    def test_actual_create_403_fails_preflight_with_no_upload_or_public_release(self):
        self.deny = ('POST', '/releases')
        self.assertEqual(release.tag_commit(self.api, TAG), SHA)
        with self.assertRaisesRegex(RuntimeError, 'HTTP 403: Resource not accessible'):
            release.draft(self.api, TAG, SHA)
        self.assertIsNone(self.current)
        self.assertEqual(self.assets, [])
        self.assertNotIn('target_commitish', self.calls[-1][2])

    def test_real_cli_exits_nonzero_on_403_and_on_moved_tag(self):
        environment = dict(os.environ, GH_TOKEN='fixture-token', GITHUB_API_URL=self.url,
                           GITHUB_REPOSITORY='example/localvox')
        for sha, denied in ((SHA, ('POST', '/releases')), ('f' * 40, None)):
            self.deny = denied
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/publish-release.py'),
                                     'preflight', '--tag', TAG, '--sha', sha],
                                    env=environment, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Release delivery failed:', result.stderr)
            self.assertNotIn('fixture-token', result.stderr)
            self.assertIsNone(self.current)

    def test_missing_remote_tag_never_creates_a_tag_or_release(self):
        self.tag = None
        with self.assertRaisesRegex(RuntimeError, 'must already exist'):
            release.tag_commit(self.api, TAG)
        self.assertTrue(all(method == 'GET' for method, _, _ in self.calls))

    def test_annotated_tag_is_peeled_to_the_built_commit(self):
        self.tag = {'object': {'type': 'tag', 'sha': 'a' * 40}}
        self.assertEqual(release.tag_commit(self.api, TAG), SHA)

    def test_cli_rejects_a_tag_moved_away_from_the_built_commit_before_any_write(self):
        argv = ['publish-release.py', 'preflight', '--tag', TAG, '--sha', 'f' * 40]
        with patch.object(release, 'GitHub', return_value=self.api), \
                patch.dict(release.os.environ, {'GH_TOKEN': 'fixture-token'}), \
                patch.object(release.sys, 'argv', argv):
            with self.assertRaisesRegex(RuntimeError, 'does not match the built commit'):
                release.main()
        self.assertTrue(all(method == 'GET' for method, _, _ in self.calls))

    def test_draft_creation_is_hidden_and_reuse_performs_a_real_write(self):
        first = release.draft(self.api, TAG, SHA)
        self.assertTrue(first['draft'])
        self.assertNotIn('target_commitish', self.calls[-1][2])
        second = release.draft(self.api, TAG, SHA)
        self.assertEqual(first['id'], second['id'])
        self.assertEqual(self.calls[-1][:2], ('PATCH', '/releases/1'))
        self.deny = ('PATCH', '/releases/1')
        with self.assertRaisesRegex(RuntimeError, 'HTTP 403'):
            release.draft(self.api, TAG, SHA)

    def test_public_release_and_unowned_draft_are_not_overwritten(self):
        release.draft(self.api, TAG, SHA)
        for change, message in (({'draft': False}, 'already published'),
                                ({'draft': True, 'body': 'unrelated draft'}, 'does not belong')):
            self.current.update(change)
            self.calls.clear()
            with self.assertRaisesRegex(RuntimeError, message):
                release.draft(self.api, TAG, SHA)
            self.assertEqual(len(self.calls), 1)

    def test_recovery_accepts_completed_matching_build_even_when_only_publisher_failed(self):
        release.verify_source_run(self.api, '123', SHA)
        self.assertEqual(len(self.calls), 3)

    def test_recovery_rejects_wrong_commit_incomplete_run_missing_or_failed_gate(self):
        for field, value in (('head_sha', 'a' * 40), ('status', 'in_progress'), ('path', 'other.yml')):
            previous = self.run[field]
            self.run[field] = value
            with self.assertRaisesRegex(RuntimeError, 'completed binary build'):
                release.verify_source_run(self.api, '123', SHA)
            self.run[field] = previous
        self.jobs[0]['conclusion'] = 'failure'
        with self.assertRaisesRegex(RuntimeError, 'missing successful'):
            release.verify_source_run(self.api, '123', SHA)
        self.jobs[0]['conclusion'] = 'success'
        self.jobs.append({'name': 'Additional required gate', 'conclusion': 'failure'})
        with self.assertRaisesRegex(RuntimeError, 'failed gates'):
            release.verify_source_run(self.api, '123', SHA)

    def test_recovery_rejects_expired_or_missing_artifacts(self):
        self.artifacts[0]['expired'] = True
        with self.assertRaisesRegex(RuntimeError, 'unexpired native artifacts'):
            release.verify_source_run(self.api, '123', SHA)
        self.artifacts.pop(0)
        with self.assertRaisesRegex(RuntimeError, 'unexpired native artifacts'):
            release.verify_source_run(self.api, '123', SHA)

    def test_complete_verified_assets_are_uploaded_before_draft_becomes_public(self):
        paths = self.files()
        staged = release.draft(self.api, TAG, SHA)
        url = release.publish(self.api, staged, paths)
        self.assertEqual(len(self.assets), 14)
        self.assertTrue(url.endswith(TAG))
        self.assertFalse(self.current['draft'])
        self.assertEqual(self.calls[-1], ('PATCH', '/releases/1', {'draft': False, 'make_latest': 'true'}))

    def test_partial_upload_is_reused_and_starter_asset_is_replaced(self):
        paths = self.files()
        staged = release.draft(self.api, TAG, SHA)
        self.assets = [dict(id=71, name=paths[0].name, size=paths[0].stat().st_size,
                            digest='sha256:' + release.digest(paths[0]), state='uploaded'),
                       dict(id=72, name=paths[1].name, size=0, digest=None, state='starter')]
        release.publish(self.api, staged, paths)
        uploads = [call for call in self.calls if call[1] == '/upload']
        self.assertEqual(len(uploads), 13)
        self.assertIn(('DELETE', '/releases/assets/72', b''), self.calls)

    def test_bad_remote_digest_and_upload_403_leave_the_release_hidden(self):
        paths = self.files()
        staged = release.draft(self.api, TAG, SHA)
        self.deny = ('POST', '/upload')
        with self.assertRaisesRegex(RuntimeError, 'HTTP 403'):
            release.publish(self.api, staged, paths)
        self.assertTrue(self.current['draft'])
        self.deny, self.bad_upload = None, True
        with self.assertRaisesRegex(RuntimeError, 'remote checksum'):
            release.publish(self.api, staged, paths)
        self.assertTrue(self.current['draft'])

    def test_foreign_or_changed_draft_assets_are_not_overwritten(self):
        paths = self.files()
        staged = release.draft(self.api, TAG, SHA)
        self.assets = [dict(id=1, name='foreign.zip', size=1, digest='sha256:wrong', state='uploaded')]
        with self.assertRaisesRegex(RuntimeError, 'Unexpected assets'):
            release.publish(self.api, staged, paths)
        self.assets[0]['name'] = paths[0].name
        with self.assertRaisesRegex(RuntimeError, 'Existing draft asset differs'):
            release.publish(self.api, staged, paths)
        self.assertTrue(self.current['draft'])

    def test_missing_archive_hash_mismatch_and_foreign_asset_fail_before_upload(self):
        paths = self.files()
        archive = next(p for p in paths if p.name.endswith('.zip'))
        archive.write_bytes(b'corrupt')
        with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
            release.checked_assets(self.folder)
        archive.unlink()
        with self.assertRaisesRegex(RuntimeError, 'Missing archive'):
            release.checked_assets(self.folder)
        paths = self.files()
        (self.folder / 'trace.zip').write_bytes(b'not a product')
        with self.assertRaisesRegex(RuntimeError, 'exactly five archives'):
            release.checked_assets(self.folder)
        self.assertEqual(self.calls, [])

    def test_upload_url_cannot_leak_the_token_to_another_host(self):
        with self.assertRaisesRegex(RuntimeError, 'Untrusted'):
            self.api.request('POST', 'https://untrusted.example/upload', {})
        self.assertEqual(self.calls, [])


class WorkflowOrdering(unittest.TestCase):
    def test_write_preflight_precedes_native_builds_and_interface_and_recovery_is_pinned(self):
        source = (ROOT / '.github/workflows/build-binaries.yml').read_text(encoding='utf-8')
        jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)',
                               source.split('\njobs:\n', 1)[1], re.M | re.S))
        for name in ('model-sources', 'build', 'interface', 'release'):
            needs = re.search(r'^    needs: (.+)$', jobs[name], re.M)
            self.assertIsNotNone(needs, name)
            self.assertIn('release-preflight', needs[1], name)
        gate = jobs['release-preflight']
        self.assertIn('contents: write', gate)
        self.assertIn('publish-release.py preflight', gate)
        self.assertIn('test_publication_delivery.py', gate)
        publisher = jobs['release']
        self.assertIn('run-id: ${{ inputs.artifact_run_id || github.run_id }}', publisher)
        self.assertIn('git show "$RELEASE_SHA:scripts/$script"', publisher)
        self.assertIn('publish-release.py publish', publisher)
        self.assertNotIn('--target', publisher)


if __name__ == '__main__': unittest.main()
