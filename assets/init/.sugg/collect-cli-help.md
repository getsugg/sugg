# Collecting CLI Help Docs

A workflow for generating a complete offline help reference for any CLI tool, used as context when writing Sugg completion scripts.

## Workflow

1. **Run level-1** → collect the tool's own help and every top-level subcommand's `--help`
2. **Inspect** → identify which top-level commands list further subcommands
3. **Append level-2** → fill in the subcommand map and run the append script
4. **Done** → the output file is your complete reference

All intermediate files (scripts + output) go into `_tmp/`. Nothing is written to the workspace root.

---

## Step 1 — Collect top-level help (overwrite)

Save as `_tmp/run1.sh`, set `tool` and `cmds`, then run.

```bash
#!/bin/bash
set -e

tool="your-command"          # e.g. npm, git, bun
file="_tmp/${tool}_help"

mkdir -p "$(dirname "$file")"

cmds=(
    subcommand1
    subcommand2
    # copy from: your-command --help
)

echo "========== $tool HELP ==========" > "$file"
echo "Version: $($tool --version 2>/dev/null || echo 'unknown')" >> "$file"
echo "Generated: $(date)" >> "$file"
echo "" >> "$file"

echo "========== PART 0: $tool --help ==========" >> "$file"
$tool --help >> "$file" 2>&1
echo "" >> "$file"

echo "========== PART 1: subcommand --help ==========" >> "$file"
for cmd in "${cmds[@]}"; do
    echo "---------- $tool $cmd --help ----------" >> "$file"
    $tool $cmd --help >> "$file" 2>&1
    echo "" >> "$file"
done

echo "Level-1 help collected."
```

Run: `bash _tmp/run1.sh`

---

## Step 2 — Append level-2 help

Save as `_tmp/run2.sh`, fill in the `subs` map, then run. Output is appended to the same file.

```bash
#!/bin/bash
set -e

tool="your-command"
file="_tmp/${tool}_help"

mkdir -p "$(dirname "$file")"

declare -A subs
subs["parent1"]="child-a child-b"
subs["parent2"]="child-x child-y"
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

echo "Level-2 help appended."
```

Run: `bash _tmp/run2.sh`

For level-3, repeat the same pattern with one more loop level.

---

## Notes

- Copy the subcommand list directly from the `Commands:` section of `your-tool --help`.
- The append script is safe to re-run (it appends again); to start over, re-run Step 1 to overwrite.
- `--version` failures are non-fatal and won't abort the script.
