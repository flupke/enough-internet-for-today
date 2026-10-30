import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import helper

class HelperTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)
        self.calls=[]
        self.wait={'ended':True,'status':'done','reason':'status'}
        self.sent=False

    def endpoint(self,*args,**kwargs):
        self.calls.append(args)
        if args[0]=='get': return None
        if args[0]=='run': return {'address':'t99','first_message':{'sent':self.sent}}
        if args[0]=='wait':
            result=next((self.root/'jobs').iterdir())/'result.json'
            result.write_text(json.dumps({'items':[]}))
            return self.wait

    def test_new_helper_requires_confirmed_first_task_delivery(self):
        with patch.object(helper,'WORKSPACE',self.root),patch.object(helper,'hc',side_effect=self.endpoint):
            with self.assertRaises(ValueError): helper.run_job({'test':True},self.root)
        self.assertFalse(any(c[0]=='send' for c in self.calls))

    def test_reads_result_file_after_helper_finishes(self):
        self.sent=True
        with patch.object(helper,'WORKSPACE',self.root),patch.object(helper,'hc',side_effect=self.endpoint):
            self.assertEqual(helper.run_job({'test':True},self.root),{'items':[]})
        self.assertTrue(any(c[0]=='wait' for c in self.calls))

    def test_blocked_helper_is_reported_without_forcing_or_killing(self):
        self.sent=True;self.wait={'ended':True,'status':'blocked','reason':'status'}
        with patch.object(helper,'WORKSPACE',self.root),patch.object(helper,'hc',side_effect=self.endpoint):
            with self.assertRaises(ValueError): helper.run_job({},self.root)
        self.assertFalse(any(c[0] in ('kill','send') for c in self.calls))

    def test_never_reuses_unrelated_terminal(self):
        with patch.object(helper,'WORKSPACE',self.root),patch.object(helper,'hc',return_value={'address':'t88','labels':{}}):
            with self.assertRaises(ValueError): helper.run_job({},self.root)

    def test_reuses_owned_idle_helper_and_waits_for_submitted_turn(self):
        def endpoint(*args,**kwargs):
            self.calls.append(args)
            if args[0]=='get': return {'address':'t99','state':'live','status':{'state':'idle'},'labels':{'eit.owner':helper.NAME}}
            if args[0]=='send':
                result=next((self.root/'jobs').iterdir())/'result.json'
                result.write_text(json.dumps({'items':[]}))
                return {'started':True,'wait':{'ended':True,'status':'done','reason':'status'}}
            raise AssertionError(args)
        with patch.object(helper,'WORKSPACE',self.root),patch.object(helper,'hc',side_effect=endpoint):
            self.assertEqual(helper.run_job({},self.root),{'items':[]})
        self.assertTrue(any(c[0]=='send' and '--wait' in c for c in self.calls))
        self.assertFalse(any(c[0]=='run' for c in self.calls))

if __name__=='__main__': unittest.main()
