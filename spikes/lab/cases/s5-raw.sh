# S5: raw keystroke / key_char / input preference per physical key.
# Run with SPIKE_MODE=log so nothing is consumed and the resulting text is visible.
k() { keys "$@"; }
k -M ctrl -M shift -k slash -m shift -m ctrl     # C-?
k -k Return                                      # RET
k -M ctrl -k m -m ctrl                           # C-m
k -k Tab                                         # TAB
k -M ctrl -k i -m ctrl                           # C-i
k -k BackSpace                                   # DEL / backspace
k -M shift -k a -m shift                         # Shift char
k -k minus                                       # plain char
k -M alt -k f -m alt                             # M-f
k -M altgr -k e -m altgr                         # AltGr
k -k dead_acute                                  # dead key start
k -k a                                           # dead key composed
k -k Escape                                      # ESC
k -M logo -k a -m logo                           # s-a
k -k Left                                        # arrow
k -k Page_Up                                     # pageup
k -k F1                                          # function key
