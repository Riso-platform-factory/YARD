import json
import sys
from pathlib import Path

# Prints one fact of a run from its lifecycle trace, or nothing:
#   name, identifier   the product the process reported at run start
#   data               its data folder (needs yard_paths=debug)
#   socket             its one-instance socket or pipe
#   menu               the name of its first (application) menu
#   no-such-extension  how many URLs it refused as `no such extension`
path, key = sys.argv[1:]
try:
    contents = Path(path).read_bytes()
except FileNotFoundError:
    contents = b""

# A live writer may still be finishing the last record.
records = []
for line in contents.split(b"\n"):
    try:
        records.append(json.loads(line))
    except (json.JSONDecodeError, UnicodeDecodeError):
        continue

facts = {"no-such-extension": 0}
groups = {}
for record in records:
    kind = record.get("kind")
    if kind == "run-start":
        facts.setdefault("name", record["name"])
        facts.setdefault("identifier", record["identifier"])
    elif kind == "group-start":
        groups[record["id"]] = record
        if record["name"] == "instance-check":
            facts.setdefault("socket", str(record["fields"].get("socket", "")))
    elif kind == "line":
        group = groups.get(record["group"], {}).get("name")
        message = record.get("message")
        fields = record.get("fields", {})
        if group == "paths-resolve" and message == "data":
            facts.setdefault("data", str(fields.get("path", "")))
        elif group == "menus-set" and message == "menu":
            facts.setdefault("menu", str(fields.get("name", "")))
        elif group == "url-open" and message == "refused" and fields.get("reason") == "no such extension":
            facts["no-such-extension"] += 1

if key in facts:
    print(facts[key])
