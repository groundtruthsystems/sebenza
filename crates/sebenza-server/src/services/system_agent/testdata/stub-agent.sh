#!/bin/sh
# Test-only stand-in for `claude -p --output-format stream-json`. It never
# calls a model: it records how it was started, then replays a recorded,
# scrubbed claude stream from fixtures/. Behaviour comes from files beside the
# copy a test makes, because the system agent gives its child a clean env:
#   stub.mode   ok | slow | hang | exit | fixture:<name>   (default ok)
#   stub.delay  seconds `slow` sleeps before replying       (default 0.3)
# `ok` and `slow` reply with fixtures/<job kind>.jsonl, the kind read from the
# prompt's `JOB-KIND:` line.
dir=$(cd "$(dirname "$0")" && pwd)
log="$dir/log"
mkdir -p "$log"
n=$$
prompt=$(cat)
printf '%s' "$prompt" > "$log/prompt.$n"
for a in "$@"; do printf '%s\n' "$a"; done > "$log/argv.$n"
env > "$log/env.$n"
pwd > "$log/cwd.$n"
ls -A > "$log/cwdlist.$n"
touch "$log/running.$n"
ls "$log" | grep -c '^running\.' >> "$log/peaks"
kind=$(printf '%s\n' "$prompt" | sed -n 's/^JOB-KIND: //p' | head -n 1)
echo "$kind $n" >> "$log/order"
mode=$(cat "$dir/stub.mode" 2>/dev/null || echo ok)
finish() { rm -f "$log/running.$n"; }
case "$mode" in
  hang)
    sleep 300 &
    echo $! > "$log/grandchild.$n"
    wait
    ;;
  exit)
    echo "stub agent failing on purpose" >&2
    finish
    exit 3
    ;;
  slow)
    sleep "$(cat "$dir/stub.delay" 2>/dev/null || echo 0.3)"
    cat "$dir/fixtures/$kind.jsonl"
    ;;
  fixture:*)
    cat "$dir/fixtures/${mode#fixture:}.jsonl"
    ;;
  *)
    cat "$dir/fixtures/$kind.jsonl"
    ;;
esac
finish
