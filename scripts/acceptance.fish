#!/usr/bin/env fish
# Host-only acceptance; never falls back to software rendering.
function fail
    echo "STOP: $argv" >&2
    if set -q _semacs_acceptance_output; and test -d "$_semacs_acceptance_output"
        printf '%s\n' "$argv" > "$_semacs_acceptance_output/STOP.txt"
    end
    exit 1
end

if test (count $argv) -ne 3; or not contains -- $argv[1] perf display-perf split-perf ime split-ime manual candidates scripting messages themes syntax languages flicker styles styles-perf syntax-perf open-diagnostic; or test "$argv[3]" != --consent
    echo "Usage: fish scripts/acceptance.fish <perf|display-perf|split-perf|ime|split-ime|manual|candidates|scripting|messages|themes|syntax|languages|flicker|styles|styles-perf|syntax-perf|open-diagnostic> NEW_OUTPUT_DIRECTORY --consent" >&2
    exit 2
end
set -l mode $argv[1]
set -l root (path resolve (path dirname (status filename))/..)
cd "$root"; or fail "cannot enter repository"
set -l output (path resolve "$argv[2]")
test -n "$WAYLAND_DISPLAY"; or fail "run from the actual Wayland host terminal"
test -d /dev/dri; or fail "no /dev/dri; hardware acceptance cannot run here"
for binary in cargo python3 fc-match sha256sum
    command -q $binary; or fail "missing $binary"
end
if test "$mode" = ime
    for binary in hyprctl wtype fcitx5-remote
        command -q $binary; or fail "missing $binary"
    end
    test -n "$HYPRLAND_INSTANCE_SIGNATURE"; or fail "IME automation requires Hyprland"
end
if test "$mode" = split-ime
    for binary in hyprctl grim
        command -q $binary; or fail "split-ime requires $binary"
    end
    test -n "$HYPRLAND_INSTANCE_SIGNATURE"; or fail "split-ime requires Hyprland"
end
test ! -e "$output"; or fail "output must not exist: $output"
set -l untracked (git ls-files --others --exclude-standard -z | string split0)
python3 -c 'import pathlib,sys; pathlib.Path(sys.argv[1]).mkdir(parents=True, exist_ok=False)' "$output"
or fail "cannot create output"
set -g _semacs_acceptance_output "$output"
git rev-parse HEAD > "$output/git-head.txt"
git status --short > "$output/git-status.txt"
git diff --no-ext-diff --binary > "$output/source.patch"
or fail "cannot capture tracked source changes"
for file in $untracked
    git diff --no-ext-diff --binary --no-index -- /dev/null "$file" >> "$output/source.patch"
    test $status -le 1; or fail "cannot capture untracked source: $file"
end
echo "Output: $output"
echo "Build logs are saved; any failure stops the batch without retries."
if contains -- "$mode" candidates scripting messages themes syntax languages flicker styles
    cargo build --release --bin semacs --target-dir "$root/target" > "$output/build.log" 2>&1
else if test "$mode" = split-ime
    cargo build --release --bin semacs --example native_split_ime --target-dir "$root/target" > "$output/build.log" 2>&1
else
    cargo build --release --bin semacs --example perf_native --features native-profiler --target-dir "$root/target" > "$output/build.log" 2>&1
end
or fail "build failed; see $output/build.log"
sha256sum "$root/target/release/semacs" > "$output/binary.sha256"
or fail "cannot capture binary identity"

if test "$mode" = open-diagnostic
    echo "Diagnostic only: one mixed-file open. No other shapes, editing or OS key injection."
    echo "Existing budgets still apply; a STOP is expected if the slow frame recurs."
    echo "Do not type in, resize or close the probe; send the output directory even on failure."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "fixture generation failed"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/open-diagnostic" --consent --m2-open-diagnostic --sample mixed --runs 1
    or fail "single-open diagnostic stopped; keep CSV / frames.csv / views.csv / preparation.csv / diagnostic.json and report this directory"
