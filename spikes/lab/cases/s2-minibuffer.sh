# S2: minimal minibuffer as a second EditTarget, prompt, cancel, confirm.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
dump() { p s; }

echo "### C-x b enters the minibuffer; type, C-g cancels"
keys -M ctrl -k x -m ctrl
keys b
dump
keys abc
dump
keys -M ctrl -k g -m ctrl
dump
echo "### enter again, type, C-a / C-e motion, RET confirms"
keys -M ctrl -k x -m ctrl
keys b
keys xyz
keys -M ctrl -k a -m ctrl
dump
keys -k Return
dump
echo "### buffer text untouched by minibuffer editing"
keys q
dump
