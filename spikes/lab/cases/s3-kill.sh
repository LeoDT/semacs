# S3: kill grouping, yank grouping, End of buffer (mirrors Emacs kill2 / kill-yank).
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }
echo "### C-a, C-k (kills cd), C-k (End of buffer), undo, undo"
keys -M ctrl -k a -m ctrl
keys -M ctrl -k k -m ctrl
keys -M ctrl -k k -m ctrl
dump
keys -M ctrl -k slash -m ctrl
dump
keys -M ctrl -k slash -m ctrl
dump
