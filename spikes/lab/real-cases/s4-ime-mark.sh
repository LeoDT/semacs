# S4: mark follows a real fcitx5 commit as one insertion and undo moves it back.
# fcitx5 grabs Ctrl-SPC globally, so the mark is set through the F7 probe binding
# (same set-mark-command path) while the IME is inactive.
echo "### mark=2 with the IME inactive"
keys -M ctrl -k a -m ctrl
keys -M ctrl -k f -m ctrl
keys -M ctrl -k f -m ctrl
vk F7
keys -M ctrl -k a -m ctrl
keys -M ctrl -k f -m ctrl
keys -M ctrl -M shift -k s -m shift -m ctrl
echo "### activate pinyin, then preedit + space commit at point=1"
fcitx5-remote -o >/dev/null 2>&1
sleep 0.4
vk n i h a o space ctrl+shift+s
echo "### undo removes the committed text and restores the mark"
vk ctrl+slash ctrl+shift+s
