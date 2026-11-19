"""Offline coverage of the live source gate, including HEAD/GET disagreement."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import io
from pathlib import Path
import threading
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('model_sources', ROOT / 'scripts/check-model-sources.py')
sources = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sources)


class ModelSources(unittest.TestCase):
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
