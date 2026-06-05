# Collecting CLI Help Docs

A workflow for generating a complete offline help reference for any CLI tool, used as context when writing Sugg completion scripts.

## Workflow (AI-driven recursion)

Each level follows the same cycle — AI identifies which commands have subcommands, you run the script, repeat:

```
Set tool → run script → send output to AI → AI tells you next subs → update script → run → ...
                                            ↓
                                    Done when AI says
                                    no more subcommands
```

All intermediate files go into `_tmp/`. Nothing is written to the workspace root.

---

## Initial — Collect root `--help`

Save as `_tmp/run0.sh`, set `tool`, then run.

```bash
#!/bin/bash
set -e

tool="your-command"          # e.g. npm, git, bun
file="_tmp/${tool}_help"

mkdir -p "$(dirname "$file")"

echo "========== $tool HELP ==========" > "$file"
echo "Version: $($tool --version 2>/dev/null || echo 'unknown')" >> "$file"
echo "Generated: $(date)" >> "$file"
echo "" >> "$file"

echo "========== PART 0: $tool --help ==========" >> "$file"
$tool --help >> "$file" 2>&1
echo "" >> "$file"
```

Run: `bash _tmp/run0.sh`

Then send `_tmp/${tool}_help` to AI and ask:
> "List all top-level subcommands of `${tool}` from the help above. Output as a bash array, e.g. `cmds=(commit push pull)`"

Fill the result into `cmds` in step 1 below.

---

## Step 1 — Collect subcommand `--help`

Save as `_tmp/run1.sh`, fill in `tool` and `cmds`, then run.

```bash
#!/bin/bash
set -e

tool="your-command"
file="_tmp/${tool}_help"

cmds=(
    # paste AI output here
)

echo "========== PART 1: subcommand --help ==========" >> "$file"
for cmd in "${cmds[@]}"; do
    echo "---------- $tool $cmd --help ----------" >> "$file"
    $tool $cmd --help >> "$file" 2>&1
    echo "" >> "$file"
done
```

Run: `bash _tmp/run1.sh`

Then send the updated `_tmp/${tool}_help` to AI and ask:
> "Which of these subcommands have further subcommands? Output as a bash associative array, e.g. `subs["parent"]="child1 child2"`"

Fill the result into `subs` in step 2 below.

---

## Step 2 (and beyond) — Collect deeper levels

Save as `_tmp/run2.sh`, fill in `tool` and `subs`, then run. Append further levels by repeating the same pattern.

```bash
#!/bin/bash
set -e

tool="your-command"
file="_tmp/${tool}_help"

declare -A subs
# paste AI output here
# e.g. subs["config"]="set get delete list"

echo "" >> "$file"
echo "========== PART 2: level-2 subcommands --help ==========" >> "$file"

for cmd in "${!subs[@]}"; do
    for sub in ${subs[$cmd]}; do
        echo "---------- $tool $cmd $sub --help ----------" >> "$file"
        $tool $cmd $sub --help >> "$file" 2>&1
        echo "" >> "$file"
    done
done
```

Run: `bash _tmp/run2.sh`

Repeat: send the updated help to AI, ask for the next level's subcommands, create `run3.sh`, run it. Keep going until AI says there are no more subcommands.

---

## Notes

- The script appends to the file. To start over, re-run section **Initial** to overwrite.
- `--version` failures are non-fatal and won't abort the script.
- For deeper levels, just repeat the step 2 pattern with one more loop level.
- AI never misses a subcommand — it reads the raw `Commands:` section. Trust its output over manual copying.

---

## Verify — Check for omissions

After writing the completion script, send these two files to AI to check for missing subcommands:

1. `_tmp/<tool>_help` — the collected help reference
2. Your completion script (`<tool>.ts` or `<tool>/index.ts`)

AI will compare the subcommand lists and report what's missing.
