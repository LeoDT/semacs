# S3 own undo: does our recorded grouping match Emacs?
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }

echo "### 1: type abc -> own undo (Emacs: one group, all gone)"
keys abc; dump; p u; dump
echo "### 2: type abc, move, type def -> undo (Emacs: only def)"
keys abc; p f; keys def; dump; p u; dump; p u; dump
echo "### 3: own delete-backward x3 -> undo (Emacs: all three back)"
keys abc; p d; p d; p d; dump; p u; dump
echo "### 4: own insert x2 -> undo (Emacs: one group)"
p z; p z; dump; p u; dump
echo "### 5: redo"
p r; dump
