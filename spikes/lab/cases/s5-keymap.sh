# S5: full-keymap virtual keyboard. Unlike wtype, this uploads the real evdev
# keymap, so Shift levels, C-? vs C-/, dead keys and AltGr are observable.
vk shift+a
vk shift+1
vk ctrl+question        # C-? -> undo-redo (nothing to redo)
vk ctrl+slash           # C-/ -> undo
vk ctrl+shift+minus     # C-_ -> undo
vk --variant intl dead_acute a
vk --variant intl altgr+eacute
sleep 0.3
vk ctrl+shift+s         # spike-dump
vk ctrl+slash           # undo the composition group
vk ctrl+shift+s         # spike-dump
