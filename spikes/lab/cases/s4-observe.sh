# S4/S1: can the target adapter observe platform text input, and does the
# pending-prefix gate stop insertion?
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
echo "### type abc (platform path)"
keys abc
echo "### preedit -> commit through our own adapter"
p e
echo "### prefix pending, then type 'x' (must not be inserted)"
p p
keys x
p s
echo "### clear prefix, type 'x' again (must be inserted)"
p p
keys x
p s
