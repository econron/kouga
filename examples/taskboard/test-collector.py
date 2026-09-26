"""Tiny local OTLP/HTTP capture endpoint for Taskboard process smoke tests.

Usage: python3 test-collector.py PORT NEW_OUTPUT_DIRECTORY
The directory must not exist. Captured protobuf bodies are test data and may contain
application-provided attributes; never upload them to a public artifact store.
"""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import itertools
import sys


if len(sys.argv) != 3:
    raise SystemExit("usage: test-collector.py PORT NEW_OUTPUT_DIRECTORY")

destination = Path(sys.argv[2])
destination.mkdir(parents=True, exist_ok=False)
counter = itertools.count(1)


class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path not in ("/v1/traces", "/v1/metrics", "/v1/logs"):
            self.send_error(404)
            return
        length = int(self.headers.get("Content-Length", "0"))
        if length > 4_000_000:
            self.send_error(413)
            return
        body = self.rfile.read(length)
        path = destination / f"{next(counter):04d}-{self.path.rsplit('/', 1)[1]}.pb"
        path.write_bytes(body)
        print(f"{self.path} bytes={len(body)}", flush=True)
        self.send_response(200)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, _format, *_args):
        pass


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
