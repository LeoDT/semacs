# S2: C-g echo depends on whether a region is active. Emacs echoes "Quit"
# (and deactivates the mark) with an active region, but reports the sequence
# as undefined when there is none: "C-x C-g is undefined".
p() { keys -s 200 -M ctrl -M shift -k "$1" -m shift -m ctrl; }
d() { p s; }
e() { keys -s 200 "$@"; }
echo "### active region + C-x C-g -> Quit, mark kept, active cleared"
e hello
e -M ctrl -k e -m ctrl          # C-e
e -M ctrl -k space -m ctrl      # C-SPC
e -M ctrl -k a -m ctrl          # C-a
d
e -M ctrl -k x -m ctrl
e -M ctrl -k g -m ctrl
d
echo "### no region + C-x C-g -> C-x C-g is undefined"
e -M ctrl -k x -m ctrl
e -M ctrl -k g -m ctrl
d
echo "### top-level C-g always echoes Quit"
e -M ctrl -k g -m ctrl
d
e -M ctrl -k g -m ctrl
d
