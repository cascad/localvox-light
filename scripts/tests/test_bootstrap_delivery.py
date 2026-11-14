"""Bootstrap stand: localhost releases, tiny payloads, no models or OS installers.

Run on each native OS: python -m unittest discover -s scripts/tests -p test_bootstrap_delivery.py -v
The real wizard is covered separately by the native ConPTY/PTY integration test.
"""
import functools
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import threading
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PS = shutil.which('powershell') if os.name == 'nt' else None
BASH = os.getenv('LOCALVOX_TEST_BASH') or (r'C:\Program Files\Git\bin\bash.exe' if os.name == 'nt' else shutil.which('bash'))


class BootstrapDelivery(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-bootstrap-fixture-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / 'isolated home'
        self.install = self.root / 'installed with spaces'
        self.scratch = self.root / 'temp'
        self.bin = self.root / 'bin'
        self.web = self.root / 'web'
        for directory in (self.home, self.scratch, self.bin, self.web):
            directory.mkdir()
        self.log = self.root / 'launch.log'
        self.environment = {k: v for k, v in os.environ.items() if not k.startswith('LOCALVOX_')}
        self.environment.update(HOME=str(self.home), USERPROFILE=str(self.home), LOCALAPPDATA=str(self.home),
                                TEMP=str(self.scratch), TMP=str(self.scratch),
                                LOCALVOX_BOOTSTRAP_FIXTURE_LOG=str(self.log))
        self.requests = []
        requests = self.requests

        class Handler(SimpleHTTPRequestHandler):
            def do_HEAD(self):
                requests.append(self.path)
                if self.path == '/releases/latest':
                    self.send_response(302)
                    self.send_header('Location', '/releases/tag/v9.8.7')
                    self.end_headers()
                elif self.path == '/releases/tag/v9.8.7':
                    self.send_response(200)
                    self.end_headers()
                else:
                    super().do_HEAD()

            def do_GET(self):
                requests.append(self.path)
                super().do_GET()

            def log_message(self, *_):
                pass

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Handler, directory=str(self.web)))
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.stop)
        self.url = f'http://127.0.0.1:{self.server.server_port}'

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()

    @staticmethod
    def unix(path):
        value = Path(path).as_posix()
        return '/' + value[0].lower() + value[2:] if os.name == 'nt' else value

    def command(self, name, text):
        path = self.bin / name
        path.write_text('#!/usr/bin/env bash\n' + text + '\n', encoding='utf-8', newline='\n')
        path.chmod(0o755)

    def archive(self, target, *, corrupt=False, unsafe=False):
        folder = self.web / 'releases/download/v9.8.7'
        folder.mkdir(parents=True, exist_ok=True)
        windows = 'windows' in target
        name = f'localvox-{target}' + ('.zip' if windows else '.tar.gz')
        archive = folder / name
        files = {'bundle.json': b'{}'}
        if windows:
            # .NET Framework stub tests delivery only; the Rust test drives the real wizard.
            source = self.root / 'Fixture.cs'
            source.write_text('''using System; using System.IO;
class Fixture {
 static int Main(string[] args) {
  string dest=""; bool check=false;
  foreach(string a in args) if(a=="--bundle-check") check=true;
  for(int i=0;i+1<args.Length;i++) if(args[i]=="--install-dir") dest=args[i+1];
  if(check) return File.Exists(Path.Combine(dest,"installed.ok"))?0:2;
  foreach(string a in args) if(a=="--check") return 2;
  File.AppendAllText(Environment.GetEnvironmentVariable("LOCALVOX_BOOTSTRAP_FIXTURE_LOG"), String.Join("\\n",args)+"\\n");
  Directory.CreateDirectory(dest);
  string source=AppDomain.CurrentDomain.BaseDirectory;
  if(Path.GetFullPath(source).TrimEnd('\\\\')!=Path.GetFullPath(dest).TrimEnd('\\\\'))
   foreach(string file in Directory.GetFiles(source)) File.Copy(file,Path.Combine(dest,Path.GetFileName(file)),true);
  File.WriteAllText(Path.Combine(dest,"installed.ok"),"fixture"); return 0;
 }
}''', encoding='utf-8')
            binary = self.root / 'fixture.exe'
            compiler = self.root / 'compile.ps1'
            compiler.write_text("param($Source,$Output)\n$ErrorActionPreference='Stop'\nAdd-Type -Path $Source -OutputAssembly $Output -OutputType ConsoleApplication\n")
            result = subprocess.run([PS, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', compiler,
                                     '-Source', source, '-Output', binary], env=self.environment, capture_output=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            files['localvox-install.exe'] = binary.read_bytes()
            for dll in ('libvosk.dll', 'libgcc_s_seh-1.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll'):
                files[dll] = b'loader presence fixture, not loaded by managed stub'
            with zipfile.ZipFile(archive, 'w') as out:
                for member, data in files.items():
                    out.writestr(member, data)
                if unsafe:
                    out.writestr('../outside', b'must not escape')
        else:
            files['libvosk.dylib' if 'darwin' in target else 'libvosk.so'] = b'fixture'
            files['localvox-install'] = b'''#!/usr/bin/env bash
set -eu
DEST='' CHECK=0
for arg in "$@"; do [[ "$arg" != --bundle-check ]] || CHECK=1; done
ARGS=("$@")
while [[ $# -gt 0 ]]; do case "$1" in --install-dir) DEST="$2"; shift 2;; --check) exit 2;; *) shift;; esac; done
if [[ "$CHECK" = 1 ]]; then [[ -f "$DEST/installed.ok" ]]; exit; fi
printf '%s\\n' "${ARGS[@]}" >> "$LOCALVOX_BOOTSTRAP_FIXTURE_LOG"
mkdir -p "$DEST"
SOURCE="$(cd "$(dirname "$0")" && pwd)"
if [[ "$SOURCE" != "$DEST" ]]; then cp -R "$SOURCE/." "$DEST/"; fi
echo fixture > "$DEST/installed.ok"
'''
            with tarfile.open(archive, 'w:gz') as out:
                for member, data in files.items():
                    info = tarfile.TarInfo(member)
                    info.size = len(data)
                    info.mode = 0o755 if member == 'localvox-install' else 0o644
                    out.addfile(info, io.BytesIO(data))
                if unsafe:
                    link = tarfile.TarInfo('escape')
                    link.type, link.linkname = tarfile.SYMTYPE, '../outside'
                    out.addfile(link)
        digest = '0' * 64 if corrupt else hashlib.sha256(archive.read_bytes()).hexdigest()
        (folder / (name + '.sha256')).write_text(f'{digest}  {name}\n')
        return name

    def run_bootstrap(self, shell, *flags, platform=('Linux', 'x86_64'), tty=False):
        env = self.environment.copy()
        if shell == 'ps':
            command = [PS, '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ROOT / 'scripts/install.ps1',
                       '-InstallDir', self.install, '-ReleaseBase', self.url, '-NoPathUpdate', *flags]
        else:
            self.command('uname', f'if [[ "$1" = -s ]]; then echo {platform[0]}; else echo {platform[1]}; fi')
            self.command('jq', 'echo "bootstrap must not require jq" >&2; exit 97')
            env.update(HOME=self.unix(self.home), TMPDIR=self.unix(self.scratch),
                       LOCALVOX_BOOTSTRAP_FIXTURE_LOG=self.unix(self.log))
            command = [BASH, '-c', 'export PATH="$1:$PATH"; shift; exec bash "$@"', 'fixture',
                       self.unix(self.bin), self.unix(ROOT / 'scripts/install.sh'),
                       '--install-dir', self.unix(self.install), '--release-base', self.url, '--no-path-update', *flags]
        if tty:
            import pty
            # Supply a controlling terminal for the production </dev/tty handoff.
            # The stub never asks for input; genuine prompt keys are tested in Rust.
            output = bytearray()
            previous = os.environ.copy()
            try:
                os.environ.clear()
                os.environ.update(env)
                status = pty.spawn([str(a) for a in command], lambda fd: output.extend(data := os.read(fd, 8192)) or data)
            finally:
                os.environ.clear()
                os.environ.update(previous)
            result = subprocess.CompletedProcess(command, os.waitstatus_to_exitcode(status), bytes(output), b'')
        else:
            result = subprocess.run([str(a) for a in command], env=env, capture_output=True, timeout=45)
        self.assertFalse(list(self.scratch.glob('localvox-bootstrap*')), 'bootstrap cleans only its own staging directory')
        return result

    @unittest.skipUnless(PS, 'Windows PowerShell fixture')
    def test_windows_download_apply_reuse_and_readonly_check(self):
        absent = self.run_bootstrap('ps', '-Check')
        self.assertEqual(absent.returncode, 2)
        self.assertFalse(self.install.exists())
        self.assertEqual(self.requests, [])
        name = self.archive('x86_64-pc-windows-msvc')
        result = self.run_bootstrap('ps')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertTrue((self.install / 'installed.ok').is_file())
        self.assertIn('/releases/download/v9.8.7/' + name, self.requests)
        before = list(self.requests)
        result = self.run_bootstrap('ps')
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
        self.assertEqual(self.requests, before, 'intact bundle needs no network')

    @unittest.skipUnless(PS, 'Windows PowerShell fixture')
    def test_windows_checksum_and_traversal_fail_before_destination_creation(self):
        for corrupt, unsafe in ((True, False), (False, True)):
            for file in self.root.glob('fixture.exe'):
                file.unlink()
            self.archive('x86_64-pc-windows-msvc', corrupt=corrupt, unsafe=unsafe)
            result = self.run_bootstrap('ps')
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.install.exists())
            self.assertFalse(self.log.exists())
            self.assertFalse((self.root / 'outside').exists())

    @unittest.skipUnless(BASH and Path(BASH).is_file(), 'Bash fixture')
    def test_all_unix_target_routes_and_hash_failure_need_no_jq_or_terminal(self):
        for system, arch, target in [('Darwin', 'arm64', 'aarch64-apple-darwin'),
                                     ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
                                     ('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
                                     ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu')]:
            with self.subTest(target=target):
                name = self.archive(target, corrupt=True)
                result = self.run_bootstrap('bash', platform=(system, arch))
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('SHA-256 mismatch', result.stderr.decode(errors='replace'))
                self.assertIn('/releases/download/v9.8.7/' + name, self.requests)
                self.assertFalse(self.install.exists())

    @unittest.skipIf(os.name == 'nt' or not BASH, 'native Unix controlling terminal')
    def test_unix_download_apply_reuse_and_archive_links(self):
        for system, arch, target in [('Darwin', 'arm64', 'aarch64-apple-darwin'),
                                     ('Darwin', 'x86_64', 'x86_64-apple-darwin'),
                                     ('Linux', 'x86_64', 'x86_64-unknown-linux-gnu'),
                                     ('Linux', 'aarch64', 'aarch64-unknown-linux-gnu')]:
            with self.subTest(target=target):
                self.archive(target, unsafe=True)
                rejected = self.run_bootstrap('bash', platform=(system, arch), tty=True)
                self.assertNotEqual(rejected.returncode, 0)
                self.assertFalse(self.install.exists())
                self.archive(target)
                result = self.run_bootstrap('bash', platform=(system, arch), tty=True)
                self.assertEqual(result.returncode, 0, result.stdout.decode(errors='replace'))
                before = list(self.requests)
                result = self.run_bootstrap('bash', platform=(system, arch), tty=True)
                self.assertEqual(result.returncode, 0, result.stdout.decode(errors='replace'))
                self.assertEqual(self.requests, before)
                # Only the fixture owns and removes this explicitly resolved directory.
                self.assertTrue(self.install.resolve().is_relative_to(self.root.resolve()))
                shutil.rmtree(self.install)


if __name__ == '__main__':
    unittest.main()
