# D17 close entry with a real fcitx5 composition: uncommitted preedit is not
# input. The close request must cancel it and close the window, never save it.
echo "### preedit active"
vk n i h a o ctrl+shift+s
echo "### close the lab window: composition cancelled, no save"
hyprctl dispatch 'hl.dsp.window.close()' >/dev/null
sleep 1
