# D17 with a real fcitx5 composition: save (F9, no C-x prefix) is refused while
# the composition is active and the composition is left intact. After Escape
# cancels it, the same save command runs on the finished text.
echo "### preedit active: save refused"
vk n i h a o F9 ctrl+shift+s
echo "### cancel composition, then save runs"
vk Escape F9 ctrl+shift+s
