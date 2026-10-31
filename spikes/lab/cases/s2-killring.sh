# S2: kill ring shared between buffer and minibuffer, undo history separate.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }
echo "### buffer C-k kills text, minibuffer C-y yanks the same text"
keys hello
keys -M ctrl -k a -m ctrl
keys -M ctrl -k k -m ctrl
dump
keys -M ctrl -k x -m ctrl
keys b
keys -M ctrl -k y -m ctrl
dump
echo "### minibuffer kill goes back to the shared ring"
keys -M ctrl -k a -m ctrl
keys -M ctrl -k k -m ctrl
keys -M ctrl -k g -m ctrl
keys -M ctrl -k y -m ctrl
dump
