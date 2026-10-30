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
        collect.save(self.config, {'x_token_file':str(token), 'x_oauth_file':str(self.root/'oauth.json')})
        self.posts = [{'id':'123','author':'alice','text':'A useful release'}]
        self.result = {'sources':{k:{'status':'ok','count':0,'sampled':False,'error':''} for k in ('Slack','Email')}, 'candidates':[],
                       'items':[{'id':'x:123','title':'Release','summary':'A useful release','why':'New capability'}]}

    def run_worker(self):
        with patch.object(collect, 'collect', return_value=(self.posts,False)), patch('helper.run_job',return_value=self.result):
            return collect.run(self.config,self.data)

    def test_publishes_verified_links_and_independent_watermarks(self):
        self.assertEqual(self.run_worker(),0)
        digest=collect.read(self.data/'latest.json',{})
        self.assertEqual(digest['cursor'],'123')
        self.assertEqual(digest['items'][0]['url'],'https://x.com/i/status/123')
        self.assertEqual(set(digest['watermarks']),{'Slack','Email'})

    def test_failed_selection_preserves_previous_digest_and_cached_x(self):
        self.run_worker()
        previous=(self.data/'latest.json').read_bytes()
        self.result['items'][0]['id']='unknown'
        self.assertEqual(self.run_worker(),1)
        self.assertEqual((self.data/'latest.json').read_bytes(),previous)
        self.result['items'][0]['id']='x:123'
        with patch.object(collect,'collect',side_effect=AssertionError('paid refetch')), patch('helper.run_job',return_value=self.result):
            self.assertEqual(collect.run(self.config,self.data),0)

    def test_source_failure_is_visible_without_advancing_its_watermark(self):
        self.run_worker()
        previous=collect.read(self.data/'latest.json',{})['watermarks']['Slack']
        self.result['sources']['Slack']={'status':'error','count':0,'sampled':False,'error':'Slack unavailable'}
        self.assertEqual(self.run_worker(),0)
        digest=collect.read(self.data/'latest.json',{})
        self.assertEqual(digest['watermarks']['Slack'],previous)
        self.assertIn('Slack unavailable',collect.read(self.data/'status.json',{})['error'])
        self.assertIn('Slack · failed',digest['sources'])

    def test_initial_source_failure_keeps_original_boundary_days_later(self):
        self.result['sources']['Slack']={'status':'error','count':0,'sampled':False,'error':'Unavailable'}
        real_datetime=dt.datetime
        class Clock(real_datetime):
            day=30
            @classmethod
            def now(cls,tz=None):
                value=real_datetime(2026,10,30,18,tzinfo=dt.timezone.utc)+dt.timedelta(days=cls.day-30)
                return value if tz else value.replace(tzinfo=None)
        payloads=[]
        def gather(payload,*args):
            payloads.append(payload)
            return self.result
        with patch.object(collect.dt,'datetime',Clock),patch.object(collect,'collect',return_value=(self.posts,False)),patch('helper.run_job',side_effect=gather):
            self.assertEqual(collect.run(self.config,self.data),0)
            Clock.day=32
            self.assertEqual(collect.run(self.config,self.data),0)
        self.assertEqual(payloads[0]['windows']['Slack']['since'],'2026-10-29T18:00:00+00:00')
        self.assertEqual(payloads[1]['windows']['Slack']['since'],payloads[0]['windows']['Slack']['since'])
        self.assertNotEqual(payloads[1]['windows']['Slack']['until'],payloads[0]['windows']['Slack']['until'])

    def test_expanding_slack_scope_starts_a_new_collection_window(self):
        self.run_worker()
        settings=collect.read(self.config,{})
        settings['slack_include_private']=True
        collect.save(self.config,settings)
        payloads=[]
        def gather(payload,*args):
            payloads.append(payload)
            return self.result
        with patch.object(collect,'collect',return_value=(self.posts,False)),patch('helper.run_job',side_effect=gather):
            self.assertEqual(collect.run(self.config,self.data),0)
        latest=collect.read(self.data/'latest.json',{})
        self.assertEqual(latest['slack_scope'],'all')
        self.assertIn('explicitly authorized private channels and DMs',payloads[0]['instructions'])
        self.assertLess(payloads[0]['windows']['Slack']['since'],latest['watermarks']['Slack'])

    def test_processed_slot_does_not_launch_a_helper(self):
        self.run_worker()
        with patch('helper.run_job',side_effect=AssertionError('duplicate helper')):
            self.assertIsNone(collect.run(self.config,self.data,due=True))

    def test_x_failure_still_allows_mcp_digest(self):
        self.result['items']=[]
        with patch.object(collect,'collect',side_effect=ValueError('X unavailable')), patch('helper.run_job',return_value=self.result):
            self.assertEqual(collect.run(self.config,self.data),0)
        self.assertIn('X collection failed',collect.read(self.data/'status.json',{})['error'])
        self.assertIsNone(collect.read(self.data/'latest.json',{})['cursor'])

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


if __name__ == '__main__': unittest.main()
