import json
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch
import x_auth

class AuthTests(unittest.TestCase):
    def test_rejects_forged_callback_and_declined_consent(self):
        for target in ['/callback?code=secret&state=wrong', '/other?code=secret&state=ok', '/callback?error=access_denied&state=ok']:
            with self.assertRaises(ValueError):
                x_auth.callback_values(target, 'ok')
        self.assertEqual(x_auth.callback_values('/callback?code=abc&state=ok', 'ok'), 'abc')

    def test_refreshes_expired_token_and_persists_rotated_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'oauth.json'
            x_auth.atomic_json(path, {'tokens': {'access_token':'old', 'refresh_token':'old-refresh', 'expires_at':0}, 'secrets_file':'/unused', 'public_client':True})
            refreshed = {'access_token':'new', 'refresh_token':'new-refresh', 'expires_at':time.time()+7200}
            with patch.object(x_auth, 'credentials', return_value=('client', None)), patch.object(x_auth, 'exchange', return_value=refreshed) as endpoint:
                self.assertEqual(x_auth.access_token(path), 'new')
                self.assertEqual(x_auth.access_token(path), 'new')
                self.assertEqual(endpoint.call_count, 1)
                self.assertEqual(endpoint.call_args.args[0]['refresh_token'], 'old-refresh')
            self.assertEqual(json.loads(path.read_text())['tokens']['refresh_token'], 'new-refresh')
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)

    def test_secret_file_is_parsed_without_executing_shell(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'secrets'
            path.write_text("export ENOUGH_INTERNET_FOR_TODAY_X_COM_CLIENT_ID='client'\nENOUGH_INTERNET_FOR_TODAY_X_COM_SECRET='secret'\nOTHER=$(bad-command)\n")
            self.assertEqual(x_auth.credentials(path), ('client', 'secret'))
            self.assertEqual(x_auth.credentials(path, public=True), ('client', None))

if __name__ == '__main__':
    unittest.main()
