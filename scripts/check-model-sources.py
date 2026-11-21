#!/usr/bin/env python3
"""Check public model URLs without downloading weights or changing the installation.

HEAD follows redirects; a bounded Range GET catches endpoints that advertise a
file but refuse downloads. GitHub release digests are compared with the catalog.
This does not compute hashes of the full weights or execute inference.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import sys
import urllib.error
import urllib.request
import urllib.parse

MANIFEST = Path(__file__).resolve().with_name('models.json')
HEADERS = {'User-Agent': 'localvox-fetch-models/1.0', 'Accept-Encoding': 'identity'}


def check_published_digest(url, expected, timeout):
    """Compare GitHub's published asset digest without downloading its weights."""
    parsed = urllib.parse.urlsplit(url)
    parts = parsed.path.split('/')
    if parsed.netloc != 'github.com' or len(parts) != 7 or parts[3:5] != ['releases', 'download']:
        return
    owner, repo, tag, name = (urllib.parse.unquote(parts[i]) for i in (1, 2, 5, 6))
    api = 'https://api.github.com/repos/{}/{}/releases/tags/{}'.format(
        urllib.parse.quote(owner, safe=''), urllib.parse.quote(repo, safe=''),
        urllib.parse.quote(tag, safe=''))
    headers = dict(HEADERS)
    if os.getenv('GH_TOKEN'):
        headers['Authorization'] = 'Bearer ' + os.environ['GH_TOKEN']
    with urllib.request.urlopen(urllib.request.Request(api, headers=headers), timeout=timeout) as response:
        data = response.read(4 * 1024 * 1024 + 1)
    if len(data) > 4 * 1024 * 1024:
        raise ValueError('GitHub release metadata exceeds 4 MiB')
    assets = [asset for asset in json.loads(data).get('assets', []) if asset.get('name') == name]
    if len(assets) != 1 or assets[0].get('digest') != 'sha256:' + expected:
        raise ValueError('published GitHub asset SHA-256 differs from the model manifest')


def check_item(component, item, timeout=20):
    url = item['url']
    label = f"{component}: {url}"
    try:
        if item.get('sha256'):
            check_published_digest(url, item['sha256'], timeout)
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
