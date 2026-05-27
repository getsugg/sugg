_sugg_completion() {
    local -a args=("${(@)words[1,CURRENT]}")
    local cmd="${args[1]}"
    if [[ -n "${aliases[$cmd]}" ]]; then
        local real_cmd="${aliases[$cmd]%% *}"
        args[1]="$real_cmd"
    fi
    local IFS=$'\n'
    local -a output=($("{{SUGG_BIN}}" complete zsh -- "${args[@]}"))
    local -a matches
    for line in "${output[@]}"; do
        local val="${line%%$'\t'*}"
        local desc="${line#*$'\t'}"
        val="${val//:/\\:}"
        desc="${desc//:/\\:}"
        if [[ -n "$desc" && "$desc" != "$val" ]]; then
            matches+=("$val:$desc")
        else
            matches+=("$val")
        fi
    done
    if (( ${#matches} > 0 )); then
        _describe -t completions 'completions' matches -Q -S ''
    fi
}

sugg_cmds=($("{{SUGG_BIN}}" commands))

for cmd in "${sugg_cmds[@]}"; do
    compdef _sugg_completion "$cmd"
done

for al in ${(k)aliases}; do
    target="${aliases[$al]%% *}"
    if (( ${sugg_cmds[(Ie)$target]} )); then
        compdef _sugg_completion "$al"
    fi
done
