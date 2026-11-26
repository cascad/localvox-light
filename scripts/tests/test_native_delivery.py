"""Intel Mac SDK orchestration fixtures; never claim a native Mac source build."""
import importlib.util
import json
from pathlib import Path
import subprocess
import plistlib
import struct
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('native_ort', ROOT / 'scripts/build-onnxruntime-macos.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)

capture_spec = importlib.util.spec_from_file_location('capture_metadata', ROOT / 'scripts/check-macos-capture.py')
capture = importlib.util.module_from_spec(capture_spec)
capture_spec.loader.exec_module(capture)


class MacCaptureMetadata(unittest.TestCase):
    def test_desktop_and_daemon_explain_the_same_audio_permissions(self):
        daemon = plistlib.loads((ROOT / 'assets/macos/daemon-Info.plist').read_bytes())
        desktop = plistlib.loads((ROOT / 'src-tauri/Info.plist').read_bytes())
        for key in ('NSMicrophoneUsageDescription', 'NSAudioCaptureUsageDescription'):
            self.assertEqual(daemon[key], desktop[key])

    def test_actual_macho_section_requires_both_capture_descriptions(self):
        metadata = plistlib.loads((ROOT / 'assets/macos/daemon-Info.plist').read_bytes())
        metadata['CFBundleVersion'] = '0.2.13'
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / 'daemon'

            def write_macho(properties):
                payload = plistlib.dumps(properties)
                command_size = 72 + 80
                header = struct.pack('<IiiIIIII', 0xfeedfacf, 0x100000c, 0, 2, 1, command_size, 0, 0)
                segment = struct.pack('<II16sQQQQiiII', 0x19, command_size, b'__TEXT', 0, 0, 0, 0, 0, 0, 1, 0)
                section = struct.pack('<16s16sQQIIIIIIII', b'__info_plist', b'__TEXT', 0, len(payload), 32 + command_size, 0, 0, 0, 0, 0, 0, 0)
                binary.write_bytes(header + segment + section + payload)

            write_macho(metadata)
            self.assertEqual(capture.inspect(binary)['CFBundleIdentifier'], 'ru.localvox.daemon')
            for missing in ('NSAudioCaptureUsageDescription', 'NSMicrophoneUsageDescription'):
                broken = dict(metadata)
                del broken[missing]
                write_macho(broken)
                with self.assertRaisesRegex(ValueError, missing):
                    capture.inspect(binary)
            binary.write_bytes(struct.pack('<IiiIIIII', 0xfeedfacf, 0x100000c, 0, 2, 0, 0, 0, 0))
            with self.assertRaisesRegex(ValueError, '__info_plist'):
                capture.inspect(binary)


class NativeRuntimeContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='localvox-native-sdk-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / 'SDK with spaces'
        self.calls = []
        for name, value in [('system', 'Darwin'), ('machine', 'x86_64')]:
            fixture = patch.object(native.platform, name, return_value=value)
            fixture.start()
            self.addCleanup(fixture.stop)

    def fake_run(self, arguments, **options):
        args = [str(a) for a in arguments]
        self.calls.append(args)
        if args[:2] == ['git', 'clone']:
            source = Path(args[-1])
            source.mkdir()
            for name in native.LICENSES: (source / name).write_text('upstream fixture ' + name)
        elif 'rev-parse' in args:
            return native.COMMIT
        elif '--build_dir' in args:
            compiled = Path(args[args.index('--build_dir') + 1]) / 'Release'
            compiled.mkdir(parents=True)
            for name in (native.LIBRARY, 'libonnxruntime.1.dylib', 'libonnxruntime.dylib'):
                (compiled / name).write_bytes(b'native library fixture')
        elif args[:2] == ['lipo', '-archs']:
            return 'x86_64'
        return ''

    def build(self):
        with patch.object(native, 'run', side_effect=self.fake_run):
            return native.build(self.output)

    def test_complete_verified_sdk_is_reused_without_commands_or_rewrites(self):
        self.assertFalse(self.build()['reused'])
        self.assertTrue(native.verified(self.output))
        self.assertEqual((self.output / 'libonnxruntime.dylib').read_bytes(),
                         (self.output / native.LIBRARY).read_bytes())
        self.assertTrue((self.output / 'libonnxruntime.1.dylib').is_file())
        before = {p.relative_to(self.output): (p.read_bytes(), p.stat().st_mtime_ns)
                  for p in self.output.rglob('*') if p.is_file()}
        with patch.object(native, 'run', side_effect=AssertionError('unexpected work')):
            self.assertTrue(native.build(self.output)['reused'])
        self.assertEqual(before, {p.relative_to(self.output): (p.read_bytes(), p.stat().st_mtime_ns)
                                  for p in self.output.rglob('*') if p.is_file()})

    def test_corrupt_stale_and_unlisted_cache_contents_are_rejected_without_overwrite(self):
        for mode in ('corrupt', 'stale', 'unlisted', 'missing_license', 'bad_json'):
            with self.subTest(mode=mode):
                self.output = self.root / mode
                self.build()
                if mode == 'corrupt': (self.output / native.LIBRARY).write_bytes(b'corrupt')
                elif mode == 'unlisted': (self.output / 'unexpected.dylib').write_bytes(b'unlisted')
                elif mode == 'missing_license': (self.output / 'licenses/onnxruntime/LICENSE').unlink()
                elif mode == 'bad_json': (self.output / native.STAMP).write_text('null')
                else:
                    stamp = json.loads((self.output / native.STAMP).read_text())
                    stamp['commit'] = 'stale'
                    (self.output / native.STAMP).write_text(json.dumps(stamp))
                before = {p.relative_to(self.output): p.read_bytes() for p in self.output.rglob('*') if p.is_file()}
                self.assertFalse(native.verified(self.output))
                with patch.object(native, 'run', side_effect=AssertionError('unexpected work')):
                    with self.assertRaises(RuntimeError): native.build(self.output)
                self.assertEqual(before, {p.relative_to(self.output): p.read_bytes() for p in self.output.rglob('*') if p.is_file()})

    def test_failed_source_build_does_not_publish_a_finished_sdk(self):
        def fail(args, **options):
            if '--build_dir' in args: raise subprocess.CalledProcessError(7, args)
            return self.fake_run(args, **options)
        with patch.object(native, 'run', side_effect=fail):
            with self.assertRaises(subprocess.CalledProcessError): native.build(self.output)
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.root.iterdir()), [])

    def test_changed_upstream_tag_and_wrong_architecture_do_not_publish(self):
        for phase in ('revision', 'architecture'):
            def wrong(args, **options):
                if phase == 'revision' and 'rev-parse' in args: return 'wrong-commit'
                if phase == 'architecture' and args[0] == 'lipo': return 'arm64'
                return self.fake_run(args, **options)
            with patch.object(native, 'run', side_effect=wrong):
                with self.assertRaises(RuntimeError): native.build(self.output)
            self.assertFalse(self.output.exists())

    def test_unsupported_native_host_fails_before_network_or_output(self):
        with patch.object(native.platform, 'machine', return_value='arm64'):
            with patch.object(native, 'run', side_effect=AssertionError('unexpected work')):
                with self.assertRaises(RuntimeError): native.build(self.output)
        self.assertFalse(self.output.exists())


if __name__ == '__main__': unittest.main()
