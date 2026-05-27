Set-PSReadLineKeyHandler -Key Tab -Function MenuComplete

$baseCommands = & '{{SUGG_BIN}}' commands

$allNames = @()

$exts = if ($env:PATHEXT) { 
    ($env:PATHEXT -split ';').Where({ $_ }) 
} else { 
    @() 
}

foreach ($cmd in $baseCommands) {
    $allNames += $cmd
    foreach ($ext in $exts) {
        $allNames += "$cmd$($ext.ToLower())"
    }
}

if ($allNames.Count -gt 0) {
    Register-ArgumentCompleter -Native -CommandName $allNames -ScriptBlock {
        param($wordToComplete, $commandAst, $cursorPosition)

        [Console]::OutputEncoding = [System.Text.Encoding]::UTF8

        try {
            $rawText = $commandAst.Extent.Text
            $offset  = $cursorPosition - $commandAst.Extent.StartOffset

            if ($offset -ge 0 -and $offset -lt $rawText.Length) {
                $line = $rawText.Substring(0, $offset)
            } elseif ($offset -gt $rawText.Length) {
                $line = $rawText.PadRight($offset, ' ')
            } else {
                $line = $rawText
            }

            $spans = @(-split $line)
            if ($line -match '\s+$') {
                $spans += ""
            }

            if ($spans.Count -gt 0) {
                $aliasInfo = Get-Command $spans[0] -CommandType Alias -ErrorAction Ignore
                if ($aliasInfo) {
                    $spans[0] = $aliasInfo.ResolvedCommandName
                }
            }

            $raw = & '{{SUGG_BIN}}' complete powershell -- $spans 2>&1
            if (-not $raw) { 
                return 
            }

            $items = @($raw | ConvertFrom-Json -ErrorAction Stop)
            
            $results = [System.Collections.Generic.List[System.Management.Automation.CompletionResult]]::new()
            foreach ($item in $items) {
                $completionText = if ($item.value) { $item.value } else { $item.display_override }
                if (-not $completionText) { continue }
                
                $display = if ($item.display_override) { $item.display_override } else { $completionText }
                $desc    = if ([string]::IsNullOrWhiteSpace($item.description)) { $display } else { $item.description }

                $resultObj = [System.Management.Automation.CompletionResult]::new(
                    $completionText,
                    $display,
                    [System.Management.Automation.CompletionResultType]::ParameterValue,
                    $desc
                )
                $results.Add($resultObj)
            }

            return $results
        } catch {
            return
        }
    }
}
