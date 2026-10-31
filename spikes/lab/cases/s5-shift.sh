# S5: is Shift state propagated into key_char? wtype text mode vs explicit modifier.
keys 'A'                                          # wtype text: should be shift+a internally
keys -M shift -k a -m shift                       # explicit shift+a
keys -M shift -k b -m shift
keys -M ctrl -k slash -m ctrl                     # C-/
keys -M ctrl -M shift -k slash -m shift -m ctrl   # C-?
keys -M ctrl -M shift -k s -m shift -m ctrl       # control: ctrl+shift+s
keys -M ctrl -M shift -k minus -m shift -m ctrl   # C-_
