"""Source and dependency tampering must stop the managed build before compilation."""

import difflib
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import build


class SourcePreparationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        (self.source / "a.txt").write_text("before\n")
        (self.source / "codex-rs").mkdir()
        (self.source / "codex-rs/Cargo.lock").write_text(
            'version = 4\n[[package]]\nname = "dependency"\nversion = "1.0.0"\n'
            'source = "registry+https://example.invalid/index"\nchecksum = "fixture"\n'
        )
        self.package = self.root / "package"
        self.package.mkdir()
        self.target = self.root / "target"

    def provenance(self, changes):
        hunks = []
        changed_files = {}
        for name, content in changes.items():
            original = (self.source / name).read_text()
            hunks.extend(
                difflib.unified_diff(
                    original.splitlines(keepends=True),
                    content.splitlines(keepends=True),
                    fromfile="a/" + name,
                    tofile="b/" + name,
                )
            )
            changed_files[name] = {
                "original_sha256": build.digest(self.source / name),
                "patched_sha256": hashlib.sha256(content.encode()).hexdigest(),
            }
        path = self.package / "managed-compaction.patch"
        path.write_text("".join(hunks))
        return {
            "source_tree_sha256": build.tree_digest(build.tree_records(self.source)),
            "patch_sha256": build.digest(path),
            "changed_files": changed_files,
        }

    def test_replay_preserves_source_and_reuse_rejects_changed_candidate(self):
        provenance = self.provenance({"a.txt": "after\n"})
        with patch.object(build, "PACKAGE", self.package):
            build.prepare(self.source, self.target, provenance, reuse=False)
            self.assertEqual((self.source / "a.txt").read_text(), "before\n")
            self.assertEqual((self.target / "a.txt").read_text(), "after\n")
            (self.target / "a.txt").write_text("tampered\n")
            with self.assertRaisesRegex(RuntimeError, "exact expected patched tree"):
                build.prepare(self.source, self.target, provenance, reuse=True)

    def test_correctly_hashed_patch_cannot_change_external_dependencies(self):
        lock = (
            (self.source / "codex-rs/Cargo.lock")
            .read_text()
            .replace('version = "1.0.0"', 'version = "2.0.0"')
        )
        provenance = self.provenance({"codex-rs/Cargo.lock": lock})
        with patch.object(build, "PACKAGE", self.package):
            with self.assertRaisesRegex(RuntimeError, "pinned external dependency"):
                build.prepare(self.source, self.target, provenance, reuse=False)

    def test_source_and_patch_tampering_are_rejected_before_copy(self):
        provenance = self.provenance({"a.txt": "after\n"})
        with patch.object(build, "PACKAGE", self.package):
            (self.package / "managed-compaction.patch").write_text("tampered\n")
            with self.assertRaisesRegex(RuntimeError, "patch digest"):
                build.prepare(self.source, self.target, provenance, reuse=False)
            self.assertFalse(self.target.exists())
            (self.source / "a.txt").write_text("tampered\n")
            with self.assertRaisesRegex(RuntimeError, "source tree"):
                build.prepare(self.source, self.target, provenance, reuse=False)
            self.assertFalse(self.target.exists())

    def test_source_links_cannot_escape_or_hide_a_directory(self):
        (self.root / "outside").write_text("outside\n")
        (self.source / "link").symlink_to(self.root / "outside")
        with self.assertRaisesRegex(RuntimeError, "symlink escapes"):
            build.tree_records(self.source)
        (self.source / "link").unlink()
        (self.source / "link").symlink_to(self.package, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, "source directory"):
            build.tree_records(self.source)


if __name__ == "__main__":
    unittest.main()
