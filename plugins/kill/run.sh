#!/bin/sh
# `kill <name>`: list matching processes; Enter quits the selected one (SIGTERM).
ps -axco pid=,comm= | LC_ALL=C awk '
  function json(s) { gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s); return s }
  BEGIN { q = tolower(ENVIRON["BARKLEY_QUERY"]); printf "{\"items\":[" }
  { pid = $1; name = $0; sub(/^ *[0-9]+ /, "", name) }
  q != "" && index(tolower(name), q) {
    printf "%s{\"title\":\"%s\",\"subtitle\":\"PID %s\",\"verb\":\"Quit\",", sep, json(name), pid
    printf "\"icon\":{\"symbol\":\"xmark.octagon\"},\"action\":{\"type\":\"run\",\"cmd\":\"/bin/kill\",\"args\":[\"%s\"]}}", pid
    sep = ","
  }
  END {
    if (q == "") printf "{\"title\":\"Quit a process\",\"subtitle\":\"Type part of its name\",\"icon\":{\"symbol\":\"xmark.octagon\"}}"
    print "]}"
  }'
