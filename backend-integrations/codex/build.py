#!/usr/bin/env python3
"""Build the pinned managed Codex source and retain an artifact-specific receipt."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tomllib

PACKAGE = Path(__file__).resolve().parent
REQUIREMENT_ENV = "CODEX_DEMONCODER_COMPACTION_RELAY"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_records(root):
    records = {}
    for directory, subdirectories, filenames in os.walk(root):
        relative = Path(directory).relative_to(root)
        for name in subdirectories:
            if name == ".git" or (Path(directory) / name).is_symlink():
                raise RuntimeError(f"unexpected source directory: {relative / name}")
        subdirectories[:] = [
            name
            for name in subdirectories
            if relative / name != Path("codex-rs/target")
        ]
        for name in filenames:
            path = Path(directory) / name
            relative_path = path.relative_to(root).as_posix()
            if relative_path == "managed-build-receipt.json":
                continue
            if path.is_symlink() and root not in path.resolve(strict=True).parents:
                raise RuntimeError(
                    f"source symlink escapes the release: {relative_path}"
                )
            records[relative_path] = digest(path)
    return records


def tree_digest(records):
    hasher = hashlib.sha256()
    for name, value in sorted(records.items()):
        hasher.update(f"{name}\0{value}\n".encode())
    return hasher.hexdigest()


def verify_external_lock(source, prepared):
    original = tomllib.loads((source / "codex-rs/Cargo.lock").read_text())
    patched = tomllib.loads((prepared / "codex-rs/Cargo.lock").read_text())

    def external(lock):
        return [entry for entry in lock["package"] if "source" in entry]

    if external(original) != external(patched):
        raise RuntimeError("patch changes the pinned external dependency records")


def patch_plan(source, provenance, ordinary=None):
    records = tree_records(source)
    if tree_digest(records) != provenance["source_tree_sha256"]:
        raise RuntimeError("source tree does not match the retained release")
    expected = dict(records)
    stages = [(PACKAGE / "managed-compaction.patch", provenance)]
    if ordinary is not None:
        stages.append((PACKAGE / "managed-ordinary.patch", ordinary))
    plan = []
    for path, declaration in stages:
        if digest(path) != declaration["patch_sha256"]:
            raise RuntimeError(f"patch digest does not match provenance: {path.name}")
        if declaration is ordinary and (
            ordinary["base_patch_sha256"] != provenance["patch_sha256"]
            or ordinary["base_prepared_tree_sha256"] != tree_digest(expected)
        ):
            raise RuntimeError("ordinary patch base differs from the compaction stage")
        for name, hashes in declaration["changed_files"].items():
            if expected.get(name) != hashes["original_sha256"]:
                raise RuntimeError(f"source precondition failed: {name}")
            expected[name] = hashes["patched_sha256"]
        plan.append((path, dict(expected)))
    return plan


def prepare(source, build_root, provenance, reuse, ordinary=None):
    if (
        source == build_root
        or source in build_root.parents
        or build_root in source.parents
    ):
        raise RuntimeError("source and build root must not overlap")
    plan = patch_plan(source, provenance, ordinary)
    if not reuse:
        shutil.copytree(source, build_root)
        for path, expected in plan:
            subprocess.run(
                ["patch", "--batch", "--fuzz=0", "-p1", "-i", str(path)],
                cwd=build_root,
                check=True,
            )
            if tree_records(build_root) != expected:
                raise RuntimeError(
                    f"intermediate source differs from the exact expected patched tree: {path.name}"
                )
            verify_external_lock(source, build_root)
    expected = plan[-1][1]
    if tree_records(build_root) != expected:
        raise RuntimeError(
            "prepared source differs from the exact expected patched tree"
        )
    verify_external_lock(source, build_root)
    return tree_digest(expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--build-root", type=Path, required=True)
    parser.add_argument(
        "--reuse", action="store_true", help="verify and reuse an already patched tree"
    )
    parser.add_argument(
        "--prepare-only",
        action="store_true",
        help="verify/apply source without compiling",
    )
    parser.add_argument("--profile", choices=["dev", "release"], default="dev")
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    source = args.source.resolve(strict=True)
    build_root = args.build_root.resolve()
    provenance = json.loads((PACKAGE / "source-provenance.json").read_text())
    ordinary = json.loads((PACKAGE / "ordinary-provenance.json").read_text())
    patched_tree = prepare(source, build_root, provenance, args.reuse, ordinary)
    if args.prepare_only:
        print(
            json.dumps(
                {"prepared_tree_sha256": patched_tree, "build_root": str(build_root)}
            )
        )
        return
    environment = os.environ.copy()
    for key in list(environment):
        if key.startswith("CARGO_PROFILE_") or key in {
            REQUIREMENT_ENV,
            "CODEX_DEMONCODER_ORDINARY_RELAY",
            "CODEX_DEMONCODER_MODEL_HOOK",
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "RUSTC",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_TARGET",
            "CARGO_BUILD_RUSTFLAGS",
        }:
            environment.pop(key)
    environment.update(
        {
            "RUSTUP_TOOLCHAIN": provenance["rust_toolchain"],
            "CARGO_BUILD_JOBS": str(args.jobs),
            "CARGO_PROFILE_DEV_DEBUG": "0",
            "CARGO_PROFILE_TEST_DEBUG": "0",
            "CARGO_PROFILE_RELEASE_DEBUG": "0",
            "CARGO_TARGET_DIR": str(build_root / "codex-rs/target"),
        }
    )
    working_directory = build_root / "codex-rs"
    subprocess.run(
        ["just", "test", "--locked", "-p", "codex-hooks"],
        cwd=working_directory,
        env=environment,
        check=True,
    )
    command = [
        "cargo",
        "build",
        "--locked",
        "--profile",
        args.profile,
        "-p",
        "codex-cli",
        "--bin",
        "codex",
    ]
    subprocess.run(command, cwd=working_directory, env=environment, check=True)
    if tree_records(build_root) != patch_plan(source, provenance, ordinary)[-1][1]:
        raise RuntimeError("source changed while building; no receipt recorded")
    profile_directory = "debug" if args.profile == "dev" else "release"
    binary = (
        working_directory
        / "target"
        / profile_directory
        / ("codex.exe" if os.name == "nt" else "codex")
    )

    def capture(command):
        return subprocess.check_output(
            command, env=environment, text=True, timeout=15
        ).strip()

    capability = json.loads(
        capture([str(binary), "--demoncoder-compaction-capability"])
    )
    expected = {
        "protocol": "demoncoder-compaction-v1",
        "source_version": "0.153.4",
        "patch_version": 1,
    }
    if capability != expected:
        raise RuntimeError(
            "built artifact does not expose the expected managed capability"
        )
    model_hook_capability = json.loads(
        capture([str(binary), "--demoncoder-model-hook-capability"])
    )
    if model_hook_capability != {
        "protocol": "demoncoder-model-hook-v1",
        "source_version": "0.153.4",
    }:
        raise RuntimeError("built artifact lacks model-hook instruction isolation")
    ordinary_capability = json.loads(
        capture([str(binary), "--demoncoder-ordinary-capability"])
    )
    if ordinary_capability != {
        "protocol": "demoncoder-ordinary-v1",
        "source_version": "0.153.4",
        "patch_version": 1,
    }:
        raise RuntimeError("built artifact lacks the private ordinary hook boundary")
    receipt = {
        "schema_version": 2,
        "source_tree_sha256": provenance["source_tree_sha256"],
        "patch_sha256": provenance["patch_sha256"],
        "ordinary_patch_sha256": ordinary["patch_sha256"],
        "compaction_tree_sha256": ordinary["base_prepared_tree_sha256"],
        "prepared_tree_sha256": patched_tree,
        "binary": str(binary),
        "binary_sha256": digest(binary),
        "binary_bytes": binary.stat().st_size,
        "profile": args.profile,
        "debug_information": False,
        "build_command": command,
        "rustc": capture(["rustc", "--version", "--verbose"]),
        "cargo": capture(["cargo", "--version"]),
        "platform": platform.platform(),
        "version": capture([str(binary), "--version"]),
        "capability": capability,
        "model_hook_capability": model_hook_capability,
        "ordinary_capability": ordinary_capability,
        "checks": ["just test --locked -p codex-hooks"],
        "qualification": "not established by this build receipt; run backend and host-adapter qualification",
    }
    output = build_root / "managed-build-receipt.json"
    temporary = output.with_suffix(".tmp")
    temporary.write_text(json.dumps(receipt, indent=2) + "\n")
    temporary.replace(output)
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
