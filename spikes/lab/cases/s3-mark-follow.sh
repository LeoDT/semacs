# S3/S4: the mark is a marker, not an offset. Insert before / at / after it.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p s; }
ctrl() { keys -s 200 -M ctrl -k "$1" -m ctrl; }

echo "### point=2, mark at point"
ctrl a
ctrl f
ctrl f
ctrl space
d
echo "### insert at mark: mark stays"
keys x
d
echo "### insert before mark: mark shifts by 1"
ctrl a
keys y
d
echo "### insert after mark: mark unchanged"
ctrl e
keys z
d
