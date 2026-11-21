"""Offline coverage of the live source gate, including HEAD/GET disagreement."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import io
import json
import os
from pathlib import Path
import threading
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('model_sources', ROOT / 'scripts/check-model-sources.py')
sources = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sources)


class ModelSources(unittest.TestCase):
    def test_github_digest_drift_fails_before_the_model_download(self):
        url = 'https://github.com/owner/models/releases/download/speakers/embedding.onnx'
        metadata = {'assets': [{'name': 'embedding.onnx', 'digest': 'sha256:' + 'b' * 64}]}
        with patch.object(sources.urllib.request, 'urlopen', return_value=io.BytesIO(json.dumps(metadata).encode())) as get:
            ok, message = sources.check_item('diarize', {'url': url, 'kind': 'file', 'sha256': 'a' * 64})
        self.assertFalse(ok)
        self.assertIn('SHA-256 differs', message)
        self.assertEqual(get.call_count, 1, 'a drift must fail before HEAD/GET or building the release')

    def test_github_metadata_token_never_reaches_model_head_or_range(self):
        url = 'https://github.com/owner/models/releases/download/speakers/embedding.onnx'
        calls = []
        class Response(io.BytesIO):
            headers = {'Content-Length': '64'}
        def get(request, **_):
            calls.append(request)
            if request.full_url.startswith('https://api.github.com/'):
                return Response(json.dumps({'assets': [{'name': 'embedding.onnx', 'digest': 'sha256:' + 'a' * 64}]}).encode())
            return Response(b'x' * 64)
        with patch.dict(os.environ, {'GH_TOKEN': 'fixture-token'}), patch.object(sources.urllib.request, 'urlopen', side_effect=get):
            ok, message = sources.check_item('diarize', {'url': url, 'kind': 'file', 'sha256': 'a' * 64})
        self.assertTrue(ok, message)
        self.assertEqual(calls[0].full_url, 'https://api.github.com/repos/owner/models/releases/tags/speakers')
        self.assertEqual(calls[0].get_header('Authorization'), 'Bearer fixture-token')
        self.assertEqual([r.get_header('Authorization') for r in calls[1:]], [None, None])
        self.assertEqual(calls[-1].get_header('Range'), 'bytes=0-63')

    def test_github_missing_digest_is_not_an_integrity_pass(self):
        url = 'https://github.com/owner/models/releases/download/speakers/embedding.onnx'
        for assets in ([], [{'name': 'embedding.onnx', 'digest': None}], [{'name': 'other.onnx', 'digest': 'sha256:' + 'a' * 64}]):
            with self.subTest(assets=assets), patch.object(sources.urllib.request, 'urlopen', return_value=io.BytesIO(json.dumps({'assets': assets}).encode())):
                ok, _ = sources.check_item('diarize', {'url': url, 'kind': 'file', 'sha256': 'a' * 64})
                self.assertFalse(ok)

    def setUp(self):
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, 'w') as zip_file:
            zip_file.writestr('model/am/final.mdl', b'fixture')
        self.payload = payload = archive.getvalue()
        self.requests = requests = []

        class Handler(BaseHTTPRequestHandler):
            def respond(self):
                requests.append((self.command, self.path, self.headers.get('Range')))
                if self.path == '/redirect':
                    self.send_response(302)
                    self.send_header('Location', '/model')
                    self.end_headers()
                    return
                if self.path == '/denied' or (self.path == '/get-denied' and self.command == 'GET'):
                    self.send_error(401)
                    return
                body = b'<html>sign in</html>' if self.path == '/html' else payload
                self.send_response(200)  # Deliberately ignore Range; client must bound the read.
                self.send_header('Content-Length', str(len(body)))
                self.send_header('Content-Type', 'text/html' if self.path == '/html' else 'application/zip')
                self.end_headers()
                if self.command == 'GET':
                    self.wfile.write(body)

            do_HEAD = respond
            do_GET = respond

            def log_message(self, *_):
                pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop_server)

    def stop_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    def check(self, path, **properties):
        item = {'kind': 'zip', 'url': f'http://127.0.0.1:{self.server.server_port}{path}', **properties}
        return sources.check_item('fixture', item, timeout=2)

    def test_public_archive_redirect_and_range(self):
        ok, message = self.check('/redirect', size=len(self.payload))
        self.assertTrue(ok, message)
        self.assertEqual(self.requests, [('HEAD', '/redirect', None), ('HEAD', '/model', None),
                                        ('GET', '/redirect', 'bytes=0-63'), ('GET', '/model', 'bytes=0-63')])

    def test_auth_failure_in_head_is_rejected_without_get(self):
        ok, message = self.check('/denied')
        self.assertFalse(ok)
        self.assertIn('HTTP 401', message)
        self.assertEqual(len(self.requests), 1)

    def test_successful_head_does_not_hide_download_auth_failure(self):
        ok, message = self.check('/get-denied')
        self.assertFalse(ok)
        self.assertIn('HTTP 401', message)

    def test_changed_size_is_rejected_before_get(self):
        ok, message = self.check('/model', size=len(self.payload) + 1)
        self.assertFalse(ok)
        self.assertIn('HEAD reports', message)
        self.assertEqual(len(self.requests), 1)

    def test_login_page_is_never_a_successful_model_source(self):
        for kind in ('zip', 'file'):
            with self.subTest(kind=kind):
                ok, message = self.check('/html', kind=kind)
                self.assertFalse(ok, message)


if __name__ == '__main__':
    unittest.main()
