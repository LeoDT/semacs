# S3: undo then immediate input starts a new group and drops the redo branch.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
keys c; p s
p u; p s
keys d
p r; p s
p u; p s
