"""X OAuth PKCE authorization and automatic token refresh; no credential logging."""
import argparse
import base64
import fcntl
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import secrets
import shlex
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import webbrowser

PREFIX = 'ENOUGH_INTERNET_FOR_TODAY_X_COM_'
DEFAULT_STORE = Path.home() / '.config/herdr-digest/x-oauth.json'
REDIRECT = 'http://127.0.0.1:8765/callback'


def credentials(path=Path.home() / '.secrets', public=False):
    values = {}
    for line in path.read_text().splitlines():
        parts = shlex.split(line, comments=True)
        if parts and parts[0] == 'export':
            parts = parts[1:]
        if len(parts) == 1 and '=' in parts[0]:
            key, value = parts[0].split('=', 1)
            if key.startswith(PREFIX):
                values[key] = value
    client_id = values.get(PREFIX + 'CLIENT_ID')
    if not client_id:
        raise ValueError('X Client ID not found in the configured secrets file')
    return client_id, None if public else values.get(PREFIX + 'SECRET')


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(value, stream)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def exchange(params, client_id, client_secret=None):
    headers = {'Content-Type': 'application/x-www-form-urlencoded'}
    if client_secret:
        encoded = base64.b64encode(f'{client_id}:{client_secret}'.encode()).decode()
        headers['Authorization'] = 'Basic ' + encoded
    else:
        params = dict(params, client_id=client_id)
    req = urllib.request.Request('https://api.x.com/2/oauth2/token',
                                 data=urllib.parse.urlencode(params).encode(), headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=25) as response:
            result = json.load(response)
    except urllib.error.HTTPError as e:
        raise ValueError(f'X token exchange returned HTTP {e.code}. Check app type and OAuth credentials.') from None
    if not result.get('access_token') or not result.get('refresh_token'):
        raise ValueError('X did not issue both access and refresh tokens; authorize with offline.access')
    result['expires_at'] = time.time() + float(result.get('expires_in', 7200))
    return result


def access_token(path=DEFAULT_STORE):
    with path.with_suffix('.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        stored = json.loads(path.read_text())
        tokens = stored['tokens']
        if tokens['expires_at'] <= time.time() + 120:
            client_id, client_secret = credentials(Path(stored['secrets_file']), stored['public_client'])
            tokens = exchange({'grant_type': 'refresh_token', 'refresh_token': tokens['refresh_token']},
                              client_id, client_secret)
            stored['tokens'] = tokens
            atomic_json(path, stored)
        return tokens['access_token']


def callback_values(target, expected_state):
    url = urllib.parse.urlsplit(target)
    args = urllib.parse.parse_qs(url.query)
    if url.path != '/callback':
        raise ValueError('Unexpected callback path')
    if not secrets.compare_digest(args.get('state', [''])[0], expected_state):
        raise ValueError('Invalid OAuth state')
    if args.get('error'):
        raise ValueError('X authorization was declined')
    if not args.get('code', [''])[0]:
        raise ValueError('Missing authorization code')
    return args['code'][0]


def authorize(store, secrets_file, public):
    client_id, client_secret = credentials(secrets_file, public)
    state, verifier = secrets.token_urlsafe(32), secrets.token_urlsafe(64)
    challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip('=')
    params = {'response_type': 'code', 'client_id': client_id, 'redirect_uri': REDIRECT,
              'scope': 'tweet.read users.read offline.access', 'state': state,
              'code_challenge': challenge, 'code_challenge_method': 'S256'}
    outcome = {}

    class Callback(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            try:
                code = callback_values(self.path, state)
                tokens = exchange({'grant_type':'authorization_code', 'code':code,
                                   'redirect_uri':REDIRECT, 'code_verifier':verifier}, client_id, client_secret)
                atomic_json(store, {'tokens':tokens, 'secrets_file':str(secrets_file), 'public_client':public})
                outcome['ok'] = True
                body = b'Enough Internet for Today is connected. You can close this tab.'
                status = 200
            except ValueError as e:
                body = str(e).encode()
                status = 400
                if 'declined' in str(e) or 'exchange' in str(e):
                    outcome['error'] = str(e)
            except Exception:
                outcome['error'] = 'Could not complete authorization; credentials were not logged'
                body = outcome['error'].encode()
                status = 500
            self.send_response(status)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            self.send_header('Cache-Control', 'no-store')
            self.end_headers()
            self.wfile.write(body)

    with HTTPServer(('127.0.0.1',8765),Callback) as server:
        server.timeout = 1
        url = 'https://x.com/i/oauth2/authorize?' + urllib.parse.urlencode(params)
        print('Opening X authorization. Approve read access in your browser.', flush=True)
        print(url, flush=True)
        webbrowser.open(url)
        deadline = time.monotonic() + 300
        while not outcome and time.monotonic() < deadline:
            server.handle_request()
    if not outcome.get('ok'):
        raise ValueError(outcome.get('error', 'Authorization timed out; run it again when ready'))
    print('X connected. Tokens saved locally with mode 0600.', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--authorize', action='store_true', required=True)
    parser.add_argument('--store', type=Path, default=DEFAULT_STORE)
    parser.add_argument('--secrets-file', type=Path, default=Path.home()/'.secrets')
    parser.add_argument('--public-client', action='store_true', help='Native app: do not use a client secret')
    args = parser.parse_args()
    try:
        args.store.parent.mkdir(parents=True, exist_ok=True)
        authorize(args.store, args.secrets_file, args.public_client)
    except (ValueError, OSError) as e:
        print(str(e) if isinstance(e, ValueError) else 'Authorization could not start; check callback port and file access')
        raise SystemExit(1)
