"""Summarise what a One-Research run fetched and stored.

usage: summarize.py HOME LABEL > summary.json
Reads HOME/.config/one-research/{cache.json,one-research.log}.
"""
import collections, json, os, re, sys

home, label = sys.argv[1], sys.argv[2]
cfg = os.path.join(home, ".config", "one-research")

items = []
try:
    items = json.load(open(os.path.join(cfg, "cache.json")))
except Exception as e:  # missing cache = nothing was stored
    items_error = str(e)
else:
    items_error = None

by_source = collections.Counter(i.get("source_name", "?") for i in items)
titles = [i.get("title", "") for i in items]
suspect = {
    # A literal entity left in a title means unescaping failed.
    "literal_entity": [t for t in titles if re.search(r"&(amp|lt|gt|quot|apos|#\d+);", t)],
    # "word&word" where the source almost certainly had spaces.
    "glued_ampersand": [t for t in titles if re.search(r"[A-Za-z]{2}&[A-Za-z]{2}", t)],
    "empty_title": [i.get("url", "") for i in items if not i.get("title")],
}
samples = {
    "with_ampersand": [t for t in titles if "&" in t][:8],
    "with_lt_gt": [t for t in titles if "<" in t or ">" in t][:5],
    "non_ascii": [t for t in titles if any(ord(c) > 127 for c in t)][:8],
}
log_lines = []
try:
    for line in open(os.path.join(cfg, "one-research.log"), errors="replace"):
        if re.search(r"(complete|failed|error|ERROR|WARN|items|skipp|panick)", line):
            log_lines.append(line.rstrip()[:240])
except FileNotFoundError:
    pass

json.dump({
    "label": label,
    "cache_error": items_error,
    "total_items": len(items),
    "by_source": dict(sorted(by_source.items())),
    "suspect": {k: v[:10] for k, v in suspect.items()},
    "suspect_counts": {k: len(v) for k, v in suspect.items()},
    "samples": samples,
    "files": sorted(os.listdir(cfg)) if os.path.isdir(cfg) else [],
    "log_highlights": log_lines[-80:],
}, sys.stdout, indent=1, ensure_ascii=False)
