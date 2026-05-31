_sugg_completion() {
    local -a args=("${(@)words[1,CURRENT]}")
    local cmd="${args[1]}"
    if [[ -n "${aliases[$cmd]}" ]]; then
        local real_cmd="${aliases[$cmd]%% *}"
        args[1]="$real_cmd"
    fi
    local IFS=$'\n'
    local -a output=($("{{SUGG_BIN}}" complete zsh -- "${args[@]}"))

    local -a messages=()
    local -a displays=() values=()

    for line in "${output[@]}"; do
        if [[ "$line" == "__msg__"$'\t'* ]]; then
            messages+=("${line#*$'\t'}")
        else
            local val="${line%%$'\t'*}"
            local rest="${line#*$'\t'}"
            local disp="${rest%%$'\t'*}"
            local desc="${rest#*$'\t'}"
            local disp_safe="${disp//:/\\:}"
            local desc_safe="${desc//:/\\:}"
            if [[ -n "$desc_safe" ]]; then
                displays+=("$disp_safe:$desc_safe")
            else
                displays+=("$disp_safe")
            fi
            values+=("$val")
        fi
    done

    if (( ${#displays} > 0 )); then
        _describe -t completions 'completions' displays values -Q -S ''
    fi

    if (( ${#messages} > 0 )); then
        _message -r "${(j:; :)messages}"
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
