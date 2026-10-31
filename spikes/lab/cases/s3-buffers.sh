# S3/S4: two buffers keep point / mark / active / undo apart. `p l` dumps both,
# `p o` switches. A starts empty, B starts as "XY".
e() { keys -s 200 "$@"; }
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
l() { p l; }
o() { p o; }
d() { l; }
echo "### A: type aa (group 1)"
e aa
d
echo "### switch to B, type bb after XY (group 1)"
o
d
e bb
d
echo "### back to A, type cc (group 2: switch is a boundary)"
o
e cc
d
echo "### undo in A twice: cc, then aa; B untouched"
p u
d
p u
d
echo "### switch to B, undo: XY remains"
o
p u
d
echo "### marks are buffer-local: B mark, then A mark"
e -M ctrl -k space -m ctrl
e -M ctrl -k a -m ctrl
d
o
e a
e -M ctrl -k space -m ctrl
e -M ctrl -k a -m ctrl
d
