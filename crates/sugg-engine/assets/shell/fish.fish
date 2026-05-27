function _sugg_completion
    set -l tokens (commandline -opc) (commandline -ct)
    set -l sugg_cmds ("{{SUGG_BIN}}" commands)
    
    if not contains $tokens[1] $sugg_cmds
        set -l wraps (functions --wraps $tokens[1] 2>/dev/null)
        for cmd in $sugg_cmds
            if contains -- $cmd $wraps
                set tokens[1] $cmd
                break
            end
        end
    end
    
    "{{SUGG_BIN}}" complete fish -- $tokens
end

set -l sugg_cmds ("{{SUGG_BIN}}" commands)
for cmd in $sugg_cmds
    complete -c $cmd -f -k -a "(_sugg_completion)"
end
