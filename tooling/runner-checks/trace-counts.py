import json
import sys
from pathlib import Path

path, scheme = sys.argv[1:]
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
groups = {record["id"]: record for record in records if record["kind"] == "group-start"}
requests = {group_id for group_id, group in groups.items() if group["name"] == "instance-request"}
urls = forwarded = 0
for record in records:
    # Dispatch refuses the unregistered probe host. Cold startup can also
    # open a preliminary URL span while that URL is queued.
    if record.get("message") != "refused":
        continue
    group = groups.get(record["group"])
    if not group or group["name"] != "url-open":
        continue
    if scheme != "*" and group["fields"].get("scheme") != scheme:
        continue
    urls += 1
    parent = group["parent"]
    while parent in groups:
        if parent in requests:
            forwarded += 1
            break
        parent = groups[parent]["parent"]

print(urls, len(requests), forwarded)
