# D17 policy branch: while a composition is active, commands and prefix keys
# are refused; observation probes and composition probes still run.
# f10 / f11 set a deterministic fake composition flag (GPUI deletes synthetic
# marked text when the virtual keyboard leaves, so headless cannot hold a real
# one); real marked-range detection is covered by real-cases/s4-ime-save.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
echo "### fake composition on: save is refused"
vk F10
vk F9
p s
echo "### C-x prefix is refused too, so C-s cannot save"
keys -M ctrl -k x -m ctrl
keys -M ctrl -k s -m ctrl
p s
echo "### buffer switch is refused"
p o
p s
echo "### composition off: save runs"
vk F11
vk F9
p s
