#!/usr/bin/env python3
"""The recorded-fixture server CI's dogfood job points herald at (ADR-0010).

Usage:
  fixture_server.py FIXTURE PORT

Serves FIXTURE's exchanges in order on 127.0.0.1:PORT. FIXTURE is a file
under .github/dogfood. A request that is not the next exchange, by its method,
path and JSON body, is answered 599 and counted. GET /__done answers 200 once
every exchange was served and none was refused, and 500 otherwise.

No answer repeats what a request sent: why a request was refused, and what
/__done found, go to stderr, which the job prints.

Two halves: Replay decides from what it is handed, and is what
test_fixture_server.py tests; the server below it reads the sockets.
"""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

FIXTURES = Path(__file__).resolve().parent.parent / "dogfood"

REFUSED = 599


class Replay:
    """The exchanges a run must make, in order."""

    def __init__(self, exchanges: list[dict]):
        self.exchanges = exchanges
        self.served = 0
        self.refusals: list[str] = []

    def answer(self, method: str, path: str, body: bytes) -> tuple[int, object]:
        """The answer to one request: the next exchange's, or a refusal."""
        sent = json.loads(body) if body.strip() else None
        if self.served >= len(self.exchanges):
            return self._refuse(f"{method} {path} came after every exchange was served")
        expected = self.exchanges[self.served]
        if (expected["method"], expected["path"]) != (method, path):
            return self._refuse(
                f"exchange {self.served + 1} is {expected['method']} {expected['path']}, not {method} {path}"
            )
        if expected["body"] != sent:
            return self._refuse(f"exchange {self.served + 1}, {method} {path}, sent another body")
        self.served += 1
        return expected["status"], expected["response"]

    def _refuse(self, why: str) -> tuple[int, object]:
        self.refusals.append(why)
        return REFUSED, {"refused": why}

    def done(self) -> tuple[int, str]:
        """Whether the run made every exchange and nothing else."""
        if self.refusals:
            return 500, "refused: " + "; ".join(self.refusals)
        if self.served != len(self.exchanges):
            return 500, f"{self.served} of {len(self.exchanges)} exchanges were served"
        return 200, f"all {self.served} exchanges were served"


def handler(replay: Replay) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def _serve(self) -> None:
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length)
            if self.command == "GET" and self.path == "/__done":
                status, said = replay.done()
                sys.stderr.write(f"{said}\n")
                payload = b""
            else:
                status, answered = replay.answer(self.command, self.path, body)
                if status == REFUSED:
                    sys.stderr.write(f"{replay.refusals[-1]}\n")
                    answered = {"refused": True}
                payload = b"" if answered is None else json.dumps(answered).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

        do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = _serve

        def log_message(self, format: str, *args: object) -> None:
            sys.stderr.write(f"{self.command} {self.path}\n")

    return Handler


def fixture_path(named: str) -> Path | None:
    """The fixture file `named` names, where it lies under FIXTURES."""
    path = Path(named).resolve()
    return path if path.is_file() and path.is_relative_to(FIXTURES) else None


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    fixture = fixture_path(argv[1])
    if fixture is None:
        print(f"{argv[1]} is not a fixture under {FIXTURES}", file=sys.stderr)
        return 2
    with fixture.open(encoding="utf-8") as file:
        replay = Replay(json.load(file)["exchanges"])
    ThreadingHTTPServer(("127.0.0.1", int(argv[2])), handler(replay)).serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
