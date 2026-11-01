"""The deciding half of fixture_server.py: python3 -m unittest discover .github/scripts"""
import json
import unittest

from fixture_server import REFUSED, Replay

EXCHANGES = [
    {"method": "GET", "path": "/a", "body": None, "status": 200, "response": {"id": "1"}},
    {"method": "POST", "path": "/b", "body": {"x": 1}, "status": 201, "response": None},
]


class Answer(unittest.TestCase):
    def test_each_request_in_order_gets_its_exchanges_answer(self):
        replay = Replay(EXCHANGES)
        self.assertEqual(replay.answer("GET", "/a", b""), (200, {"id": "1"}))
        self.assertEqual(replay.answer("POST", "/b", json.dumps({"x": 1}).encode()), (201, None))
        self.assertEqual(replay.done(), (200, "all 2 exchanges were served"))

    def test_a_request_out_of_order_or_with_another_body_is_refused(self):
        replay = Replay(EXCHANGES)
        status, _ = replay.answer("POST", "/b", b'{"x": 1}')
        self.assertEqual(status, REFUSED)
        replay.answer("GET", "/a", b"")
        status, _ = replay.answer("POST", "/b", b'{"x": 2}')
        self.assertEqual(status, REFUSED)
        status, said = replay.done()
        self.assertEqual(status, 500)
        self.assertIn("exchange 1 is GET /a, not POST /b", said)
        self.assertIn("exchange 2, POST /b, sent another body", said)

    def test_a_request_after_the_last_exchange_is_refused(self):
        replay = Replay(EXCHANGES[:1])
        replay.answer("GET", "/a", b"")
        status, _ = replay.answer("GET", "/a", b"")
        self.assertEqual(status, REFUSED)
        self.assertIn("came after every exchange", replay.done()[1])

    def test_a_run_that_stops_early_is_not_done(self):
        replay = Replay(EXCHANGES)
        replay.answer("GET", "/a", b"")
        self.assertEqual(replay.done(), (500, "1 of 2 exchanges were served"))


if __name__ == "__main__":
    unittest.main()
