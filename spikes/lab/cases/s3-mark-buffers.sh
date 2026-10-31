# S3/S4: mark follows edits in its own buffer only; switching keeps each mark.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p l; }
ctrl() { keys -s 200 -M ctrl -k "$1" -m ctrl; }

echo "### A: mark=3 (abcdef)"
ctrl a
ctrl f
ctrl f
ctrl f
ctrl space
d
echo "### B: mark=1, insert at mark"
p o
ctrl a
ctrl f
ctrl space
keys y
d
echo "### A: insert before its mark -> 4"
p o
ctrl a
keys x
d
echo "### B: undo only, A's mark untouched"
p o
p u
d
