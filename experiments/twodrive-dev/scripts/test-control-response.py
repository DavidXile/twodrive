#!/usr/bin/env python3
"""An old signed response at a reused sequence must not confirm a new job."""
import importlib.util
import json
import os
from pathlib import Path
import unittest
from unittest.mock import patch
SPEC=importlib.util.spec_from_file_location('control',Path(os.environ.get('TWODRIVE_CONTROL_SCRIPT',str(Path(__file__).with_name('control-test-agent.py')))))
MODULE=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

class ControlResponse(unittest.TestCase):
    def fake(self, returned_signature):
        control=object.__new__(MODULE.Control)
        control.binary='synthetic-signer'
        control.state=Path('synthetic-state')
        control.session='session'
        replies=iter([{'seq':0}, {'seq':1,'data':{'request_signature':returned_signature}}])
        control.response=lambda:next(replies)
        control.request=lambda *args:b''
        def sign(args, **kwargs):
            Path(args[-1]).write_text(json.dumps({'signature':[1]*64}))
        return control,sign

    def test_old_response_at_same_sequence_is_rejected(self):
        control,sign=self.fake([2]*64)
        with patch.object(MODULE.subprocess,'run',sign):
            with self.assertRaisesRegex(ValueError,'another request'):
                control.command({'action':'stop'})

    def test_exact_request_ack_is_accepted(self):
        control,sign=self.fake([1]*64)
        with patch.object(MODULE.subprocess,'run',sign):
            self.assertEqual(control.command({'action':'stop'})['seq'],1)

if __name__=='__main__': unittest.main()
