#!/usr/bin/env fish
# Host-only acceptance; never falls back to software rendering.
function fail
    echo "STOP: $argv" >&2
    exit 1
end

if test (count $argv) -ne 3; or not contains -- $argv[1] perf display-perf ime manual open-diagnostic; or test "$argv[3]" != --consent
    echo "Usage: fish scripts/acceptance.fish <perf|display-perf|ime|manual|open-diagnostic> NEW_OUTPUT_DIRECTORY --consent" >&2
    exit 2
end
set -l mode $argv[1]
set -l root (path resolve (path dirname (status filename))/..)
cd "$root"; or fail "cannot enter repository"
set -l output (path resolve "$argv[2]")
test -n "$WAYLAND_DISPLAY"; or fail "run from the actual Wayland host terminal"
test -d /dev/dri; or fail "no /dev/dri; hardware acceptance cannot run here"
for binary in cargo python3 fc-match
    command -q $binary; or fail "missing $binary"
end
if test "$mode" = ime
    for binary in hyprctl wtype fcitx5-remote
        command -q $binary; or fail "missing $binary"
    end
    test -n "$HYPRLAND_INSTANCE_SIGNATURE"; or fail "IME automation requires Hyprland"
end
test ! -e "$output"; or fail "output must not exist: $output"
set -l untracked (git ls-files --others --exclude-standard -z | string split0)
python3 -c 'import pathlib,sys; pathlib.Path(sys.argv[1]).mkdir(parents=True, exist_ok=False)' "$output"
or fail "cannot create output"
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
cargo build --release --bin semacs --example perf_native --features native-profiler --target-dir "$root/target" > "$output/build.log" 2>&1
or fail "build failed; see $output/build.log"

if test "$mode" = open-diagnostic
    echo "Diagnostic only: one mixed-file open. No other shapes, editing or OS key injection."
    echo "Existing budgets still apply; a STOP is expected if the slow frame recurs."
    echo "Do not type in, resize or close the probe; send the output directory even on failure."
    python3 -B scripts/perf_sample.py "$output/sample.txt"
    or fail "fixture generation failed"
    python3 -B scripts/perf_native.py "$output/sample.txt" "$output/open-diagnostic" --consent --m2-open-diagnostic --sample mixed --runs 1
    or fail "single-open diagnostic stopped; keep CSV / frames.csv / views.csv / preparation.csv / diagnostic.json and report this directory"
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
else
    python3 -B scripts/m2_manual.py prepare "$output/files"
    or fail "manual fixture generation failed"
    echo
    echo "Follow docs/acceptance.md, manual section. Only disposable files under:"
    echo "$output/files"
    echo "First window: keyboard file loop + body/minibuffer IME. Close with C-x C-c."
    "$root/target/release/semacs" "$output/files/alpha.txt" > "$output/keyboard.stdout.log" 2> "$output/keyboard.stderr.log"
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
    "$root/target/release/semacs" "$output/files/close.txt" > "$output/close.stdout.log" 2> "$output/close.stderr.log"
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
