# S3: dirty vs save point (mirrors Emacs dirty / dirty-past-save).
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }
echo "### type abc, save, type d, undo -> clean again"
keys abc
dump
keys -M ctrl -k x -m ctrl
keys -M ctrl -k s -m ctrl
dump
keys d
dump
keys -M ctrl -k slash -m ctrl
dump
