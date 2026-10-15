#!/bin/sh
# Example Barkley plugin: echoes the query back, one item per action type.
# The query (without the keyword) arrives as $1 and as $BARKLEY_QUERY.

# Minimal JSON string escaping: backslash and double quote.
q=$(printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g')

if [ -z "$q" ]; then
  echo '{"items":[{"title":"Echo…","subtitle":"Type something after \"echo \""}]}'
  exit 0
fi

url=$(printf '%s' "$1" | sed 's/%/%25/g; s/ /%20/g; s/&/%26/g; s/#/%23/g; s/?/%3F/g; s/+/%2B/g; s/"/%22/g; s/\\/%5C/g')

cat <<JSON
{"items":[
  {"title":"$q","subtitle":"Copy to clipboard","icon":{"symbol":"doc.on.doc"},"action":{"type":"copy","text":"$q"}},
  {"title":"Say \"$q\"","subtitle":"Runs /usr/bin/say","icon":{"symbol":"waveform"},"action":{"type":"run","cmd":"/usr/bin/say","args":["$q"]}},
  {"title":"Look up \"$q\" in Dictionary","subtitle":"Opens dict://","icon":{"symbol":"character.book.closed"},"action":{"type":"open_url","url":"dict://$url"}}
]}
JSON
