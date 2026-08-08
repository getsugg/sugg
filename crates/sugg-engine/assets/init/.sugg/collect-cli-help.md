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

Then send `_tmp/${tool}_help` to AI and ask:
> "For each help block in PART 1, determine if the command lists subcommands (in any format — `Commands:` section, a table, a usage line, or any listing of sub-commands). Output all commands that have their own subcommands as a bash associative array, e.g. `subs["container"]="run ls create exec"`. If none do, output `subs=()`."

Fill the result into `subs` in step 2 below. If the result is empty, skip to **Verify depth** below.

---

## Step N — Collect deeper levels (N ≥ 2)

For each level beyond the first, save as `_tmp/runN.sh`, fill in `tool` and `subs`, then run.

**Level 2** — save as `_tmp/run2.sh`:

```bash
#!/bin/bash
set -e

tool="your-command"
file="_tmp/${tool}_help"

declare -A subs
# paste AI output here
# e.g. subs["container"]="run ls create exec"

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

Then send `_tmp/${tool}_help` to AI and ask:
> "Examine every new help block in PART 2 (and any earlier PART not yet checked). For each one, determine if it lists subcommands (in any format). Output all that do as a bash associative array. The key is the full prefix used in the `----------` header, and the value is the subcommand list — e.g. `subs["docker container"]="run ls create"`. If none do, output `subs=()`."

If `subs` is not empty, create `_tmp/run3.sh` using the same pattern (PART 3, three nested loops for `cmd`/`sub`/`subsub`), run it, and repeat. If empty, skip to **Verify depth**.

---

## Notes

- The script appends to the file. To start over, re-run section **Initial** to overwrite.
- `--version` failures are non-fatal and won't abort the script.
- AI may miss subcommands at deeper levels — always run **Verify depth** before proceeding.
- At levels beyond 2, the subs key uses the full prefix (e.g. `subs["container run"]="subsub"`). Adjust the loop nesting in `runN.sh` accordingly: each extra level adds one more `for` loop.

---

## Verify depth — No skipped levels

Before writing the completion script, verify that no level was skipped. Every PART block in `_tmp/<tool>_help` that lists subcommands (in any format) must have those subcommands collected in a deeper PART.

Send `_tmp/<tool>_help` to AI and ask:
> "Examine every PART block in the file. For each one that lists subcommands (regardless of format — `Commands:` section, table, usage line, inline list, etc.), verify that every listed subcommand has its own `--help` block collected in a deeper PART. Report any commands whose subcommands were not collected."

Do not proceed to writing the completion script until all levels are complete.

---

## Verify — Check for omissions

After writing the completion script, send these two files to AI to check for missing subcommands:

1. `_tmp/<tool>_help` — the collected help reference
2. Your completion script (`<tool>.ts` or `<tool>/index.ts`)

AI will compare the subcommand lists and report what's missing.
