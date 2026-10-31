# S3: kill then yank are separate undo units; yank sets mark (mirrors Emacs kill-yank).
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }
keys -M ctrl -k a -m ctrl
keys -M ctrl -k k -m ctrl
keys -M ctrl -k y -m ctrl
dump
keys -M ctrl -k slash -m ctrl
dump
keys -M ctrl -k slash -m ctrl
dump
