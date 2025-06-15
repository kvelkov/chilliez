#!/bin/bash

# Script to preview what files would be removed (dry run)
echo "🔍 DRY RUN: Previewing files that would be removed"
echo "=================================================="

echo "📁 Files in chilliez.git directory (6.5MB):"
git ls-files | grep "chilliez.git/" | head -10
echo ""

echo "📊 Impact analysis:"
echo "Current repository size: $(du -sh .git | cut -f1)"
echo "Files that would be removed:"

# Count files in chilliez.git
chilliez_count=$(git ls-files | grep "chilliez.git/" | wc -l)
echo "   chilliez.git/: $chilliez_count files (~6.5MB)"

# Check for other redundant patterns
PATTERNS=("*.DS_Store" "target/" "node_modules/" "venv/" "*.log" "*.vsix")
for pattern in "${PATTERNS[@]}"; do
    count=$(git ls-files | grep "$pattern" | wc -l)
    if [ "$count" -gt 0 ]; then
        echo "   $pattern: $count files"
    fi
done

echo ""
echo "🎯 Recommendation:"
if [ "$chilliez_count" -gt 0 ]; then
    echo "✅ PROCEED with cleanup - chilliez.git directory should not be in git"
    echo "   This will free up ~6.5MB and clean up your repository"
else
    echo "ℹ️  No major issues found - repository looks clean"
fi

echo ""
echo "To proceed with cleanup: ./scripts/cleanup_large_files.sh"
