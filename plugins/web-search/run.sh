#!/bin/sh
# Barkley plugin: web search. `g <term>`, and the fallback for unmatched queries.
# One awk process does the URL- and JSON-encoding; reads the query from the
# environment so awk never interprets backslash escapes in it.
export BARKLEY_QUERY="${1-${BARKLEY_QUERY-}}"
LC_ALL=C exec awk 'BEGIN {
  q = ENVIRON["BARKLEY_QUERY"]
  sub(/^[ \t]+/, "", q); sub(/[ \t]+$/, "", q)
  name = ENVIRON["BARKLEY_SEARCH_NAME"]; if (name == "") name = "the web"
  if (q == "") {
    printf "{\"items\":[{\"title\":\"Search %s…\",\"subtitle\":\"Type a search term\",\"icon\":{\"symbol\":\"globe\"}}]}\n", json(name)
    exit
  }
  for (i = 0; i < 256; i++) ord[sprintf("%c", i)] = i
  enc = ""
  for (i = 1; i <= length(q); i++) {
    c = substr(q, i, 1)
    enc = enc (c ~ /[A-Za-z0-9_.~-]/ ? c : sprintf("%%%02X", ord[c]))
  }
  url = ENVIRON["BARKLEY_SEARCH_URL"]; if (url == "") url = "https://www.google.com/search?q={query}"
  gsub(/\{query\}/, enc, url)
  printf "{\"items\":[{\"title\":\"Search %s for \\u201c%s\\u201d\",\"subtitle\":\"%s\",\"score\":1,\"icon\":{\"symbol\":\"globe\"},\"action\":{\"type\":\"open_url\",\"url\":\"%s\"}}]}\n", json(name), json(q), json(url), json(url)
}
function json(s,   out, i, c) {
  out = ""
  for (i = 1; i <= length(s); i++) {
    c = substr(s, i, 1)
    if (c == "\\" || c == "\"") out = out "\\" c
    else if (c == "\t") out = out "\\t"
    else if (c < " ") out = out " "
    else out = out c
  }
  return out
}'
