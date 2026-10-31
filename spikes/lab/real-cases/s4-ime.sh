# S4/S1: real fcitx5 (pinyin) preedit, commit, cancel and undo, driven through
# the real text-input-v3 path. One virtual-keyboard batch per phase keeps the
# focus and compose state stable.
echo "### 1. preedit (no undo record, not dirty)"
vk n i h a o ctrl+shift+s
echo "### 2. commit with space: one undo group, dirty"
vk space ctrl+shift+s
echo "### 3. undo after IME commit (must not crash, removes committed text)"
vk ctrl+slash ctrl+shift+s
echo "### 4. preedit then Escape cancels without text or undo record"
vk n i h a o Escape ctrl+shift+s
