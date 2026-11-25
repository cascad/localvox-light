"""CI's installed-product checks, without inference or shared application data."""
import importlib.util
import hashlib
import base64
import io
import json
import os
from pathlib import Path
import re
import shutil
import shlex
import subprocess
import sys
import tempfile
import tomllib
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('check_release_install', ROOT / 'scripts/check-release-install.py')
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class InstalledCheckContracts(unittest.TestCase):
    @unittest.skipUnless(os.name == 'nt' and shutil.which('node') and shutil.which('tar.exe'), 'Windows mpv setup contract')
    def test_mpv_setup_exports_the_actual_path_on_fresh_and_cached_runs(self):
        with tempfile.TemporaryDirectory(prefix='mpv-ci-export-') as temporary:
            root = Path(temporary) / 'project with spaces Ю'
            (root / 'scripts').mkdir(parents=True)
            (root / 'Cargo.toml').write_text('fixture', encoding='utf-8')
            script = root / 'scripts/setup-mpv.mjs'
            shutil.copy2(ROOT / 'scripts/setup-mpv.mjs', script)
            payload = b'fixture binary; not executed'
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, 'w') as archive:
                archive.writestr('mpv.exe', payload)
            data = stream.getvalue()
            bootstrap = '''
import { pathToFileURL } from 'node:url';
const bytes=Buffer.from(process.env.FIXTURE_ZIP,'base64');
globalThis.fetch=async (url)=>{
  if (process.env.CACHED==='1') throw Error('cached setup must not access the network');
  if (url==='https://api.github.com/repos/mpv-player/mpv/releases/tags/git-release')
    return new Response(JSON.stringify({assets:[{name:'mpv-fixture-x86_64-w64-mingw32-full.zip',digest:'sha256:'+process.env.FIXTURE_SHA,browser_download_url:'https://fixture.invalid/mpv.zip'}]}));
  if (url==='https://fixture.invalid/mpv.zip') return new Response(bytes);
  throw Error('unexpected network request');
};
await import(pathToFileURL(process.argv[1]));
'''
            env = {k: v for k, v in os.environ.items() if k not in ('GH_TOKEN', 'GITHUB_TOKEN')}
            exported = root / 'github-env'
            env.update(GITHUB_ENV=str(exported), FIXTURE_ZIP=base64.b64encode(data).decode(), FIXTURE_SHA=hashlib.sha256(data).hexdigest())
            for cached in (False, True):
                if exported.exists(): exported.unlink()
                env['CACHED'] = '1' if cached else '0'
                result = subprocess.run([shutil.which('node'), '--input-type=module', '-e', bootstrap, str(script)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                lines = exported.read_text(encoding='utf-8').splitlines()
                self.assertEqual(len(lines), 1)
                key, path = lines[0].split('=', 1)
                self.assertEqual(key, 'LOCALVOX_MPV')
                self.assertTrue(Path(path).is_absolute())
                self.assertEqual(Path(path).read_bytes(), payload)
                self.assertEqual(Path(path), root / 'target/tools/mpv/mpv.exe')

    def test_native_and_interface_workflows_install_the_shared_rustup_pin(self):
        source = '# общий компилятор · UTF-8\n' + (ROOT / 'rust-toolchain.toml').read_text(encoding='utf-8')
        toolchain = tomllib.loads(source)['toolchain']
        self.assertRegex(toolchain['channel'], r'^\d+\.\d+\.\d+$')
        self.assertEqual(toolchain['profile'], 'minimal')
        self.assertIn('rustfmt', toolchain['components'])
        for name in ('build-binaries.yml', 'interface-tests.yml'):
            with self.subTest(workflow=name):
                workflow = (ROOT / '.github/workflows' / name).read_text(encoding='utf-8')
                self.assertEqual(re.findall(r'uses: dtolnay/rust-toolchain@([^\s]+)', workflow), ['master'])
                self.assertIn('toolchain: ${{ steps.rust-version.outputs.toolchain }}', workflow)
                self.assertIn('id: rust-version', workflow)
                line = next(x.strip()[5:] for x in workflow.splitlines()
                            if x.strip().startswith('run: python -c ') and 'rust-toolchain.toml' in x)
                arguments = shlex.split(line)
                self.assertEqual(arguments[:2], ['python', '-c'])
                self.assertEqual(arguments[3:], ['>>', '$GITHUB_OUTPUT'])
                # Execute the workflow's actual parser, including a changed TOML
                # value: no duplicate compiler version may be baked into YAML.
                with tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    for version in (toolchain['channel'], '1.98.0'):
                        (root / 'rust-toolchain.toml').write_text(
                            source.replace(toolchain['channel'], version), encoding='utf-8')
                        result = subprocess.run([sys.executable, '-c', arguments[2]], cwd=root,
                                                capture_output=True, text=True, timeout=20)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(result.stdout.strip(), 'toolchain=' + version)
        build = (ROOT / '.github/workflows/build-binaries.yml').read_text(encoding='utf-8')
        self.assertIn('- "rust-toolchain.toml"', build)

    def test_structured_doctor_requires_unique_ids_and_detects_the_deliberately_missing_models(self):
        states = {'vosk': 'ok', 'gigaam': 'fail', 'worker': 'ok', 'archive': 'warn',
                  'ollama_cli': 'warn', 'ollama_api': 'warn', 'llm_model': 'warn'}
        report = {'schema': 1, 'findings': [{'id': k, 'state': v} for k, v in states.items()]}
        self.assertEqual(smoke.inspect_doctor(json.dumps(report)), states)
        for mode in ('schema', 'duplicate', 'missing', 'wrong_model_state'):
            changed = json.loads(json.dumps(report))
            if mode == 'schema': changed['schema'] = 9
            elif mode == 'duplicate': changed['findings'].append(changed['findings'][0])
            elif mode == 'missing': changed['findings'].pop()
            else: changed['findings'][1]['state'] = 'ok'
            with self.assertRaises(RuntimeError, msg=mode): smoke.inspect_doctor(json.dumps(changed))

    def test_readonly_snapshot_catches_empty_directories_and_rewrites_of_identical_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            file = root / '.env'
            file.write_text('# private fixture\nX=value\n')
            before = smoke.snapshot(root)
            os.utime(file, ns=(file.stat().st_atime_ns, file.stat().st_mtime_ns + 1_000_000_000))
            self.assertNotEqual(smoke.snapshot(root), before)
            before = smoke.snapshot(root)
            (root / 'accidental-empty-models').mkdir()
            self.assertNotEqual(smoke.snapshot(root), before)

    @unittest.skipUnless(os.name == 'nt' and shutil.which('pwsh'), 'PowerShell 7 native CI error policy')
    def test_workflow_native_failure_stops_before_a_later_success_can_hide_it(self):
        workflow = (ROOT / '.github/workflows/interface-tests.yml').read_text(encoding='utf-8')
        blocks = re.findall(r'^        run: \|\n((?:          [^\n]*\n|\n)+)', workflow, re.M)
        headers = set()
        for block in blocks:
            header = '\n'.join(line[10:] for line in block.splitlines()[:2])
            self.assertIn("$ErrorActionPreference = 'Stop'", header)
            self.assertIn('$PSNativeCommandUseErrorActionPreference = $true', header)
            headers.add(header)
        self.assertTrue(headers)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for i, header in enumerate(headers):
                script = root / f'native-failure-{i}.ps1'
                script.write_text(header + '\n& $env:ComSpec /c exit 7\n[IO.File]::WriteAllText("$PSScriptRoot/continued", "unexpected")\n', encoding='utf-8-sig')
                result = subprocess.run([shutil.which('pwsh'), '-NoProfile', '-File', str(script)], capture_output=True, timeout=20)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((root / 'continued').exists())

    @unittest.skipUnless(os.getenv('LOCALVOX_DELIVERY_ARCHIVE'), 'Explicit complete native archive')
    def test_actual_archive_doctor_setup_and_reconciliation(self):
        result = smoke.check(Path(os.environ['LOCALVOX_DELIVERY_ARCHIVE']))
        self.assertTrue(result['reconcile_unchanged'] and result['wizard_requires_tty_without_mutation'])


if __name__ == '__main__': unittest.main()
