#!/usr/bin/python3
"""Two declared source calls through the existing controlled Claude peer."""
import sys
sys.dont_write_bytecode = True
import copy
import http.server
import json
from pathlib import Path
import threading
import plugin_external_non_tool as fixture
root = Path(sys.argv[1])
adapter = sys.argv[2] if sys.argv[2] in ('claude', 'codex', 'openai-api', 'anthropic-api') else 'claude'
wrapper = len(sys.argv) > 3 and sys.argv[3] == 'wrapper'
wire_adapter = 'claude' if adapter in ('claude', 'anthropic-api') else 'codex'
original = fixture.model_response
def response(wire, sequence, text_case, case):
    rows = original(wire, sequence, text_case, case)
    if sequence == 1 and wrapper:
        arguments = {'calls': [{'tool':'write','arguments':{'path':'proof.txt','content':'first'}}, {'tool':'bash','arguments':{'command':'sleep 0.05'}}, {'tool':'edit','arguments':{'path':'proof.txt','old_text':'first','new_text':'settled host batch'}}]}
        for row in rows:
            if row['type'] == 'content_block_start': row['content_block']['name'] = 'mcp__demoncoder__tool_batch' if adapter == 'claude' else 'tool_batch'
            if row['type'] == 'content_block_delta': row['delta']['partial_json'] = json.dumps(arguments)
            item = row.get('item')
            if item and item['type'] == 'function_call': item.update(name='tool_batch', arguments=json.dumps(arguments))
            for item in row.get('response',{}).get('output',[]):
                if item['type'] == 'function_call': item.update(name='tool_batch', arguments=json.dumps(arguments))
    elif sequence == 1:
        second = copy.deepcopy(rows[1:4])
        for row in second:
            row['index'] = 1
            if row['type'] == 'content_block_start':
                row['content_block']['id'] = 'external_mixed_write_2'
        rows[4:4] = second
    return rows
fixture.model_response = response
requests, errors, lock = [], [], threading.Lock()
base = fixture.peer_handler(wire_adapter, requests, errors, lock, 'plain', 'mixed-batch')
class Peer(base):
    def do_POST(self):
        super().do_POST()
        with lock:
            (root / 'model-requests.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in requests))
            (root / 'peer-errors.json').write_text(json.dumps(errors))
if adapter == 'codex':
    from codex_https_fixture import create_server
    from installed_backends import fake_codex_auth
    home = root / 'codex-home'
    home.mkdir()
    fake_codex_auth(home)
    (home / 'config.toml').write_text('model="gpt-5.4"\ncli_auth_credentials_store="file"\n[features]\nenable_request_compression=false\n')
    server = create_server(root / 'tls', Peer)
else:
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Peer)
print(json.dumps({'port':server.server_port,'ca':str(server.ca_certificate) if adapter == 'codex' else None}) if wrapper else server.server_port, flush=True)
server.serve_forever()
