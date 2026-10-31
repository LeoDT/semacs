# S4: pointer drag selection must be importable as point/mark/active.
# `p k` (C-S-k) imports the EditorState selection into our point/mark model.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
sync_dump() { p k; p s; }
echo "### forward drag (left to right): mark before point"
mouse drag 40 10 160 10
sync_dump
echo "### backward drag (right to left): point before mark"
mouse drag 160 10 40 10
sync_dump
echo "### click: point moves, region deactivates, mark position kept"
mouse click 30 10
sync_dump
