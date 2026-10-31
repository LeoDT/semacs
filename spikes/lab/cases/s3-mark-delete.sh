# S3: deleting text covered by the mark collapses it to the deletion start;
# undo restores the old offset only while the mark is still at that start.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p s; }
ctrl() { keys -s 200 -M ctrl -k "$1" -m ctrl; }

echo "### mark=2, kill whole line: mark collapses to 0"
ctrl a
ctrl f
ctrl f
ctrl space
ctrl a
ctrl k
d
echo "### undo: text and mark come back"
p u
d
echo "### redo: mark collapses again"
p r
d
