# S2: prefix pending, C-g cancel, undefined sequence, echo area.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }

echo "### C-x C-s -> save-buffer"
keys -M ctrl -k x -m ctrl
keys -M ctrl -k s -m ctrl
dump
echo "### C-x C-x u -> undo (three-key sequence)"
keys -M ctrl -k x -m ctrl
keys u
dump
