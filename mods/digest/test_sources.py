import copy
import unittest
from sources import job_input,checked_result

class SourceTests(unittest.TestCase):
    def setUp(self):
        self.payload=job_input([],{}, {},'2026-10-30T18:00:00+00:00')
        self.result={'sources':{k:{'status':'ok','count':1,'sampled':False,'error':''} for k in ('Slack','Email')},
            'candidates':[{'id':'slack:C1:123','kind':'Slack','author':'Alice','timestamp':'2026-10-30T17:00:00Z','text':'Ship tomorrow','url':'https://team.slack.com/archives/C1/p123'},
                          {'id':'email:abc','kind':'Email','author':'Bob','timestamp':'2026-10-30T17:00:00Z','text':'Meeting moved','url':'https://mail.google.com/mail/u/0/#inbox/abc'}],
            'items':[{'id':'email:abc','title':'Meeting','summary':'Meeting moved','why':'Schedule changed'}]}

    def test_selected_items_keep_original_source_links(self):
        items,status=checked_result(self.result,self.payload)
        self.assertEqual(items[0]['source'],'Email · Bob')
        self.assertEqual(items[0]['url'],'https://mail.google.com/mail/u/0/#inbox/abc')

    def test_rejects_fabricated_id_unofficial_link_old_message_and_duplicate(self):
        variants=[]
        bad=copy.deepcopy(self.result);bad['sources']['Unexpected']={'status':'ok','count':0,'sampled':False,'error':''};variants.append(bad)
        bad=copy.deepcopy(self.result);bad['items'][0]['id']='email:fake';variants.append(bad)
        bad=copy.deepcopy(self.result);bad['candidates'][0]['url']='https://team.slack.com.attacker.test/archives/C1/p123';variants.append(bad)
        bad=copy.deepcopy(self.result);bad['candidates'][0]['timestamp']='2025-01-01T00:00:00Z';variants.append(bad)
        bad=copy.deepcopy(self.result);bad['items']*=2;variants.append(bad)
        bad=copy.deepcopy(self.result);bad['sources']['Email']['status']='error';bad['sources']['Email']['error']='Denied';variants.append(bad)
        for result in variants:
            with self.assertRaises(ValueError): checked_result(result,self.payload)

    def test_failed_source_cannot_look_like_empty_success(self):
        self.result['sources']['Slack']={'status':'error','count':0,'sampled':False,'error':'No access'}
        self.result['candidates']=self.result['candidates'][1:]
        items,status=checked_result(self.result,self.payload)
        self.assertEqual(status['Slack']['status'],'error')
        self.assertEqual(len(items),1)

if __name__=='__main__': unittest.main()
