#!/usr/bin/env python3
"""Safe violating examples for the live-record validator; no provider calls."""
import copy
import sys
import unittest

sys.dont_write_bytecode = True
from live_connections import validate


def fixture():
    turns = []
    for index in range(2):
        events = []
        for tool in (["read", "write", "edit", "bash"] if index == 0 else ["read", "edit", "bash"]):
            events.append({"type":"tool_started", "call":{"id":tool, "name":tool, "arguments":{"command":"python3 -B -c 'assert True'"}}})
            events.append({"type":"tool_finished", "result":{"call_id":tool,"tool":tool,"success":True,"exit_code":0 if tool == "bash" else None}})
        events.append({"type":"turn_finished", "status":"complete"})
        turns.append({"source":"def dc_value():\n    return " + str(11 if index == 0 else 18) + "\n", "events":[{"connection":"openai-api", "event":event} for event in events]})
    return {"input_digest":"current", "adapter":"openai-api", "transport":"live-default-endpoint", "auth_method":"api-key", "seed":10, "turns":turns}


class LiveEvidence(unittest.TestCase):
    def test_corrected_record(self):
        self.assertEqual(validate(fixture(), "current"), "dc_value")

    def test_violations(self):
        baseline = fixture()
        faults = [
            lambda r: r.update(input_digest="stale"),
            lambda r: r.update(transport="fixture"),
            lambda r: r.update(auth_method="subscription"),
            lambda r: r["turns"][0].update(source="def dc_value():\n    return 0\n"),
            lambda r: r["turns"][1].update(source="def dc_unrelated():\n    return 18\n"),
            lambda r: r["turns"][0]["events"][-1]["event"].update(status="failed"),
            lambda r: r["turns"][0]["events"][1]["event"]["result"].update(call_id="unrelated"),
            lambda r: r["turns"][0]["events"][6]["event"]["call"]["arguments"].update(command="echo pretend"),
        ]
        for index, fault in enumerate(faults):
            record = copy.deepcopy(baseline)
            fault(record)
            with self.subTest(fault=index), self.assertRaises(AssertionError):
                validate(record, "current")


if __name__ == "__main__":
    unittest.main()
