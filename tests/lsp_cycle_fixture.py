"""Controlled model decisions; only the production executor touches source."""
import json


class Cycle:
    def __init__(self, token, receipt_prefix=""):
        self.token = token
        self.receipt_prefix = receipt_prefix
        self.step = 0
        self.calls = [
            ("lsp", {"operation": "status", "language": "rust"}),
            *[("lsp", {"operation": operation, "path": "main.rs", "line": 2, "character": 12})
              for operation in ("definition", "references", "hover")],
            ("lsp", {"operation": "diagnostics", "path": "main.rs"}),
            ("edit", {"path": "main.rs", "old_text": "BROKEN", "new_text": "FIXED"}),
            ("lsp", {"operation": "diagnostics", "path": "main.rs"}),
            ("lsp", {"operation": "hover", "path": "main.rs", "line": 0, "character": 5}),
            ("write", {"path": ".fixture-mode", "content": "empty"}),
            ("lsp", {"operation": "references", "path": "main.rs", "line": 2, "character": 12}),
            ("lsp", {"operation": "hover", "path": "main.rs", "line": 0, "character": 5}),
            ("write", {"path": ".fixture-mode", "content": "unsupported"}),
            ("lsp", {"operation": "hover", "path": "main.rs", "line": 2, "character": 12}),
            ("write", {"path": ".fixture-mode", "content": "oversized"}),
            ("lsp", {"operation": "references", "path": "main.rs", "line": 2, "character": 12}),
            ("write", {"path": ".fixture-mode", "content": "normal"}),
            ("lsp", {"operation": "status", "language": "rust"}),
        ]

    def next(self, result=None):
        if result is not None:
            assert result["call_id"] == f"{self.receipt_prefix}{self.token}-lsp-{self.step}"
            assert result["tool"] == self.calls[self.step - 1][0]
            if self.step in (8, 11):
                assert not result["success"], "a split UTF-16 surrogate must be rejected"
                assert "surrogate" in result["output"], result
            elif self.step == 13:
                assert not result["success"] and "support" in result["output"], result
            elif self.step == 15:
                assert not result["success"], "oversized server output must not silently succeed"
                assert any(word in result["output"] for word in ("bound", "MiB", "large", "1048576 bytes")), result
            elif self.step in (9, 12, 14, 16):
                assert result["success"], result
            elif self.step == 6:
                assert result["success"], result
                assert "Language diagnostics:" in result["output"]
                value = json.loads(result["output"].split("Language diagnostics: ", 1)[1])
                assert value["data"]["state"] == "current"
                assert value["data"]["items"] == []
            else:
                assert result["success"], result
                value = json.loads(result["output"])
                if self.step in (1, 17):
                    assert value["state"] == "ready"
                    assert value["capabilities"]["definition"]
                else:
                    assert value["source"]["path"] == "main.rs"
                    assert len(value["source"]["sha256"]) == 64
                    assert value["truncated"] is False
                    if self.step in (5, 7):
                        assert value["data"]["state"] == "current"
                        assert bool(value["data"]["items"]) == (self.step == 5)
                        assert value["data"]["verification"] == "not run"
                    elif self.step == 10:
                        assert value["data"]["state"] == "available"
                        assert value["data"]["result"] == []
                    else:
                        assert value["data"]["state"] == "available"
                        assert value["data"]["result"] is not None
        if self.step == len(self.calls):
            return None
        name, arguments = self.calls[self.step]
        self.step += 1
        return {"id": f"{self.token}-lsp-{self.step}", "name": name, "arguments": arguments}
