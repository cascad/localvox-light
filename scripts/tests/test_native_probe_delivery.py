"""Native Windows diagnostics must fail without opening modal system dialogs."""
import ctypes
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('release_probe', ROOT / 'scripts/check-release-install.py')
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


@unittest.skipUnless(os.name == 'nt', 'Windows native error dialogs')
class NativeProbeContracts(unittest.TestCase):
    def test_error_mode_is_scoped_even_when_the_probe_fails(self):
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        before = kernel.GetErrorMode()
        with self.assertRaisesRegex(RuntimeError, 'synthetic'):
            with probe.native_errors_to_exit_code():
                self.assertEqual(kernel.GetErrorMode() & 3, 3)
                raise RuntimeError('synthetic')
        self.assertEqual(kernel.GetErrorMode(), before)

    def test_missing_dll_reports_a_native_failure_without_a_loader_dialog(self):
        target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / 'target'))
        binary = target / 'x86_64-pc-windows-msvc/release/localvox-light.exe'
        if not binary.is_file():
            self.skipTest('built Windows daemon required; no models/devices are used')
        original_run = subprocess.run
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        before = kernel.GetErrorMode()

        def guarded_spawn(*args, **kwargs):
            self.assertEqual(kernel.GetErrorMode() & 3, 3, 'guard must be set before the loader runs')
            return original_run(*args, **kwargs)

        with tempfile.TemporaryDirectory(prefix='localvox-missing-dll-') as temporary:
            private = Path(temporary)
            copied = private / 'localvox-light.exe'
            shutil.copy2(binary, copied)
            environment = {key: value for key, value in os.environ.items() if not key.startswith('LOCALVOX_')}
            environment['PATH'] = str(Path(os.environ['SystemRoot']) / 'System32')
            with patch.object(probe.subprocess, 'run', side_effect=guarded_spawn):
                with self.assertRaisesRegex(RuntimeError, r'missing native DLL \(0xC0000135\)'):
                    probe.run([copied, '--help'], cwd=private, environment=environment)
        self.assertEqual(kernel.GetErrorMode(), before)


if __name__ == '__main__':
    unittest.main()
