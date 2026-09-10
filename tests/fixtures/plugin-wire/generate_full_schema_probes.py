#!/usr/bin/env python3
"""Regenerate frozen schema probes with independent python-jsonschema 4.26.0.

Run from the repository root in an isolated environment with that pinned package.
No schema retrieval is performed: all references must be local JSON pointers.
Production tests consume the JSON output, never execute this generator.
"""
import copy
import json
from collections import defaultdict
from pathlib import Path
from jsonschema.validators import validator_for

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PROFILE = json.loads((ROOT / "docs/spec/compatibility/plugin-profile-v1.json").read_text())
FIXTURES = json.loads((HERE / "full-schemas.json").read_text())
BAD = [None, False, 17, "invalid-probe", {}, [], {"unexpected_probe": True}, [False]]


def esc(value):
    return str(value).replace("~", "~0").replace("/", "~1")


def parts(pointer):
    return [s.replace("~1", "/").replace("~0", "~") for s in pointer.split("/")[1:]]


def get(value, pointer):
    for key in parts(pointer):
        value = value[int(key)] if isinstance(value, list) else value[key]
    return value


def patch(value, change):
    value = copy.deepcopy(value)
    if change["op"] == "bound":
        change = dict(change, op="set", value="x" * (1024 * 1024 + 1))
    if not change["path"]:
        assert change["op"] == "set"
        return copy.deepcopy(change["value"])
    parent_path, key = change["path"].rsplit("/", 1)
    key = parts("/" + key)[0]
    parent = get(value, parent_path)
    if isinstance(parent, list):
        key = int(key)
    if change["op"] == "append":
        parent[key].append(copy.deepcopy(change["value"]))
    elif change["op"] == "remove":
        del parent[key]
    elif change["op"] == "rename":
        parent[change["value"]] = parent.pop(key)
    else:
        parent[key] = copy.deepcopy(change["value"])
    return value


def set_at(location, value):
    return {"op": "rename" if location[1] else "set", "path": location[0], "value": value}


def schema_key(entry):
    return ({"portable": entry["name"]} if "name" in entry else
            {"codex": entry["path"], "definition": entry.get("definition")})


