"""Model discovery shared by controlled provider fixtures; no live service calls."""
import http.server
import json
from urllib.parse import unquote, urlsplit


class ModelMetadataHandler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        path = urlsplit(self.path).path
        if "/models/" not in path:
            self.send_error(404)
            return
        model = unquote(path.split("/models/", 1)[1])
        data = json.dumps({"id": model, "max_tokens": getattr(self.server, "model_max_tokens", 128000)}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
