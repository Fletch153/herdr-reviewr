set -uo pipefail
SOCK="jexplore$$"
export TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"
printf 'a1\na2\na3\n' > "$REPO/src/alpha.txt"
printf 'b1\nb2\nb3\nb4\n' > "$REPO/src/beta.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'ALPHA_CHANGE\n' >> "$REPO/src/alpha.txt"
printf 'BETA_CHANGE\n' >> "$REPO/src/beta.txt"
tui_start
wait_for "alpha.txt"
keys 2; sleep 0.5
wait_for "ALPHA_CHANGE"
keys Tab; sleep 0.4
esc
keys -l ':e src/beta.txt'; keys Enter; sleep 0.8
echo "=== FULL FRAME after jump (right pane only, cols 140+) ==="
frame | cut -c140- | sed -n '1,15p'
keys Tab; keys q; sleep 0.3; keys y 2>/dev/null || true
