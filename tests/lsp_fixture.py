#!/usr/bin/python3
"""Controlled framed LSP peer. Modes are chosen only by the integration tests."""
import json
import pathlib
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid


control_file = pathlib.Path(".fixture-control")
CONTROL = control_file.read_text().strip() if control_file.exists() else None
INSTANCE = uuid.uuid4().hex


def control_read(name):
    if CONTROL is None:
        return pathlib.Path(name).read_text()
    with urllib.request.urlopen(CONTROL + name, timeout=3) as response:
        return response.read().decode()


def control_exists(name):
    try:
        control_read(name)
        return True
    except (FileNotFoundError, urllib.error.HTTPError):
        return False


def control_write(name, value="", append=False):
    if CONTROL is None:
        with pathlib.Path(name).open("a" if append else "w") as output:
            output.write(value)
        return
    request = urllib.request.Request(CONTROL + name, data=value.encode(),
                                     headers={"X-Append": str(append).lower()}, method="POST")
    with urllib.request.urlopen(request, timeout=3):
        pass


def start_descendant():
    if CONTROL is None:
        raise RuntimeError("descendant test requires an independent observer")
    subprocess.Popen(["/usr/bin/python3", "-c",
        "import time,urllib.request; time.sleep(1); "
        "urllib.request.urlopen(urllib.request.Request(" + repr(CONTROL + "descendant-escaped") +
        ",data=b'escaped',method='POST'),timeout=3).close()"])


def probe_reads(paths):
    effects = {}
    for name, path in paths.items():
        try:
            pathlib.Path(path).read_bytes()
            effects[name] = True
        except OSError:
            effects[name] = False
    return effects


def read():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            raise EOFError
        if line == b"\r\n":
            break
        key, value = line.decode().split(":", 1)
        headers[key.lower()] = value.strip()
    return json.loads(sys.stdin.buffer.read(int(headers["content-length"])))


def send(value):
    body = json.dumps({"jsonrpc": "2.0", **value}, ensure_ascii=False).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    sys.stdout.buffer.flush()


def mode():
    return control_read(".fixture-mode").strip()


def diagnostic(uri, version, text):
    items = [] if "BROKEN" not in text else [{
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
        "severity": 1, "message": "persistent fixture error π", "source": "fixture",
    }]
    params = {"uri": uri, "diagnostics": items}
    if version is not None:
        params["version"] = version
    send({"method": "textDocument/publishDiagnostics", "params": params})


