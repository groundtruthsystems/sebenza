#!/bin/sh
# Test-only stand-in for claude, codex and grok in role mode. It never calls a
# model and is never written at test time (so no ETXTBSY races). The child gets
# a clean env, so its behaviour comes from lines in the prompt:
#   STUB-LOG: <dir>     record argv, env and cwd there
#   STUB-MODE: reply | hang | exit | write | slow   (default reply)
#   STUB-REPLY: <file>  the assistant's final text
#   STUB-REPAIR: <file> the reply to a repair retry
#   STUB-REPLY-<role>[-<round>]: <file>  per role (and review round), from the
#                       daemon's ROLE: and ROUND: header lines
# The CLI flavour comes from argv: codex starts with `exec`; claude is
# `-p --verbose` with the prompt on stdin; grok is `-p <prompt>`.
if [ "$1" = "exec" ]; then
  flavour=codex
  for a in "$@"; do prompt="$a"; done
elif [ "$1" = "-p" ] && [ "$2" = "--verbose" ]; then
  flavour=claude
  prompt=$(cat)
else
  flavour=grok
  prompt="$2"
fi
field() { printf '%s\n' "$prompt" | sed -n "s/^$1: //p" | head -n 1; }
log=$(field STUB-LOG)
mode=$(field STUB-MODE)
[ -n "$(field STUB-HANG-ROLE)" ] && [ "$(field STUB-HANG-ROLE)" = "$(field ROLE)" ] && mode=hang
role=$(field ROLE)
round=$(field ROUND)
# Most specific first: per role and round, per role, then the default.
mode_line=$(field MODE)
reply=""
[ -n "$mode_line" ] && reply=$(field "STUB-REPLY-$role-$mode_line")
[ -z "$reply" ] && reply=$(field "STUB-REPLY-$role-$round")
[ -z "$reply" ] && reply=$(field "STUB-REPLY-$role")
[ -z "$reply" ] && reply=$(field STUB-REPLY)
# A repair retry (the runner's second attempt) answers from STUB-REPAIR.
case "$prompt" in
  *"could not be used"*) r=$(field STUB-REPAIR); [ -n "$r" ] && reply="$r" ;;
esac
if [ -n "$log" ]; then
  mkdir -p "$log"
  for a in "$@"; do printf '%s\n' "$a"; done > "$log/argv.$$"
  env > "$log/env.$$"
  pwd > "$log/cwd.$$"
  find . -type f | sort > "$log/files.$$"
  touch "$log/running.$$"
  ls "$log" | grep -c '^running\.' >> "$log/peaks"
  echo "$role $round$mode_line" >> "$log/order"
fi
finish() { if [ -n "$log" ]; then rm -f "$log/running.$$"; fi; }
# STUB-TOUCH: <path> — change a file outside the cwd (a tampering reviewer).
touch_path=$(field STUB-TOUCH)
[ -n "$touch_path" ] && echo tampered >> "$touch_path"
case "$mode" in
  hang) sleep 300 & wait ;;
  exit) finish; echo "stub failing on purpose" >&2; exit 3 ;;
  write) touch ./written-by-agent ;;
  slow) sleep 0.4 ;;
esac
text=$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read()))' "$reply")
if [ "$flavour" = "codex" ]; then
  printf '{"type":"thread.started","thread_id":"stub-thread"}\n'
  printf '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":%s}}\n' "$text"
  printf '{"type":"turn.completed","usage":{}}\n'
else
  printf '{"type":"system","subtype":"init","session_id":"stub-session"}\n'
  printf '{"type":"result","subtype":"success","is_error":false,"result":%s,"session_id":"stub-session"}\n' "$text"
fi
finish
exit 0
