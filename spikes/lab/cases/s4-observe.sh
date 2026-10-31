# S4/S1: platform text input is observable through our target adapter, and a
# pending prefix consumes following characters as key sequences (no insert).
# Raw log also carries `TARGET accepts_text_input ... allowed=false` while the
# prefix is pending; those lines are filtered from the golden comparison.
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
echo "### type abc (platform path)"
keys abc
echo "### preedit through our own adapter"
p e
echo "### C-x pending, then 'x': parsed, not inserted"
keys -M ctrl -k x -m ctrl
keys x
p s