documents = {}
content_modified_replies = 0
while True:
    try:
        request = read()
    except EOFError:
        break
    method = request.get("method")
    params = request.get("params", {})
    current_mode = mode()
    if method == "initialize":
        if current_mode == "owned-child":
            start_descendant()
            control_write("request-started")
        if current_mode == "init-failure":
            send({"id": request["id"], "error": {"code": -32603, "message": "fixture initialization failed"}})
            continue
        capabilities = {"positionEncoding": "utf-16", "textDocumentSync": 1,
                        "definitionProvider": True, "referencesProvider": True,
                        "hoverProvider": current_mode != "unsupported"}
        if current_mode == "no-sync":
            capabilities["textDocumentSync"] = 0
        elif current_mode == "no-open-sync":
            capabilities["textDocumentSync"] = {"openClose": False, "change": 2}
        elif current_mode == "save-required":
            capabilities["textDocumentSync"] = {"openClose": True, "change": 1, "save": {"includeText": True}}
        if current_mode in ("pull", "pull-unchanged", "pull-retrigger", "pull-retrigger-forever", "pull-mixed", "pull-mixed-unversioned"):
            capabilities["diagnosticProvider"] = {"interFileDependencies": False, "workspaceDiagnostics": False}
        send({"id": request["id"], "result": {"capabilities": capabilities}})
        if current_mode in ("health-loading", "health-error", "health-warning"):
            send({"method": "experimental/serverStatus", "params": {
                "health": {"health-loading":"ok", "health-error":"error", "health-warning":"warning"}[current_mode],
                "quiescent": current_mode != "health-loading", "message": "fixture project status",
            }})
    elif method == "initialized" and current_mode == "background-probe":
        control_write("request-started")
        while not control_exists("release-probe"):
            time.sleep(0.01)
        checks = json.loads(control_read(".fixture-canaries"))
        control_write("adversarial-results", json.dumps(probe_reads(checks["read_paths"])))
    elif method == "initialized" and current_mode == "exit-after-init":
        while not control_exists("release-exit"):
            time.sleep(0.01)
        control_write("server-exiting")
        sys.exit(0)
    elif method in ("textDocument/didOpen", "textDocument/didChange"):
        document = params["textDocument"]
        text = document.get("text") if method.endswith("didOpen") else params["contentChanges"][0]["text"]
        uri, version = document["uri"], document["version"]
        documents[uri] = text
        if current_mode == "save-required":
            control_write("synchronized-version", str(version))
            continue
        if current_mode == "exit":
            sys.exit(0)
        if current_mode in ("pending", "pull", "pull-unchanged"):
            continue
        if current_mode == "late-first":
            time.sleep(3)
        if current_mode in ("mixed-version-empty-first", "mixed-version-error-first"):
            error_first = current_mode == "mixed-version-error-first"
            diagnostic(uri, version, "BROKEN" if error_first else "")
            while not control_exists("release-unversioned"):
                time.sleep(0.01)
            diagnostic(uri, None, "" if error_first else "BROKEN")
            continue
        if current_mode == "reorder":
            diagnostic(uri, version - 1, "BROKEN obsolete")
            diagnostic(uri, version + 1, "BROKEN future")
        if current_mode == "late-current":
            diagnostic(uri, version, "")
            time.sleep(0.1)
        diagnostic(uri, None if current_mode in ("unversioned", "pull-mixed-unversioned") else version, text)
    elif method == "textDocument/diagnostic":
        if current_mode == "pull-retrigger-forever" or (current_mode == "pull-retrigger" and content_modified_replies < 2):
            content_modified_replies += 1
            send({"id": request["id"], "error": {"code": -32802, "data": {"retriggerRequest": True}, "message": "fixture indexing cancelled this diagnostic request"}})
            continue
        if current_mode in ("pull-mixed", "pull-mixed-unversioned"):
            result = {"kind": "full", "items": []}
        elif current_mode == "pull-unchanged":
            result = {"kind": "unchanged", "resultId": "untrusted-previous-result"}
        else:
            result = {"kind": "full", "items": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "severity": 1, "message": "pulled fixture error"}]}
        send({"id": request["id"], "result": result})
    elif method == "textDocument/didSave":
        uri = params["textDocument"]["uri"]
        if current_mode != "save-required" or params.get("text") != documents[uri]:
            sys.exit("didSave did not contain the current synchronized text")
        control_write("save-receipts", json.dumps({"uri": uri, "text": params["text"]}) + "\n", append=True)
        diagnostic(uri, int(control_read("synchronized-version")), params["text"])
    elif method in ("textDocument/hover", "textDocument/definition", "textDocument/references"):
        uri = params["textDocument"]["uri"]
        if current_mode == "content-modified-forever" or (current_mode == "content-modified" and content_modified_replies < 2):
            content_modified_replies += 1
            send({"id": request["id"], "error": {"code": -32801, "message": "fixture graph changed during indexing"}})
            continue
        if current_mode == "notification-flood":
            for _ in range(1100):
                send({"method": "window/logMessage", "params": {"type": 3, "message": "bounded flood fixture"}})
        if current_mode == "unicode" and params["position"] != {"line": 0, "character": 3}:
            send({"id": request["id"], "error": {"code": -32602, "message": "wrong UTF-16 position"}})
            continue
        if current_mode == "stall":
            start_descendant()
            control_write("request-started")
            time.sleep(60)
        if current_mode == "external-change":
            control_write("request-started")
            while not control_exists("release-request"):
                time.sleep(0.01)
        if current_mode == "adversarial":
            checks = json.loads(control_read(".fixture-canaries"))
            effects = {}
            if "read_paths" in checks:
                effects["reads"] = probe_reads(checks["read_paths"])
            try:
                pathlib.Path(checks["protected"]).read_text()
                effects["protected_read"] = True
            except OSError:
                effects["protected_read"] = False
            try:
                pathlib.Path(checks["outside"]).write_text("ESCAPED")
                effects["outside_write"] = True
            except OSError:
                effects["outside_write"] = False
            for request_id, command, arguments in [
                ("edit", "workspace/applyEdit", {"edit": {"changes": {uri: [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "newText": "ESCAPED"}]}}}),
                ("command", "workspace/executeCommand", {"command": "touch", "arguments": ["unsolicited-command"]}),
                ("folders", "workspace/workspaceFolders", {}),
            ]:
                send({"id": request_id, "method": command, "params": arguments})
                effects[request_id] = read()
            control_write("adversarial-results", json.dumps(effects))
        location = {"uri": "file:///etc/passwd" if current_mode == "outside-uri" else uri,
                    "range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 9}}}
        if current_mode == "empty":
            result = None if method.endswith("hover") else []
        elif method.endswith("hover"):
            result = {"contents": {"kind": "plaintext", "value": "fixture π type"}, "range": location["range"], "fixture_instance": INSTANCE}
        else:
            result = [location]
        if current_mode == "oversized":
            result = [location] * 20000
        send({"id": request["id"], "result": result})
