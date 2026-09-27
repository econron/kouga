#!/usr/bin/env python3
"""Generate a preliminary, target-specific CycloneDX inventory from Cargo.lock.

This lists Rust runtime dependencies only. It is not a legal approval or an OS
package/container SBOM; those must be reviewed separately before publication.
"""

import argparse
import datetime
import json
import pathlib
import re
import subprocess
import sys
import tomllib


TARGETS = {
    "http": "taskboard",
    "admin": "taskboard",
    "grpc": "taskboard-grpc",
    "worker": "taskboard-worker",
    "task-notice-worker": "taskboard-worker",
    "taskboard-channel": "taskboard-channel-runtime",
    "taskboard-storage-cleanup": "taskboard-storage-cleanup-runtime",
}


def cargo(args, cwd):
    return subprocess.check_output(["cargo", "+1.94.0", *args], cwd=cwd, text=True)


def main():
    if sys.version_info < (3, 11):
        raise SystemExit("Python 3.11+ is required for tomllib")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=pathlib.Path, help="packaged Taskboard directory")
    parser.add_argument("output", type=pathlib.Path, help="output directory")
    args = parser.parse_args()
    app = args.app.resolve()
    output = args.output.resolve()
    if not (app / "vendor/kouga-revision.txt").is_file():
        parser.error("first package a clean Kouga revision with package-source.sh")

    lock = tomllib.loads((app / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
    metadata = json.loads(cargo(["metadata", "--locked", "--offline", "--format-version", "1"], app))
    packages = {(p["name"], p["version"]): p for p in metadata["packages"]}
    revision = (app / "vendor/kouga-revision.txt").read_text().strip()
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        parser.error("invalid vendored Kouga revision")
    output.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).isoformat()

    for target, root in TARGETS.items():
        tree = cargo([
            "tree", "--locked", "--offline", "--target", "aarch64-unknown-linux-gnu",
            "-e", "normal", "-p", root, "--prefix", "none", "--format", "{p}",
        ], app)
        keys = set()
        for line in tree.splitlines():
            match = re.match(r"^([^ ]+) v([^ ]+)", line)
            if match:
                keys.add(match.groups())
        missing = keys - packages.keys()
        if missing:
            raise RuntimeError(f"Cargo tree packages missing from metadata: {sorted(missing)}")
        components = []
        for key in sorted(keys):
            package = packages[key]
            name, version = key
            registry = package["source"] is not None
            component = {
                "type": "library",
                "bom-ref": f"pkg:cargo/{name}@{version}" if registry else f"urn:source:{name}@{version}",
                "name": name,
                "version": version,
                "scope": "required",
                "properties": [{"name": "kouga:source", "value": package["source"] or "vendored/local"}],
            }
            if registry:
                component["purl"] = f"pkg:cargo/{name}@{version}"
            if package.get("license"):
                component["licenses"] = [{"expression": package["license"]}]
            if package.get("license_file"):
                component["properties"].append({"name": "kouga:license_file", "value": package["license_file"]})
            checksum = checksums.get(key)
            if checksum:
                component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
            components.append(component)
        bom = {
            "bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1,
            "metadata": {
                "timestamp": timestamp,
                "component": {"type": "application", "name": f"taskboard-{target}", "version": "0.1.0"},
                "properties": [
                    {"name": "kouga:revision", "value": revision},
                    {"name": "kouga:target-triple", "value": "aarch64-unknown-linux-gnu"},
                    {"name": "kouga:dependency-scope", "value": "Cargo normal dependencies; OS and vendored assets excluded"},
                ],
            },
            "components": components,
        }
        (output / f"{target}.cdx.json").write_text(json.dumps(bom, indent=2, sort_keys=True) + "\n")
        print(f"{target}: {len(components)} Rust components")


if __name__ == "__main__":
    main()
