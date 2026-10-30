#!/usr/bin/env python3
"""Collect a finite X, Slack and email digest. Run from a timer; secrets are read from a local file."""
import argparse
import datetime as dt
import fcntl
import json
import os
from pathlib import Path
import tempfile
import urllib.error
import urllib.parse
import urllib.request

HOME = Path.home()
CONFIG = HOME / '.config/herdr-digest/config.json'
DATA = Path(os.environ.get('HERDR_APP_HOME', HOME / '.config/herdr-app')) / 'data/digest'


def read(path, default):
    return json.loads(path.read_text()) if path.exists() else default


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(value, stream, ensure_ascii=False)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def schedule(times, now):
    if not times:
        raise ValueError('Configure at least one digest time')
    clocks = [dt.time.fromisoformat(t) for t in times]
    slots = sorted(dt.datetime.combine(now.date() + dt.timedelta(days=d), t)
                   for d in (-1, 0, 1) for t in clocks)
    return max(s for s in slots if s <= now), min(s for s in slots if s > now)


def request(path, token, params=None):
    url = 'https://api.x.com/2/' + path
    if params:
        url += '?' + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + token})
    try:
        with urllib.request.urlopen(req, timeout=30) as response:
            payload = json.load(response)
    except urllib.error.HTTPError as e:
        raise ValueError(f'X returned HTTP {e.code}. Check user-token access, scopes, credits and rate limits.') from None
    if payload.get('errors'):
        raise ValueError('X returned partial errors; keeping the previous digest and cursor')
    return payload


def collect(token, previous, max_pages=3):
    me = request('users/me', token)['data']['id']
    params = {'max_results': 100, 'expansions': 'author_id',
              'tweet.fields': 'created_at,note_tweet', 'user.fields': 'username',
              'exclude': 'retweets,replies'}
    if previous.get('cursor'):
        params['since_id'] = previous['cursor']
    else:
        params['start_time'] = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(hours=24)).strftime('%Y-%m-%dT%H:%M:%SZ')
    posts = {}
    for _ in range(max_pages):
        page = request(f'users/{me}/timelines/reverse_chronological', token, params)
        authors = {u['id']: u['username'] for u in page.get('includes', {}).get('users', [])}
        for post in page.get('data', []):
            posts[post['id']] = {'id': post['id'], 'author': authors.get(post['author_id'], post['author_id']),
                                 'text': post.get('note_tweet', {}).get('text', post['text'])[:12000]}
        cursor = page.get('meta', {}).get('next_token')
        if not cursor:
            return list(posts.values()), False
        params['pagination_token'] = cursor
    return list(posts.values()), True


def run(config_path, data, due=False):
    from helper import run_job
    from sources import checked_result, job_input
    data.mkdir(parents=True, exist_ok=True)
    with (data / 'worker.lock').open('w') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        try:
            if not config_path.exists():
                raise ValueError(f'Set up collection in {config_path}')
            config = read(config_path, {})
            now = dt.datetime.now()
            slot, next_slot = schedule(config.get('times', ['09:00', '18:00']), now)
            previous = read(data / 'latest.json', {})
            if due and previous.get('slot', '') >= slot.isoformat():
                return
            end = dt.datetime.now(dt.timezone.utc).isoformat()
            save(data / 'progress.json', {'phase':'Collecting X', 'helper':None})
            pending_path = data / 'pending.json'
            pending = read(pending_path, {})
            x_error, posts, sampled = '', [], False
            try:
                if pending and pending.get('previous_id') == previous.get('id'):
                    posts, sampled = pending['posts'], pending['sampled']
                else:
                    limit = config.get('max_pages', 3)
                    if type(limit) is not int or not 1 <= limit <= 20:
                        raise ValueError('max_pages must be between 1 and 20')
                    oauth = Path(config.get('x_oauth_file', '~/.config/herdr-digest/x-oauth.json')).expanduser()
                    if oauth.exists():
                        from x_auth import access_token
                        token = access_token(oauth)
                    else:
                        token = Path(config.get('x_token_file', '~/.config/herdr-digest/x-token')).expanduser().read_text().strip()
                    posts, sampled = collect(token, previous, limit)
                    save(pending_path, {'previous_id':previous.get('id'), 'posts':posts, 'sampled':sampled})
            except Exception:
                x_error = 'X collection failed; check token access, credits and rate limits'
            starts_path = data / 'source-starts.json'
            starts = read(starts_path, {})
            first = (dt.datetime.fromisoformat(end) - dt.timedelta(hours=24)).isoformat()
            scope = 'all' if config.get('slack_include_private') else 'public'
            if starts.get('slack_scope', 'public') != scope:
                starts['Slack'] = first
            starts['slack_scope'] = scope
            for kind in ('Slack', 'Email'):
                starts.setdefault(kind, first)
            save(starts_path, starts)
            watermarks_before = dict(previous.get('watermarks', {}))
            if previous.get('slack_scope', 'public') != scope:
                watermarks_before.pop('Slack', None)
            effective = dict(previous, watermarks=dict({k:starts[k] for k in ('Slack','Email')}, **watermarks_before))
            payload = job_input(posts, effective, config, end)
            def progress(address, state):
                save(data / 'progress.json', {'phase':'Gathering Slack and email / selecting digest',
                                             'helper':address, 'state':state})
            result = run_job(payload, data, progress)
            items, statuses = checked_result(result, payload)
            if x_error and all(s['status'] == 'error' for s in statuses.values()):
                raise ValueError('All sources failed; previous digest preserved')
            watermarks = dict(watermarks_before)
            errors = [x_error] if x_error else []
            summaries = ['X · '+(x_error if x_error else f'{len(posts)} new posts'+(' · newest batch only; older posts omitted' if sampled else ''))]
            for kind, status in statuses.items():
                if status['status'] == 'ok':
                    watermarks[kind] = end
                    summaries.append(f"{kind} · {status['count']} inspected" + (' · bounded sample' if status['sampled'] else ''))
                else:
                    summaries.append(kind+' · failed')
                    errors.append(kind+': '+status['error'])
            cursors = [p['id'] for p in posts]
            if previous.get('cursor'):
                cursors.append(previous['cursor'])
            digest = {'id':end, 'slot':slot.isoformat(), 'cursor':max(cursors,key=int) if cursors else None,
                      'watermarks':watermarks, 'slack_scope':scope, 'status':f'{len(items)} selected across X, Slack and email',
                      'next':next_slot.strftime('%a %H:%M'), 'items':items, 'sources':summaries,
                      'profile':bool(config.get('interests','').strip())}
            save(data / 'latest.json', digest)
            save(data / 'status.json', {'error':'; '.join(errors)})
            save(data / 'progress.json', {'phase':'Ready', 'helper':read(data / 'progress.json',{}).get('helper'), 'state':'idle'})
            pending_path.unlink(missing_ok=True)
        except Exception as e:
            message = str(e) if isinstance(e, ValueError) else f'{type(e).__name__}: collection failed; previous digest preserved'
            save(data / 'status.json', {'error':message})
            progress_state = read(data / 'progress.json', {})
            save(data / 'progress.json', dict(progress_state, phase='Needs attention'))
            print(message)
            return 1
    return 0


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--due', action='store_true', help='Only collect if a scheduled slot is due')
    parser.add_argument('--config', type=Path, default=CONFIG)
    parser.add_argument('--data', type=Path, default=DATA)
    args = parser.parse_args()
    raise SystemExit(run(args.config, args.data, args.due))