else if test "$mode" = split-perf
    echo "Shared-buffer dual-view performance: right/below × mixed/single-ascii × 3."
    echo "Selected-view budgets retained; all-view ready is a separate baseline with no new threshold."
    echo "CPU/platform submission only, NOT GPU execution or screen presentation."
    echo "No screenshots, IME changes or OS key injection. Do not type in, resize or close probes."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "fixture generation failed"
    for direction in right below
        python3 -B scripts/perf_native.py "$output/sample.txt" "$output/split-$direction" --consent --split $direction --sample mixed --sample single-ascii --runs 3
        or fail "split performance stopped; keep and report this directory, do not rerun"
    end
else if test "$mode" = styles-perf
    echo "E3 representative CPU/submission checks: mixed 8 MiB x3, then Markdown 256 KiB x3."
    echo "Six sequential windows at most. No screenshots, OS key injection or scripted IME changes."
    echo "Do not type in, resize or close probes. Any failed report stops subsequent windows."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "mixed fixture generation failed"
    python3 -c 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.parent.mkdir(parents=True); p.open("xb").close()' "$output/config/semacs/init.js"
    or fail "cannot prepare isolated empty performance configuration"
    env XDG_CONFIG_HOME="$output/config" python3 -B scripts/perf_native.py "$output/sample.txt" "$output/native-mixed" --consent --sample mixed --runs 3
    or fail "E3 mixed CPU check stopped; keep and report this directory, do not rerun"
    test -f "$output/config/semacs/init.js"; and test ! -L "$output/config/semacs/init.js"; and test ! -s "$output/config/semacs/init.js"
    or fail "isolated mixed configuration changed"
    set -l syntax_manifest (python3 -B scripts/perf_syntax_sample.py "$output/files")
    or fail "syntax performance fixtures failed"
    string match --quiet --regex '^[0-9a-f]{64}$' "$syntax_manifest"
    or fail "missing frozen syntax batch identity"
    python3 -B scripts/perf_native.py "$output/files/markdown.md" "$output/native-markdown" --consent --syntax --sample markdown --runs 3 --syntax-manifest-sha256 "$syntax_manifest"
    or fail "E3 Markdown CPU check stopped; keep and report this directory, do not rerun"
else if test "$mode" = syntax-perf
    echo "Syntax-enabled Markdown/HTML, ~256 KiB each, three runs each; six sequential windows."
    echo "Input, CPU draw/submission and stable idle only; not OS keyboard, IME latency, GPU time or large-file matrix."
    echo "Isolated empty child configuration. No screenshots, OS key injection or scripted IME changes."
    echo "Do not type in, resize or close probes. Missing pending-syntax input coverage also stops the batch."
    set -l syntax_manifest (python3 -B scripts/perf_syntax_sample.py "$output/files")
    or fail "syntax performance fixtures failed"
    string match --quiet --regex '^[0-9a-f]{64}$' "$syntax_manifest"
    or fail "missing frozen syntax batch identity"
    for sample in markdown html
        set -l extension html
        if test "$sample" = markdown
            set extension md
        end
        python3 -B scripts/perf_native.py "$output/files/$sample.$extension" "$output/native-$sample" --consent --syntax --sample "$sample" --runs 3 --syntax-manifest-sha256 "$syntax_manifest"
        or fail "syntax CPU batch stopped; keep and report this directory, do not rerun"
    end
else if test "$mode" = display-perf
    echo "Display regression: mixed/single-ascii × 3; hardware rendering required."
    echo "Native CPU/submission timings only, NOT GPU execution or screen presentation."
    echo "No M1/M2 matrix or OS key injection; do not type in, resize or close test windows."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "fixture generation failed"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/native-display" --consent --sample mixed --sample single-ascii --runs 3
    or fail "display regression stopped; keep this directory and report it, do not rerun"
else if test "$mode" = perf
    echo "Native CPU/submission tests only, NOT GPU execution or presentation."
    echo "Do not type in, resize or close test windows. No OS key injection."
    echo "M2 six shapes × 3; baseline six × 3; M1 mixed/single-ascii × 3 (60 seconds each)."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "fixture generation failed"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/m2-mixed" --consent --m2 --sample mixed --runs 3
    or fail "M2 mixed failed; keep this directory and report it, do not rerun"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/m2-rest" --consent --m2 --runs 3 \
        --sample short-ascii --sample short-unicode --sample long-ascii --sample single-ascii --sample single-unicode
    or fail "M2 matrix failed; keep this directory and report it"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/native-baseline" --consent --runs 3
    or fail "native baseline failed; keep this directory and report it"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/native-m1" --consent --m1 --sample mixed --sample single-ascii --runs 3
    or fail "M1 regression failed; keep this directory and report it"
