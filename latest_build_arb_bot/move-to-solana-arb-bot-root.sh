#!/bin/bash
set -e

# This script makes /src/rust/solana-arb-bot the new project root.
# Run from the old root (where Cargo.toml and README.md live).

echo "==> PREPARATION"

NEWROOT="src/rust/solana-arb-bot"
if [[ ! -d "$NEWROOT" ]]; then
  echo "Error: $NEWROOT does not exist!"
  exit 1
fi

# Warn and prompt for confirmation
read -p "Are you SURE you want to make $NEWROOT the new repo/project root? (y/n): " yn
case "$yn" in
    [Yy]* ) ;;
    * ) echo "Aborted."; exit 1;;
esac

cd "$NEWROOT"

# Stop if root folder already has some of these files
for F in Cargo.toml Cargo.lock README.md .gitignore; do
  if [[ -e "../../$F" ]]; then
    echo "Moving ../../$F to ./ (if not already present in $NEWROOT)"
    if [[ -e "$F" ]]; then
      echo "  $F exists in new root; skipping overwrite."
    else
      mv "../../$F" .
    fi
  fi
done

# Move core Rust subdirs/files, if not present
for DIR in src tests; do
  if [[ -d "../../$DIR" ]]; then
    echo "Moving ../../$DIR to ./ (if not already present)"
    if [[ -d "$DIR" ]]; then
      echo "  $DIR exists in new root; skipping overwrite."
    else
      mv "../../$DIR" .
    fi
  fi
done

# Optionally migrate docs, config, and deployment files:
for D in docs deploy config.json README.md; do
  [[ -e "../../$D" ]] && echo "Consider moving or copying ../../$D as needed."
done

# Cleanup (if repo is cleaned after)
echo "==> New project root is now $NEWROOT"
echo "Update VS Code/workspace/project settings to start in this directory."
echo "You can now run:"
echo "   cd $(pwd)"
echo "   cargo build"
echo "   cargo test"

# README Template: overwrite with migration note
cat <<EOL > README.md
# solana-arb-bot (Project Root Migrated)

This folder is now the root for all Rust bot development.

## Usage
\`\`\`bash
cargo build
cargo test
\`\`\`

- All future development, CI/CD, and code reviews should happen here.
- If you were using a workspace, this structure is now single-crate.

**Old project files and unrelated folders remain in ../../ for backup. Clean up manually if you wish.**

EOL
echo "README.md updated to reflect new structure."