"""Corpus harvest for the impact benchmark (design doc section 4.1).

Subcommands:
  bands  Probe the installed tempcheq binary with per-class fixture call
         sites and record the recommended bands verbatim -> corpus/bands.json
  temps  Run `tempcheq --report` over real repositories and aggregate
         as-found temperatures per class -> corpus/profiles.json

The committed corpus/profiles.sample.json is a clearly-labeled synthetic
distribution so the pipeline runs before any real corpus is harvested.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from harness import CLASSES

# Band probes live as templates in corpus/probes/<class>.txt (data, not
# code): harvest writes them into a temp dir and reads TempCheq's
# classification back. Kept out of .py source so the auditor does not
# flag the probe fixtures themselves as call sites.

PROBE_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "probes")


def probe_bands(binary: str) -> dict:
    bands = {}
    with tempfile.TemporaryDirectory() as tmp:
        for cls in CLASSES:
            probe_path = os.path.join(PROBE_DIR, f"{cls}.txt")
            if not os.path.exists(probe_path):
                print(f"  [warn] missing probe template {probe_path}; skipping")
                continue
            with open(probe_path, "r", encoding="utf-8") as fh:
                probe = fh.read()
            probe_dir = os.path.join(tmp, cls)
            os.makedirs(probe_dir)
            with open(os.path.join(probe_dir, "probe.py"), "w", encoding="utf-8") as fh:
                fh.write(probe)
            proc = subprocess.run([binary, probe_dir, "--report"],
                                  capture_output=True, text=True, timeout=60)
            if proc.returncode != 0:
                raise SystemExit(f"tempcheq failed on {cls} probe: {proc.stderr[:300]}")
            report = json.loads(proc.stdout)
            # Class labels may carry qualifiers ("creative/ideation").
            action = next((a for a in report["actions"]
                           if a["class"].split("/")[0] == cls), None)
            if action is None:
                print(f"  [warn] probe for {cls} classified as "
                      f"{[a['class'] for a in report['actions']] or 'nothing'}; skipping")
                continue
            bands[cls] = {"low": round(action["band_low"], 4),
                          "ideal": round(action["band_ideal"], 4),
                          "high": round(action["band_high"], 4),
                          "confidence": round(action["confidence"], 4)}
    return bands


def harvest_temps(binary: str, repos: list) -> dict:
    profiles = {}
    for repo in repos:
        repo = os.path.abspath(repo)
        proc = subprocess.run([binary, repo, "--report"],
                              capture_output=True, text=True, timeout=300)
        if proc.returncode != 0:
            print(f"  [warn] tempcheq failed on {repo}: {proc.stderr[:200]}")
            continue
        report = json.loads(proc.stdout)
        for action in report["actions"]:
            cls = action["class"]
            entry = profiles.setdefault(cls, {"found_temps": [], "implicit_count": 0,
                                              "repos": []})
            if action["temperature_state"] == "explicit" and action["temperature"] is not None:
                entry["found_temps"].append(round(action["temperature"], 4))
            elif action["temperature_state"] == "implicit":
                entry["implicit_count"] += 1
            entry["repos"].append(os.path.basename(repo))
    return profiles


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="corpus harvest")
    parser.add_argument("subcommand", choices=("bands", "temps"))
    parser.add_argument("--binary", default="tempcheq")
    parser.add_argument("--out", default=None)
    parser.add_argument("repos", nargs="*", help="repos for the temps subcommand")
    args = parser.parse_args(argv)

    if args.subcommand == "bands":
        bands = probe_bands(args.binary)
        missing = [c for c in CLASSES if c not in bands]
        if missing:
            print(f"[warn] no band probe succeeded for: {missing}")
        out = args.out or "corpus/bands.json"
        with open(out, "w", encoding="utf-8") as fh:
            json.dump(bands, fh, indent=2, sort_keys=True)
        print(f"[bands] wrote {out} ({len(bands)} classes)")
        return 0 if not missing else 1

    if not args.repos:
        raise SystemExit("temps: pass at least one repository path")
    profiles = harvest_temps(args.binary, args.repos)
    profiles["_meta"] = {"synthetic": False, "repos": args.repos}
    out = args.out or "corpus/profiles.json"
    with open(out, "w", encoding="utf-8") as fh:
        json.dump(profiles, fh, indent=2, sort_keys=True)
    print(f"[temps] wrote {out}: "
          + ", ".join(f"{c}={len(v.get('found_temps', []))} explicit"
                      for c, v in sorted(profiles.items()) if c != "_meta"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