else if test "$mode" = ime
    echo "Real wtype/fcitx5 input: do not use keyboard/mouse during this batch."
    echo "The driver checks probe focus before injection and restores focus / input method."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "fixture generation failed"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/native-ime" --consent --ime --sample mixed --sample single-ascii --runs 3
    or fail "IME failed; keep logs/screenshots and report it"
else if test "$mode" = split-ime
    echo "Auto-asserted split/body/minibuffer IME integration, NOT a performance benchmark."
    echo "Follow the probe instructions: at each of three targets, ni -> Escape, then ni -> commit 你."
    echo "Use your usual Chinese Pinyin input method; wait for each instruction. No other editing."
    echo "No global key injection or scripted IME changes. Do not resize or close the probe."
    echo "PRIVACY: six screenshots capture the ENTIRE probe monitor, including other visible windows."
    echo "Hide sensitive windows on that monitor before continuing. Original focus will be restored."
    begin
        uname -a
        rustc --version
        fc-match monospace
    end > "$output/environment.txt" 2>&1
    python3 -B scripts/split_ime.py "$output/split-ime" --consent
    or fail "split IME integration stopped; preserve this directory and report it, do not rerun"
else if test "$mode" = scripting
    begin
        uname -a
        rustc --version
        printf 'WAYLAND_DISPLAY=%s\nXDG_SESSION_TYPE=%s\nXDG_CURRENT_DESKTOP=%s\nLANG=%s\n' "$WAYLAND_DISPLAY" "$XDG_SESSION_TYPE" "$XDG_CURRENT_DESKTOP" "$LANG"
        fc-match monospace
    end > "$output/environment.txt" 2>&1
    echo "Two manual scripting smoke windows, isolated XDG_CONFIG_HOME and disposable text."
    echo "No OS key injection, IME changes, screenshots or performance measurements."
    echo "Follow the numbered terminal prompts; leave each window open until asked to close it."
    echo "Any failure stops the batch and terminates only the child test app; all artifacts are kept."
    python3 -B scripts/scripting_manual.py "$output/scripting" --binary "$root/target/release/semacs" --consent
    or fail "scripting smoke stopped; keep and report $output, do not rerun"
