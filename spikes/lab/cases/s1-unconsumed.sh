# S1 contrast: same keys with SPIKE_MODE=log (nothing consumed) so the base
# bindings/insertion run. Shows what the interceptor is actually preventing.
keys abc
keys -M ctrl -k a -m ctrl      # SelectAll/MoveHome fires
probe() { local k=$1; keys -M ctrl -M shift -k "$k" -m shift -m ctrl; }
probe z                        # own insert replaces selection if SelectAll fired
keys -k Tab                    # IndentInline
keys -k Return
