# D17 focus entry with a real fcitx5 composition: losing focus must cancel the
# preedit (uncommitted text is not input) and leave text / dirty / undo / mark
# untouched. The lab window is refocused before the verification dump.
orig=$(hyprctl -j activeworkspace | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
tmp=$(hyprctl -j workspaces | python3 -c 'import json,sys; print(max([w["id"] for w in json.load(sys.stdin)]+[0])+1)')
echo "### preedit active, leave the workspace"
vk n i h a o ctrl+shift+s
focus_workspace "$tmp"
sleep 1
echo "### return: composition cancelled, buffer unchanged"
focus_workspace "$orig"
sleep 0.6
focus_window "$ADDR"
sleep 0.4
vk ctrl+shift+s