else if contains -- "$mode" themes syntax languages flicker styles
    begin
        uname -a
        rustc --version
        printf 'WAYLAND_DISPLAY=%s\nXDG_SESSION_TYPE=%s\nXDG_CURRENT_DESKTOP=%s\nLANG=%s\n' "$WAYLAND_DISPLAY" "$XDG_SESSION_TYPE" "$XDG_CURRENT_DESKTOP" "$LANG"
        fc-match monospace
        fc-match monospace:style=Bold
        fc-match monospace:style=Italic
    end > "$output/environment.txt" 2>&1
    set -l body body.txt
    set -l checks theme-start-split theme-switch theme-cancel-unknown theme-minibuffer-messages theme-editing
    if test "$mode" = syntax
        set body body.rs
        set checks syntax-start-split syntax-theme syntax-edit-history syntax-language syntax-layout-save
    else if test "$mode" = languages
        set body body.md
        set checks languages-colors languages-split-theme languages-markdown-ime-history languages-html-ime languages-layout-save
    else if test "$mode" = flicker
        set body body.md
        set checks flicker-markdown flicker-html flicker-shared-history
    else if test "$mode" = styles
        set body body.rs
        set checks styles-fonts styles-layout-theme styles-rust-ime styles-markdown-ime-save
    end
    python3 -B scripts/m2_manual.py prepare "$output/files" --profile "$mode"
    or fail "theme/syntax fixture generation failed"
    echo "One manual $mode window; isolated configuration and disposable files only."
    if contains -- "$mode" languages styles
        echo "Use your usual Chinese IME manually for the two prompted body targets; no scripted IME changes."
        echo "No screenshots, OS key injection or performance measurements."
    else
        echo "No screenshots, OS key injection, IME changes or performance measurements."
    end
    echo "Follow the "(count $checks)" printed groups; stop on a problem, close the window and answer no."
    env XDG_CONFIG_HOME="$output/files/config" "$root/target/release/semacs" "$output/files/$body" > "$output/$mode.stdout.log" 2> "$output/$mode.stderr.log"
    or fail "theme/syntax session exited abnormally; keep this output directory"
    for item in $checks
        read -l -P "$item: all corresponding observations passed? Type yes or no: " answer
        or set answer no
        printf '%s\t%s\n' "$item" "$answer" >> "$output/files/observations.tsv"
        if test "$answer" != yes
            read -l -P "Failure / unobserved details (optional): " note
            printf '%s\n' "$item: $note" >> "$output/notes.txt"
            python3 -B scripts/m2_manual.py verify "$output/files" --profile "$mode" > "$output/verification.log" 2>&1
            fail "theme/syntax observation not passed; keep and report $output"
        end
    end
    read -l -P "Observed window dimensions, font/color readability / flicker and (languages/styles) IME name: " geometry
    printf '%s\n' "$geometry" >> "$output/notes.txt"
    python3 -B scripts/m2_manual.py verify "$output/files" --profile "$mode" > "$output/verification.log" 2>&1
    or fail "theme/syntax observations or independent disk/config bytes failed; see $output/verification.log"
    if test "$mode" = styles
        echo "E3 user-observed font/IME behavior and independent disk/config bytes verified; native performance/IME latency/GPU remain unmeasured."
    else if test "$mode" = languages
        echo "E2 user-observed body IME and independent disk/config bytes verified; native performance/IME latency/GPU remain unmeasured."
    else
        echo "Theme/syntax manual observations and disk/config bytes verified; native performance/IME/GPU remain unmeasured."
    end
else if test "$mode" = messages
    begin
        uname -a
        rustc --version
        printf 'WAYLAND_DISPLAY=%s\nXDG_SESSION_TYPE=%s\nXDG_CURRENT_DESKTOP=%s\nLANG=%s\n' "$WAYLAND_DISPLAY" "$XDG_SESSION_TYPE" "$XDG_CURRENT_DESKTOP" "$LANG"
        fc-match monospace
    end > "$output/environment.txt" 2>&1
    python3 -B scripts/m2_manual.py prepare "$output/files" --profile messages
    or fail "message fixture generation failed"
    echo
    echo "One manual D1 window; isolated configuration and disposable files only."
    echo "No screenshots, OS key injection, IME changes or performance matrix."
    echo "Use English/ASCII input; follow the five groups printed above."
    echo "C-c l: long error; C-c m: multiline error; C-c b: +1px; C-c n: reset to 24px."
    echo "On a problem stop the steps, close the test window and answer no; do not repair or retry."
    echo "Files: $output/files"
    env XDG_CONFIG_HOME="$output/files/config" "$root/target/release/semacs" "$output/files/body.txt" > "$output/messages.stdout.log" 2> "$output/messages.stderr.log"
    or fail "message session exited abnormally; keep this output directory"
    for item in message-long message-multiline-history message-minibuffer message-layout-font message-copy-editing
        read -l -P "$item: all corresponding observations passed? Type yes or no: " answer
        or set answer no
        printf '%s\t%s\n' "$item" "$answer" >> "$output/files/observations.tsv"
        if test "$answer" != yes
            read -l -P "Failure / unobserved details (optional): " note
            printf '%s\n' "$item: $note" >> "$output/notes.txt"
            python3 -B scripts/m2_manual.py verify "$output/files" --profile messages > "$output/verification.log" 2>&1
            fail "message observation not passed; keep and report $output"
        end
    end
    read -l -P "Observed normal/narrow dimensions and enlarged font (approximate is OK): " geometry
    printf '%s\n' "$geometry" >> "$output/notes.txt"
    python3 -B scripts/m2_manual.py verify "$output/files" --profile messages > "$output/verification.log" 2>&1
    or fail "message observations or independent disk/config bytes failed; see $output/verification.log"
    echo "D1 manual observations and disk/config bytes verified; native performance/IME/GPU remain unmeasured."
