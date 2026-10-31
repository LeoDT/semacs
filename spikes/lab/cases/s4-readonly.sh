# S1/S4: read-only target must not accept insertion, but motion still works.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }
keys z
dump
keys -M ctrl -k a -m ctrl
dump
