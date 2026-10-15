#!/bin/sh
# Offers "Empty Trash" for queries like "empty", "empty tr", "trash".
# Asks Finder (Automation permission, asked once) for the item count first,
# so the row says what's about to be deleted.
q=$(printf '%s' "${BARKLEY_QUERY-}" | tr '[:upper:]' '[:lower:]' | sed 's/^ *//; s/ *$//')
match=
case "empty trash" in "$q"*) match=1 ;; esac
case "trash" in "$q"*) match=1 ;; esac
if [ ${#q} -lt 3 ] || [ -z "$match" ]; then
  echo '{"items":[]}'
  exit 0
fi

n=$(osascript -e 'tell application "Finder" to count items of trash' 2>/dev/null)
case "$n" in
  '') echo '{"items":[{"title":"Empty Trash","subtitle":"Can'"'"'t ask Finder: allow Barkley in Privacy & Security → Automation","icon":{"symbol":"trash.slash"}}]}' ;;
  0) echo '{"items":[{"title":"Trash is empty","icon":{"symbol":"trash"}}]}' ;;
  *)
    [ "$n" = 1 ] && what="1 item" || what="$n items"
    printf '{"items":[{"title":"Empty Trash","subtitle":"Permanently deletes %s","verb":"Empty","icon":{"symbol":"trash.fill"},"action":{"type":"run","cmd":"/usr/bin/osascript","args":["-e","tell application \\"Finder\\" to empty trash"]}}]}\n' "$what"
    ;;
esac
