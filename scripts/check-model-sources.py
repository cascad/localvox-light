#!/usr/bin/env python3
"""Check public model URLs without downloading weights or changing the installation.

HEAD follows redirects; a bounded Range GET catches endpoints that advertise a
file but refuse downloads. This checks availability, not full hashes/inference.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import sys
import urllib.error
import urllib.request

MANIFEST = Path(__file__).resolve().with_name('models.json')
HEADERS = {'User-Agent': 'localvox-fetch-models/1.0', 'Accept-Encoding': 'identity'}


def check_item(component, item, timeout=20):
    url = item['url']
    label = f"{component}: {url}"
    try:
        request = urllib.request.Request(url, method='HEAD', headers=HEADERS)
        with urllib.request.urlopen(request, timeout=timeout) as response:
            size = response.headers.get('Content-Length')
            if item.get('size') is not None and size is not None and int(size) != item['size']:
                raise ValueError(f"expected {item['size']} bytes, HEAD reports {size}")
        request = urllib.request.Request(url, headers={**HEADERS, 'Range': 'bytes=0-63'})
        with urllib.request.urlopen(request, timeout=timeout) as response:
            # A server may ignore Range. Never read the rest of a large model.
            prefix = response.read(64)
            if not prefix:
                raise ValueError('empty download')
            if item['kind'] == 'zip' and not prefix.startswith(b'PK\x03\x04'):
                raise ValueError('download is not a ZIP archive')
            if 'text/html' in response.headers.get('Content-Type', '').lower():
                raise ValueError('download is an HTML page')
        return True, f'OK {label}'
    except urllib.error.HTTPError as error:
        code = error.code
        error.close()
        return False, f'FAIL {label}: HTTP {code}'
    except (OSError, ValueError) as error:
        return False, f'FAIL {label}: {error}'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, default=MANIFEST)
    parser.add_argument('--timeout', type=float, default=20)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    manifest = json.loads(args.manifest.read_text(encoding='utf-8'))
    items = [(component['id'], item) for component in manifest['components']
             for item in component['items'] if item['kind'] in ('file', 'zip')]
    if not items:
        parser.error('manifest contains no downloadable models')
    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(pool.map(lambda pair: check_item(*pair, timeout=args.timeout), items))
    for _, message in results:
        print(message)
    print(f'{sum(ok for ok, _ in results)}/{len(items)} public model sources available; '
          'full weights and hashes were not checked.')
    return 0 if all(ok for ok, _ in results) else 1


if __name__ == '__main__':
    sys.exit(main())
