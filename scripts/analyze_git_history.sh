#!/bin/bash

# Script to find large files in git history
# This helps identify files that were committed and later removed but still bloat the .git directory

echo "🔍 ANALYZING GIT HISTORY FOR LARGE FILES"
echo "========================================="

echo "📊 Largest files in git history (may include deleted files):"
git rev-list --objects --all \
  | git cat-file --batch-check='%(objecttype) %(objectname) %(objectsize) %(rest)' \
  | awk '/^blob/ {print substr($0,6)}' \
  | sort --numeric-sort --key=2 \
  | tail -20 \
  | while read objectname size path; do
    # Convert size to human readable
    if [ "$size" -gt 1048576 ]; then
      size_human="$(echo "scale=1; $size/1048576" | bc)M"
    elif [ "$size" -gt 1024 ]; then
      size_human="$(echo "scale=1; $size/1024" | bc)K"
    else
      size_human="${size}B"
    fi
    echo "$size_human $path"
  done

echo ""
echo "🗂️ Current .git directory breakdown:"
du -sh .git/objects/* 2>/dev/null | sort -hr

echo ""
echo "📈 Repository statistics:"
echo "Total objects: $(git count-objects -v | grep '^count' | cut -d' ' -f2)"
echo "Packed objects: $(git count-objects -v | grep '^in-pack' | cut -d' ' -f2)"
echo "Pack size: $(git count-objects -v | grep '^size-pack' | cut -d' ' -f2) KB"

echo ""
echo "💡 To reduce repository size further:"
echo "1. git gc --aggressive --prune=now"
echo "2. Use BFG Repo-Cleaner for large files in history"
echo "3. Consider git filter-branch to rewrite history"
