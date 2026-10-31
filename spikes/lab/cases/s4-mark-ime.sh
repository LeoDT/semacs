# S4: an IME commit is a real text insertion for the mark, and undo moves it
# back. The probe starts and commits a synthetic composition in one command so
# the marked text survives the virtual-keyboard leave.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p s; }
ctrl() { keys -s 200 -M ctrl -k "$1" -m ctrl; }

echo "### point=1, mark=2 (abcd)"
ctrl a
ctrl f
ctrl f
ctrl space
ctrl a
ctrl f
d
echo "### preedit + commit at point: mark shifts by the committed bytes"
vk F8
d
echo "### undo removes the committed text and restores the mark"
p u
d
