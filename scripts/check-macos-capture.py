#!/usr/bin/env python3
"""Inspect the actual daemon Mach-O metadata; no device or permission changes."""
import argparse
from pathlib import Path
import plistlib
import struct


def inspect(binary):
    with Path(binary).open('rb') as stream:
        header = stream.read(32)
        endian = {b'\xcf\xfa\xed\xfe': '<', b'\xfe\xed\xfa\xcf': '>'}.get(header[:4])
        if not endian or len(header) != 32:
            raise ValueError('Expected a native 64-bit Mach-O daemon')
        _, _, _, _, count, size, _, _ = struct.unpack(endian + 'IiiIIIII', header)
        if size > 16 * 1024 * 1024 or count > size // 8:
            raise ValueError('Invalid Mach-O load commands')
        commands = stream.read(size)
        position = 0
        for _ in range(count):
            if position + 8 > len(commands):
                raise ValueError('Truncated Mach-O command')
            command, length = struct.unpack_from(endian + 'II', commands, position)
            if length < 8 or position + length > len(commands):
                raise ValueError('Invalid Mach-O command size')
            if command == 0x19:  # LC_SEGMENT_64
                if length < 72:
                    raise ValueError('Truncated Mach-O segment')
                sections = struct.unpack_from(endian + 'I', commands, position + 64)[0]
                if 72 + sections * 80 > length:
                    raise ValueError('Invalid Mach-O sections')
                for index in range(sections):
                    section = position + 72 + index * 80
                    name, segment = struct.unpack_from('16s16s', commands, section)
                    if name.rstrip(b'\0') != b'__info_plist' or segment.rstrip(b'\0') != b'__TEXT':
                        continue
                    byte_count, offset = struct.unpack_from(endian + 'QI', commands, section + 40)
                    if not 0 < byte_count <= 1024 * 1024:
                        raise ValueError('Invalid embedded Info.plist size')
                    stream.seek(offset)
                    metadata = plistlib.loads(stream.read(byte_count))
                    if metadata.get('CFBundleIdentifier') != 'ru.localvox.daemon':
                        raise ValueError('Missing stable daemon bundle identifier')
                    for key in ('NSMicrophoneUsageDescription', 'NSAudioCaptureUsageDescription'):
                        if not isinstance(metadata.get(key), str) or not metadata[key].strip():
                            raise ValueError(f'Missing capture usage description: {key}')
                    if not metadata.get('CFBundleVersion') or '@VERSION@' in metadata['CFBundleVersion']:
                        raise ValueError('Unresolved daemon version')
                    return metadata
            position += length
    raise ValueError('Daemon has no __TEXT,__info_plist; macOS capture consent is not configured')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    args = parser.parse_args()
    result = inspect(args.binary)
    print(f"PASS: {result['CFBundleIdentifier']} {result['CFBundleVersion']} — microphone and system audio usage descriptions embedded")
