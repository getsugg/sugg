let complete_completer = {|spans|
    let expanded_alias = (scope aliases | where name == $spans.0 | $in.0?.expansion?)

    let processed_spans = (if $expanded_alias != null {
        $spans | skip 1 | prepend ($expanded_alias | split row " " | take 1)
    } else {
        $spans
    })

    let output = (try {
        ^'{{SUGG_BIN}}' complete nushell -- ...$processed_spans | from json
    } catch {
        []
    })

    if ($output | is-empty) { null } else { $output }
}

$env.config.completions = ($env.config.completions? | default {} | merge {
    external: ($env.config.completions.external? | default {} | merge {
        enable: true,
        completer: $complete_completer
    })
})