def build(entry):
    schema = entry["schema"]
    def require_local(value):
        if isinstance(value, dict):
            for name, child in value.items():
                if name in ("$ref", "$dynamicRef", "$recursiveRef"):
                    assert child.startswith("#/"), "external reference is forbidden"
                    get(schema, child[1:])
                require_local(child)
        elif isinstance(value, list):
            for child in value:
                require_local(child)
    require_local(schema)
    key = schema_key(entry)
    cls = validator_for(schema)
    cls.check_schema(schema)
    validator = cls(schema)
    contexts = defaultdict(list)
    nodes = {}

    def local(node, value):
        return validator.evolve(schema=node).is_valid(value)

    def locate(node, path, value, location, fixture, inverted=False):
        nodes[path] = node
        contexts[path].append((fixture, location, inverted))
        if not isinstance(node, dict):
            return
        if "$ref" in node:
            reference = node["$ref"]
            assert reference.startswith("#/")
            locate(get(schema, reference[1:]), reference[1:], value, location, fixture, inverted)
        for keyword in ("oneOf", "allOf"):
            for i, branch in enumerate(node.get(keyword, [])):
                if keyword == "allOf" or local(branch, value):
                    locate(branch, f"{path}/{keyword}/{i}", value, location, fixture, inverted)
        if isinstance(value, dict):
            for name, child in node.get("properties", {}).items():
                if name in value:
                    locate(child, path + "/properties/" + esc(name), value[name],
                           (location[0] + "/" + esc(name), False), fixture, inverted)
            if isinstance(node.get("additionalProperties"), dict):
                for name, child in value.items():
                    if name not in node.get("properties", {}):
                        locate(node["additionalProperties"], path + "/additionalProperties", child,
                               (location[0] + "/" + esc(name), False), fixture, inverted)
            if "propertyNames" in node:
                for name in value:
                    locate(node["propertyNames"], path + "/propertyNames", name,
                           (location[0] + "/" + esc(name), True), fixture, inverted)
        if isinstance(value, list) and "items" in node:
            for i, child in enumerate(value):
                locate(node["items"], path + "/items", child,
                       (location[0] + "/" + str(i), False), fixture, inverted)
        if "not" in node:
            locate(node["not"], path + "/not", value, location, fixture, not inverted)

    for i, fixture in enumerate(FIXTURES):
        if fixture["key"] == key:
            assert validator.is_valid(fixture["value"]), (key, i)
            locate(schema, "", fixture["value"], ("", False), i)

    # Traverse schema structure independently of instances to require every reachable node.
    required_nodes = set()
    def reachable(node, path):
        if path in required_nodes:
            return
        required_nodes.add(path)
        if not isinstance(node, dict):
            return
        if "$ref" in node:
            ref = node["$ref"]
            assert ref.startswith("#/")
            reachable(get(schema, ref[1:]), ref[1:])
        for name, child in node.get("properties", {}).items():
            reachable(child, path + "/properties/" + esc(name))
        for keyword in ("oneOf", "allOf"):
            for i, child in enumerate(node.get(keyword, [])):
                reachable(child, f"{path}/{keyword}/{i}")
        for keyword in ("items", "propertyNames", "not"):
            if keyword in node:
                reachable(node[keyword], path + "/" + keyword)
        if isinstance(node.get("additionalProperties"), dict):
            reachable(node["additionalProperties"], path + "/additionalProperties")
    reachable(schema, "")
    assert required_nodes <= nodes.keys(), (key, "unreached schema nodes", required_nodes - nodes.keys())
    probes = []

    def make(identity, path, keyword, index=None):
        node = nodes[path]
        choices = contexts[path]
        deletion_path = path + "/" + keyword if keyword else path
        mutation = {"op": "remove", "path": deletion_path}
        if keyword == "required":
            mutation["path"] += "/" + str(index)
        elif keyword == "branch":
            mutation = {"op": "remove", "path": path}
            parent = path.rsplit("/", 1)[0]
            if len(get(schema, parent)) == 1:
                mutation["path"] = parent
        elif keyword in ("field", "unconstrained"):
            mutation = {"op": "set", "path": path, "value": False}
        elif keyword == "type-alternative":
            mutation = {"op": "remove", "path": path + "/type/" + str(index)}
        elif keyword == "alternative":
            mutation = {"op": "remove", "path": path + "/enum/" + str(index)}
            if len(node["enum"]) == 1:
                mutation = {"op": "set", "path": path + "/enum", "value": ["conflicting-enum-probe"]}
        elif keyword == "format":
            mutation = {"op": "remove", "path": path + "/format"}

        if keyword == "oneOf-exclusivity":
            for fixture, location, _ in choices:
                baseline = FIXTURES[fixture]["value"]
                actual = get(baseline, location[0])
                for branch in node["oneOf"]:
                    if local(branch, actual):
                        duplicate = {"op": "append", "path": path + "/oneOf", "value": branch}
                        altered = patch(schema, duplicate)
                        cls.check_schema(altered)
                        assert not cls(altered).is_valid(baseline)
                        return {"id": identity, "fixture": fixture, "mutation": duplicate,
                                "effect": "positive-rejected"}
            raise AssertionError("no matching oneOf alternative")
        mutated = patch(schema, mutation)
        cls.check_schema(mutated)
        changed = cls(mutated)
        fallback = None
        for fixture, location, inverted in choices:
            baseline = FIXTURES[fixture]["value"]
            actual = parts(location[0])[-1] if location[1] else get(baseline, location[0])
            valid_patch = None
            if keyword in ("minimum", "minLength", "maxLength"):
                boundary = node[keyword] if keyword == "minimum" else "x" * node[keyword]
                valid_patch = set_at(location, boundary)
                baseline = patch(baseline, valid_patch)
                assert validator.is_valid(baseline), (key, identity, "exact boundary must be representable")
            if keyword == "type-alternative":
                examples = {"string": "fixture", "null": None, "number": 1, "integer": 1, "object": {}, "array": [], "boolean": False}
                candidate = set_at(location, examples[node["type"][index]])
                positive = patch(baseline, candidate)
                if validator.is_valid(positive) and not changed.is_valid(positive):
                    return {"id": identity, "fixture": fixture, "valid": candidate,
                            "mutation": mutation, "effect": "positive-rejected"}
                continue
            if keyword == "alternative":
                candidate = set_at(location, node["enum"][index])
                candidate_value = patch(baseline, candidate)
                if inverted:
                    if validator.is_valid(candidate_value):
                        continue
                    invalid = candidate
                else:
                    if not validator.is_valid(candidate_value):
                        continue
                    baseline = candidate_value
                    valid_patch = candidate
                    invalid = None
                record = {"id": identity, "fixture": fixture, "mutation": mutation,
                          "effect": "negative-accepted" if inverted and changed.is_valid(candidate_value) else "positive-rejected"}
                if valid_patch:
                    record["valid"] = valid_patch
                if invalid:
                    record["invalid"] = invalid
                if record["effect"] == "positive-rejected" and changed.is_valid(baseline):
                    continue
                return record
            if keyword in ("field", "unconstrained", "branch"):
                if not changed.is_valid(baseline):
                    record = {"id": identity, "fixture": fixture, "mutation": mutation, "effect": "positive-rejected"}
                    if keyword == "unconstrained":
                        assert not location[1]
                        record["invalid"] = {"op": "bound", "path": location[0]}
                        record["limit_only"] = True
                    return record
                # allOf branch removal loosens; find a negative below.
            if keyword == "format":
                assert changed.is_valid(baseline)
                return {"id": identity, "fixture": fixture, "mutation": mutation, "effect": "annotation-no-effect"}
            candidates = list(BAD)
            if keyword == "minimum": candidates.insert(0, node[keyword] - 1)
            if keyword == "minLength": candidates.insert(0, "x" * max(0, node[keyword] - 1))
            if keyword == "maxLength": candidates.insert(0, "x" * (node[keyword] + 1))
            if keyword == "pattern": candidates = ["BAD_NAME", "/absolute", "bad--name", "bad..name"] + candidates
            if keyword == "not" and "enum" in node["not"]: candidates = node["not"]["enum"] + candidates
            if keyword == "enum" and inverted: candidates = node["enum"]
            if keyword == "propertyNames": candidates = [{"PLUGIN_ROOT": "value"}, {"PLUGIN_DATA": "value"}]
            if keyword == "additionalProperties" and node[keyword] is False:
                candidates = [dict(actual, unexpected_probe=True)]
            if keyword == "additionalProperties" and isinstance(node[keyword], dict):
                candidates = [dict(actual, unexpected_probe=v) for v in BAD]
            if keyword == "additionalProperties" and node[keyword] is True:
                candidate = set_at(location, dict(actual, unexpected_probe={"bounded": "data"}))
                assert validator.is_valid(patch(baseline, candidate))
                conflict = {"op": "set", "path": deletion_path, "value": False}
                assert not cls(patch(schema, conflict)).is_valid(patch(baseline, candidate))
                return {"id": identity, "fixture": fixture, "valid": candidate, "mutation": conflict,
                        "effect": "positive-rejected", "redundancy": "true allows any additional JSON; deleting it has the same meaning"}
            if keyword == "items": candidates = [[v] for v in BAD]
            if keyword == "required":
                candidates = [dict(actual)]
                del candidates[0][node[keyword][index]]
            for bad in candidates:
                # A negative must violate the selected keyword, not merely another sibling.
                if keyword in ("type", "minimum", "minLength", "maxLength", "pattern", "enum", "const"):
                    satisfies = cls({keyword: node[keyword]}).is_valid(bad)
                    if satisfies != inverted:
                        continue
                invalid = set_at(location, bad)
                negative = patch(baseline, invalid)
                if validator.is_valid(negative):
                    continue
                record = {"id": identity, "fixture": fixture, "invalid": invalid, "mutation": mutation,
                          "effect": "negative-accepted"}
                if valid_patch:
                    record["valid"] = valid_patch
                if changed.is_valid(negative):
                    return record
                if not changed.is_valid(baseline):
                    record["effect"] = "positive-rejected"
                    record["redundancy"] = "constraint lies under not; deletion tightens the accepted shape"
                    return record
                if fallback is None:
                    fallback = record
        if fallback is not None:
            # Other constraints can imply this one (e.g. enum implies string).
            # Keep the frozen negative and prove the occurrence with a conflicting constraint.
            fixture = fallback["fixture"]
            baseline = FIXTURES[fixture]["value"]
            if "valid" in fallback:
                baseline = patch(baseline, fallback["valid"])
            if keyword == "type": value = "null" if get(baseline, choices[0][1][0]) is not None else "string"
            elif keyword == "minLength": value = 1000000
            elif keyword == "minimum": value = 1000000
            else: raise AssertionError((key, identity, "needs explicit redundancy explanation"))
            conflict = {"op": "set", "path": deletion_path, "value": value}
            assert not cls(patch(schema, conflict)).is_valid(baseline), (key, identity, "conflict did not fail")
            fallback.update(mutation=conflict, effect="positive-rejected",
                            redundancy="another sibling constraint implies this constraint; a conflicting value proves evaluation")
            return fallback
        raise AssertionError((key, identity, "no frozen witness"))

    for path in sorted(nodes):
        node = nodes[path]
        if "/properties/" in path and path.rsplit("/", 2)[1] == "properties":
            probes.append(make(path + "::field", path, "field"))
        if not isinstance(node, dict) or not any(k in node for k in ("type", "$ref", "enum", "const", "oneOf", "allOf", "not", "properties", "items")):
            probes.append(make(path + "::unconstrained", path, "unconstrained"))
        if not isinstance(node, dict):
            continue
        for keyword in ("$ref", "type", "const", "enum", "minimum", "minLength", "maxLength", "pattern", "additionalProperties", "items", "oneOf", "allOf", "propertyNames", "not", "format"):
            if keyword in node:
                probes.append(make(path + "::" + keyword, path, keyword))
        if "oneOf" in node:
            probes.append(make(path + "::oneOf-exclusivity", path, "oneOf-exclusivity"))
        for i in range(len(node.get("required", []))):
            probes.append(make(path + f"/required/{i}::required", path, "required", i))
        if isinstance(node.get("type"), list):
            for i in range(len(node["type"])):
                probes.append(make(path + f"/type/{i}::type-alternative", path, "type-alternative", i))
        for i in range(len(node.get("enum", []))):
            probes.append(make(path + f"/enum/{i}::alternative", path, "alternative", i))
        for keyword in ("oneOf", "allOf"):
            for i in range(len(node.get(keyword, []))):
                child = f"{path}/{keyword}/{i}"
                probes.append(make(child + "::branch", child, "branch"))
    return {"key": key, "probes": probes}


if __name__ == "__main__":
    result = [build(e) for e in PROFILE["codex_wire"] + PROFILE["source_revisions"]["portable"]]
    (HERE / "full-schema-probes.json").write_text(json.dumps(result, indent=2) + "\n")
    print(sum(len(e["probes"]) for e in result), "explicit schema probes")
