# S3: undo / redo move the mark with the text, not by a group snapshot.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p s; }
ctrl() { keys -s 200 -M ctrl -k "$1" -m ctrl; }

echo "### mark=2, insert before it"
ctrl a
ctrl f
ctrl f
ctrl space
ctrl a
keys x
d
echo "### undo: insertion goes away, mark returns to 2"
p u
d
echo "### redo: mark shifts again (point is not part of this assertion)"
p r
d
