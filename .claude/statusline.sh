#!/usr/bin/env bash
set -uo pipefail

input=$(cat)

have_jq=0
command -v jq >/dev/null 2>&1 && have_jq=1

jget() {
  [[ $have_jq -eq 1 ]] || return 0
  printf '%s' "$input" | jq -r "$1 // empty" 2>/dev/null
}

# ---- Fields from Claude Code JSON ----
model=$(jget '.model.display_name');           [[ -z "$model" ]] && model="Claude"
cwd=$(jget '.workspace.current_dir')
[[ -z "$cwd" ]] && cwd=$(jget '.cwd')
[[ -z "$cwd" ]] && cwd="$PWD"
transcript=$(jget '.transcript_path')
cost_usd=$(jget '.cost.total_cost_usd');        cost_usd=${cost_usd:-0}
api_ms=$(jget '.cost.total_api_duration_ms');   api_ms=${api_ms:-0}
lines_added=$(jget '.cost.total_lines_added');  lines_added=${lines_added:-0}
lines_removed=$(jget '.cost.total_lines_removed'); lines_removed=${lines_removed:-0}
exceeds_200k=$(jget '.exceeds_200k_tokens')
output_style=$(jget '.output_style.name')

# ---- Colors (256-color) ----
DIM=$'\033[38;5;244m'
FG=$'\033[38;5;252m'
MUTED=$'\033[38;5;240m'
ACCENT=$'\033[38;5;39m'
GREEN=$'\033[38;5;114m'
WARN=$'\033[38;5;214m'
CRIT=$'\033[38;5;203m'
BOLD=$'\033[1m'
RESET=$'\033[0m'
SEP="  ${MUTED}│${RESET}  "

pretty_path="${cwd/#$HOME/\~}"

# ---- Git: branch + dirty file count ----
branch=""; dirty_count=0
if git -C "$cwd" rev-parse --git-dir >/dev/null 2>&1; then
  branch=$(git -C "$cwd" branch --show-current 2>/dev/null)
  [[ -z "$branch" ]] && branch=$(git -C "$cwd" rev-parse --short HEAD 2>/dev/null)
  dirty_count=$(git -C "$cwd" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
  dirty_count=${dirty_count:-0}
fi

# ---- Context % + cache hit %, both from last usage block in transcript ----
ctx_display=""; cache_display=""
if [[ $have_jq -eq 1 && -n "$transcript" && -f "$transcript" ]]; then
  read -r in_t cr_t cc_t ot_t <<< "$(tail -n 200 "$transcript" 2>/dev/null \
    | jq -rs '[.[] | select(.message.usage?) | .message.usage] | last
              | if . then
                  "\(.input_tokens // 0) \(.cache_read_input_tokens // 0) \(.cache_creation_input_tokens // 0) \(.output_tokens // 0)"
                else "0 0 0 0" end' 2>/dev/null)"
  in_t=${in_t:-0}; cr_t=${cr_t:-0}; cc_t=${cc_t:-0}; ot_t=${ot_t:-0}
  total_in=$(( in_t + cr_t + cc_t ))
  ctx_tokens=$(( total_in + ot_t ))

  if (( ctx_tokens > 0 )); then
    ctx_max=200000
    [[ "$model" == *"1M"* || "$exceeds_200k" == "true" ]] && ctx_max=1000000
    pct=$(( ctx_tokens * 100 / ctx_max ))
    (( pct > 100 )) && pct=100
    if   (( pct >= 85 )); then col="$CRIT"
    elif (( pct >= 60 )); then col="$WARN"
    else                       col="$GREEN"
    fi
    bar_width=10
    filled=$(( pct * bar_width / 100 ))
    (( filled > bar_width )) && filled=$bar_width
    empty=$(( bar_width - filled ))
    bar=""; for ((i=0;i<filled;i++)); do bar+="━"; done
    bar_empty=""; for ((i=0;i<empty;i++)); do bar_empty+="─"; done
    ctx_display="${DIM}ctx${RESET} ${col}${bar}${MUTED}${bar_empty}${RESET} ${col}${pct}%${RESET}"
  fi

  if (( total_in > 0 )); then
    cache_pct=$(( cr_t * 100 / total_in ))
    if   (( cache_pct >= 70 )); then ccol="$GREEN"
    elif (( cache_pct >= 30 )); then ccol="$WARN"
    else                              ccol="$DIM"
    fi
    cache_display="${DIM}cache${RESET} ${ccol}${cache_pct}%${RESET}"
  fi
fi

# ---- Cost / api time ----
cost_fmt=$(awk -v c="$cost_usd" 'BEGIN { printf "$%.2f", c }')

fmt_ms() {
  local ms=$1 s=$(( $1 / 1000 ))
  if   (( s < 60 ));   then printf '%ds' "$s"
  elif (( s < 3600 )); then printf '%dm %02ds' $((s/60)) $((s%60))
  else                      printf '%dh %02dm' $((s/3600)) $(((s%3600)/60))
  fi
}
api_fmt=$(fmt_ms "$api_ms")

# ---- Git branch + dirty ----
branch_fmt=""
if [[ -n "$branch" ]]; then
  branch_fmt="${DIM}${branch}${RESET}"
  if (( dirty_count > 0 )); then
    branch_fmt="${branch_fmt} ${WARN}±${dirty_count}${RESET}"
  fi
fi

# ---- Session lines changed ----
session_lines=""
if (( lines_added > 0 || lines_removed > 0 )); then
  session_lines="${GREEN}+${lines_added}${RESET} ${CRIT}-${lines_removed}${RESET}"
fi

# ---- Output style (only if non-default) ----
style_fmt=""
if [[ -n "$output_style" && "$output_style" != "default" ]]; then
  style_fmt="${DIM}${output_style}${RESET}"
fi

# ---- Assemble ----
parts=()
parts+=("${ACCENT}${BOLD}${model}${RESET}")
parts+=("${FG}${pretty_path}${RESET}")
[[ -n "$branch_fmt" ]]      && parts+=("$branch_fmt")
[[ -n "$ctx_display" ]]     && parts+=("$ctx_display")
[[ -n "$cache_display" ]]   && parts+=("$cache_display")
[[ -n "$session_lines" ]]   && parts+=("$session_lines")
parts+=("${GREEN}${cost_fmt}${RESET}")
parts+=("${DIM}api${RESET} ${api_fmt}")
[[ -n "$style_fmt" ]]       && parts+=("$style_fmt")

out=""
for p in "${parts[@]}"; do
  [[ -n "$out" ]] && out+="$SEP"
  out+="$p"
done
printf '%b\n' "$out"