else if test "$mode" = candidates
    begin
        uname -a
        rustc --version
        printf 'WAYLAND_DISPLAY=%s\nXDG_SESSION_TYPE=%s\nXDG_CURRENT_DESKTOP=%s\nLANG=%s\n' "$WAYLAND_DISPLAY" "$XDG_SESSION_TYPE" "$XDG_CURRENT_DESKTOP" "$LANG"
        fc-match monospace
        fc-match sans-serif
    end > "$output/environment.txt" 2>&1
    python3 -B scripts/m2_manual.py prepare "$output/files" --profile candidates
    or fail "candidate fixture generation failed"
    echo
    echo "Manual candidate / font / IME checks only: no OS key injection or performance matrix."
    echo "Follow docs/acceptance.md, candidate manual section, steps 1–6."
    echo "Use only these disposable files: $output/files"
    echo "On any problem stop the steps, close the test window and answer no; do not repair fixtures or retry."
    env XDG_CONFIG_HOME="$output/files/config" "$root/target/release/semacs" "$output/files/source.txt" > "$output/candidates.stdout.log" 2> "$output/candidates.stderr.log"
    or fail "candidate session exited abnormally; keep this output directory"
    for item in candidate-command candidate-files candidate-buffers candidate-layout-font candidate-minibuffer-ime candidate-body-ime
        read -l -P "$item: all corresponding observations passed? Type yes or no: " answer
        or set answer no
        printf '%s\t%s\n' "$item" "$answer" >> "$output/files/observations.tsv"
        if test "$answer" != yes
            read -l -P "Failure / unobserved details (optional): " note
            printf '%s\n' "$item: $note" >> "$output/notes.txt"
            python3 -B scripts/m2_manual.py verify "$output/files" --profile candidates > "$output/verification.log" 2>&1
            fail "candidate observation not passed; keep and report $output"
        end
    end
    read -l -P "Observed normal/narrow window dimensions and font appearance (approximate is OK): " geometry
    printf '%s\n' "$geometry" >> "$output/notes.txt"
    python3 -B scripts/m2_manual.py verify "$output/files" --profile candidates > "$output/verification.log" 2>&1
    or fail "candidate observations or independent disk-byte checks failed; see $output/verification.log"
    echo "Manual observations and disk bytes verified; performance remains unmeasured."
else
    python3 -B scripts/m2_manual.py prepare "$output/files"
    or fail "manual fixture generation failed"
    echo
    echo "Follow docs/acceptance.md, manual section. Only disposable files under:"
    echo "$output/files"
    echo "First window: keyboard file loop + body/minibuffer IME. Close with C-x C-c."
    env XDG_CONFIG_HOME="$output/files/config" "$root/target/release/semacs" "$output/files/alpha.txt" > "$output/keyboard.stdout.log" 2> "$output/keyboard.stderr.log"
    or fail "keyboard session exited abnormally"
    for item in keyboard-file-loop body-ime minibuffer-ime
        read -l -P "$item: all corresponding observations passed? Type yes or no: " answer
        printf '%s\t%s\n' "$item" "$answer" >> "$output/files/observations.tsv"
        if test "$answer" != yes
            python3 -B scripts/m2_manual.py verify "$output/files" > "$output/verification.log" 2>&1
            fail "manual observation not passed; see $output"
        end
    end
    echo "Second window: OS close cancel / reject / save / clean close; see docs/acceptance.md."
    env XDG_CONFIG_HOME="$output/files/config" "$root/target/release/semacs" "$output/files/close.txt" > "$output/close.stdout.log" 2> "$output/close.stderr.log"
    or fail "OS-close session exited abnormally"
    for item in os-close-cancel os-close-clean
        read -l -P "$item: all corresponding observations passed? Type yes or no: " answer
        printf '%s\t%s\n' "$item" "$answer" >> "$output/files/observations.tsv"
    end
    python3 -B scripts/m2_manual.py verify "$output/files"
    or fail "manual observations or independent disk-byte checks failed"
end
printf '%s\n' "$mode completed; review environment/CSV/logs and remaining coverage before acceptance" > "$output/COMPLETED.txt"
echo "Batch complete: $output"
echo "Send this path back for review. Success is not by itself full phase acceptance."
