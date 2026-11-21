"""Collect license texts for the resolved dependencies of the shipped Rust programs."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
UPSTREAM = ROOT / "licenses/rust-upstream"


def collect(destination, variant):
    host = re.search(r"^host: (.+)$", subprocess.check_output(["rustc", "-vV"], text=True), re.M)[1]
    features = "voice-vosk,embed-gui"
    roots = {"virtual-avatar-connect", "vac-english2kana-worker"}
    if variant == "portable":
        features += ",voice-whisper,vac-stt-whisper-worker/vulkan"
        roots.add("vac-stt-whisper-worker")
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", host,
        "--no-default-features", "--features", features
    ], cwd=ROOT))
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    pending = [p for p in metadata["workspace_members"] if packages[p]["name"] in roots]
    assert len(pending) == len(roots)
    visited = set()
    while pending:
        key = pending.pop()
        if key in visited:
            continue
        visited.add(key)
        pending.extend(d["pkg"] for d in nodes[key]["deps"]
                       if any(k["kind"] != "dev" for k in d["dep_kinds"]))
    pinned = json.loads((UPSTREAM / "sources.json").read_text(encoding="utf-8"))["sources"]
    texts = {}
    entries = []
    def record(raw):
        text = raw.decode("utf-8-sig", errors="replace").strip()
        tag = hashlib.sha256(raw).hexdigest()
        texts[tag] = text
        return tag
    for key in sorted(visited):
        p = packages[key]
        if not p["source"]:
            continue
        directory = Path(p["manifest_path"]).parent
        row = [p["name"] + " " + p["version"], "Source: " + key, "License: " + (p["license"] or "see included license file"),
               "Authors: " + ", ".join(p["authors"])]
        files = set()
        if p["license_file"]:
            files.add(directory / p["license_file"])
        for item in directory.rglob("*"):
            if item.is_file() and re.fullmatch(r"(LICENSE|LICENCE|COPYING|NOTICE|COPYRIGHT)([._-].*)?", item.name, re.I):
                files.add(item)
        for item in sorted(files):
            if item.is_file() and item.stat().st_size:
                row.append(item.relative_to(directory).as_posix() + "  " + record(item.read_bytes()))
        vcs = directory / ".cargo_vcs_info.json"
        commit = json.loads(vcs.read_bytes())["git"]["sha1"] if vcs.is_file() else p["source"].split("#")[-1]
        for source in pinned:
            if source.get("commit") != commit or not any(c["name"] == p["name"] and c["version"] == p["version"] for c in source["matching_crates"]):
                continue
            for notice in source["notices"]:
                raw = (UPSTREAM / notice["file"]).read_bytes()
                if hashlib.sha256(raw).hexdigest() != notice["sha256"]:
                    raise ValueError(notice["file"])
                row.append(notice["file"] + "  " + record(raw))
        if len(row) == 4:
            if "Apache-2.0" in (p["license"] or ""):
                row.append("Apache-2.0  " + record((UPSTREAM / "Apache-2.0.standard.txt").read_bytes()))
            elif p["license"] == "MIT":
                raw = (ROOT / "LICENSE").read_bytes()
                row.append("MIT  " + record(raw[raw.index(b"Permission is hereby granted"):]))
            else:
                raise ValueError("Missing license text for " + key)
        entries.append("\n".join(row))
    destination.write_text("Rust and bundled native dependency notices\n\n" + "\n\n".join(entries)
                           + "\n\n" + "\n\n".join(tag + "\n" + text for tag, text in sorted(texts.items())) + "\n", encoding="utf-8")
    print(f"Collected {len(entries)} package records and {len(texts)} distinct license texts")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    parser.add_argument("variant", choices=["portable", "minimal"])
    args = parser.parse_args()
    collect(args.destination, args.variant)
