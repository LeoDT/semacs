# S1: interceptor consumes command keys -> base actions and text insertion must not happen.
# Base (gpui-base input context) binds ctrl-a=SelectAll/MoveHome, ctrl-c=Copy,
# ctrl-x=Cut, ctrl-v=Paste, tab=IndentInline, enter=..., escape=Escape.
keys abc            # baseline self-insert
probe() { local k=$1; keys -M ctrl -M shift -k "$k" -m shift -m ctrl; }
probe s             # dump
keys -M ctrl -k a -m ctrl      # SelectAll / MoveHome  -> must be consumed
keys -M ctrl -k x -m ctrl      # Cut                   -> must not modify text
keys -M ctrl -k c -m ctrl      # Copy
keys -M ctrl -k v -m ctrl      # Paste
keys -k Tab                    # IndentInline
keys -k Return                 # enter binding
keys -k Escape                 # Escape binding
probe s
probe z                        # own insert at point: proves point was not moved/selected
probe s
