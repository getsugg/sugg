_sugg_completion() {
    local cur words cword
    if declare -F _get_comp_words_by_ref >/dev/null; then
        _get_comp_words_by_ref -n = cur words cword 2>/dev/null
    else
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
        cur=${words[cword]}
    fi
    local args=("${words[@]:0:$((cword + 1))}")
    local cmd="${args[0]}"
    local alias_def
    if alias_def=$(alias "$cmd" 2>/dev/null); then
        alias_def="${alias_def#alias }"
        alias_def="${alias_def#-- }"
        local alias_val="${alias_def#*=}"
        alias_val="${alias_val#\'}"; alias_val="${alias_val%\'}"
        alias_val="${alias_val#\"}"; alias_val="${alias_val%\"}"
        args[0]="${alias_val%% *}"
    fi
    local IFS=$'\n'
    local output=($("{{SUGG_BIN}}" complete bash -- "${args[@]}"))
    COMPREPLY=()
    for line in "${output[@]}"; do
        local val="${line%%$'\t'*}"
        if [[ -n "$val" ]]; then
            COMPREPLY+=("$val")
        fi
    done
    return 0
}

sugg_cmds=$("{{SUGG_BIN}}" commands)

for cmd in $sugg_cmds; do
    complete -o nospace -F _sugg_completion "$cmd"
done

while IFS= read -r alias_line; do
    alias_line="${alias_line#alias }"
    alias_line="${alias_line#-- }"
    name="${alias_line%%=*}"
    val="${alias_line#*=}"
    val="${val#\'}"; val="${val%\'}"
    val="${val#\"}"; val="${val%\"}"
    target="${val%% *}"
    case " $sugg_cmds " in
        *" $target "*) complete -o nospace -F _sugg_completion "$name" ;;
    esac
done < <(alias -p 2>/dev/null)
