#!/bin/bash

# Script to check for large files in git repository
# Usage: ./check_large_files.sh [size_threshold]

THRESHOLD=${1:-1M}  # Default to 1MB, can override with argument

echo "🔍 Checking for files larger than $THRESHOLD in git repository..."
echo "========================================================"

# Check files currently tracked by git
echo "📊 TRACKED FILES (largest first):"
git ls-files | xargs -I {} sh -c 'test -f "{}" && du -h "{}"' | sort -hr | head -20

echo ""
echo "🚨 LARGE FILES IN GIT (larger than $THRESHOLD):"
git ls-files | xargs -I {} sh -c 'test -f "{}" && find "{}" -size +'"$THRESHOLD"' -exec du -h {} \;' 2>/dev/null | sort -hr

echo ""
echo "📦 BINARY/COMPILED FILES IN GIT:"
git ls-files | grep -E '\.(pack|gz|zip|tar|bin|exe|dll|so|dylib|vsix|dSYM|a|o)$' | xargs -I {} sh -c 'test -f "{}" && du -h "{}"' 2>/dev/null | sort -hr

echo ""
echo "🗂️ DIRECTORIES THAT SHOULDN'T BE IN GIT:"
# Check if these directories are tracked by git
for dir in "target" "node_modules" "venv" "__pycache__" ".DS_Store" "*.dSYM"; do
    if git ls-files | grep -q "$dir"; then
        echo "⚠️  Found $dir in git:"
        git ls-files | grep "$dir" | head -5
        if [ $(git ls-files | grep "$dir" | wc -l) -gt 5 ]; then
            echo "   ... and $(( $(git ls-files | grep "$dir" | wc -l) - 5 )) more files"
        fi
    fi
done

echo ""
echo "📈 REPOSITORY SIZE ANALYSIS:"
echo "Total files tracked: $(git ls-files | wc -l)"
echo "Repository size: $(du -sh .git 2>/dev/null || echo 'Unable to calculate')"

echo ""
echo "💡 RECOMMENDATIONS:"
echo "1. Remove large binary files: git rm <filename>"
echo "2. Add patterns to .gitignore to prevent future commits"
echo "3. Use git filter-branch or BFG to remove from history if needed"
echo "4. Consider using Git LFS for legitimate large files"
