# S3: after a deletion collapsed the mark, moving the mark elsewhere means undo
# only shifts it with the reinserted text (Emacs marker adjustment rule).
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p s; }
ctrl() { keys -s 200 -M ctrl -k "$1" -m ctrl; }

echo "### mark=3, delete abc: mark collapses to 0"
ctrl a
ctrl f
ctrl f
ctrl f
ctrl space
ctrl a
ctrl d
ctrl d
ctrl d
d
echo "### point=3, mark=3, deactivate, then undo: mark shifts to 6, not 3"
ctrl e
ctrl space
ctrl g
p u
d
