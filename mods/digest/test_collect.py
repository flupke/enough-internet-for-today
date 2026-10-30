import datetime as dt
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import collect

class DigestTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.config = self.root / 'config.json'
        self.data = self.root / 'data'
        token = self.root / 'token'
        token.write_text('test-token')
        collect.save(self.config, {'x_token_file': str(token), 'x_oauth_file':str(self.root / 'oauth.json'), 'llm_command': ['test-selector']})
        self.posts = [{'id': '123', 'author': 'alice', 'text': 'A useful release'}]
        self.selection = [{'id': '123', 'title': 'Release', 'summary': 'A useful release', 'why': 'New capability'}]

    def run_worker(self, selected=None):
        with patch.object(collect, 'collect', return_value=(self.posts, False)), patch.object(collect.subprocess, 'run') as llm:
            llm.return_value.returncode = 0
            llm.return_value.stdout = json.dumps(self.selection if selected is None else selected)
            return collect.run(self.config, self.data)

    def test_publishes_verified_links_and_cursor(self):
        self.assertEqual(self.run_worker(), 0)
        digest = collect.read(self.data / 'latest.json', {})
        self.assertEqual(digest['cursor'], '123')
        self.assertEqual(digest['items'][0]['url'], 'https://x.com/i/status/123')
        self.assertEqual(digest['items'][0]['summary'], 'A useful release')

    def test_failed_selection_preserves_previous_digest(self):
        self.run_worker()
        previous = (self.data / 'latest.json').read_bytes()
        self.assertEqual(self.run_worker([dict(self.selection[0], id='999')]), 1)
        self.assertEqual((self.data / 'latest.json').read_bytes(), previous)
        self.assertTrue(collect.read(self.data / 'status.json', {})['error'])

    def test_selection_retry_reuses_pending_posts(self):
        self.assertEqual(self.run_worker([dict(self.selection[0], id='999')]), 1)
        with patch.object(collect, 'collect', side_effect=AssertionError('paid refetch')), patch.object(collect.subprocess, 'run') as llm:
            llm.return_value.returncode = 0
            llm.return_value.stdout = json.dumps(self.selection)
            self.assertEqual(collect.run(self.config, self.data), 0)
        self.assertFalse((self.data / 'pending.json').exists())

    def test_accepts_claude_structured_output(self):
        self.assertEqual(self.run_worker({'is_error':False, 'structured_output':{'items':self.selection}}), 0)
        self.assertEqual(collect.read(self.data / 'latest.json', {})['items'][0]['id'], '123')

    def test_zero_selected_is_valid_and_marks_posts_processed(self):
        self.assertEqual(self.run_worker([]), 0)
        digest = collect.read(self.data / 'latest.json', {})
        self.assertEqual(digest['items'], [])
        self.assertEqual(digest['cursor'], '123')

    def test_empty_first_collection_keeps_cursor_absent(self):
        self.posts = []
        self.assertEqual(self.run_worker(), 0)
        previous = collect.read(self.data / 'latest.json', {})
        self.assertIsNone(previous['cursor'])
        with patch.object(collect, 'request', side_effect=[{'data': {'id':'42'}}, {}]) as api:
            self.assertEqual(collect.collect('test', previous), ([], False))
        params = api.call_args.args[2]
        self.assertIn('start_time', params)
        self.assertNotIn('since_id', params)

    def test_processed_slot_does_not_fetch_again(self):
        self.run_worker()
        with patch.object(collect, 'collect', side_effect=AssertionError('unexpected fetch')):
            self.assertIsNone(collect.run(self.config, self.data, due=True))

    def test_paginated_timeline_includes_all_posts(self):
        responses = [{'data': {'id': '42'}},
                     {'data': [{'id':'124','author_id':'a','text':'new'}], 'meta': {'next_token':'page2'}},
                     {'data': [{'id':'123','author_id':'a','text':'older'}]}]
        with patch.object(collect, 'request', side_effect=responses):
            posts, sampled = collect.collect('test', {'cursor':'122'})
        self.assertFalse(sampled)
        self.assertEqual({p['id'] for p in posts}, {'123','124'})

    def test_bounded_collection_marks_sampled_coverage(self):
        with patch.object(collect, 'request', side_effect=[{'data':{'id':'42'}}, {'data':[{'id':'123','author_id':'a','text':'new'}], 'meta': {'next_token':'more'}}]):
            posts, sampled = collect.collect('test', {}, max_pages=1)
        self.assertTrue(sampled)
        self.assertEqual([p['id'] for p in posts], ['123'])

    def test_schedule_rolls_over_midnight(self):
        slot, following = collect.schedule(['09:00','18:00'], dt.datetime(2026,10,30,7))
        self.assertEqual(slot, dt.datetime(2026,10,29,18))
        self.assertEqual(following, dt.datetime(2026,10,30,9))

    def test_invalid_duplicate_and_oversize_selections_rejected(self):
        for selection in [self.selection*2, self.selection*6, [dict(self.selection[0], summary='')]]:
            with self.assertRaises(ValueError):
                collect.validate(selection, self.posts)

if __name__ == '__main__':
    unittest.main()
