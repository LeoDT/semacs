# D17 non-command entry: uncommitted preedit is not input. Closing the window
# cancels the composition and is allowed; nothing is saved. The fake flag is a
# deterministic stand-in because headless cannot hold synthetic marked text
# across virtual-keyboard batches; real fcitx5 is covered by a real case.
vk F10
wm kill
sleep 1
