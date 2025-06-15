#!/bin/bash

# Script to remove large redundant files from git repository
# IMPORTANT: Run this on a backup branch first!

echo "🧹 CLEANING UP REDUNDANT LARGE FILES"
echo "======================================"

# Backup current state
BACKUP_BRANCH="backup-before-cleanup-$(date +%Y%m%d-%H%M%S)"
echo "📦 Creating backup branch: $BACKUP_BRANCH"
git checkout -b "$BACKUP_BRANCH"
git checkout main

echo ""
echo "🗑️ REMOVING REDUNDANT FILES:"

# Remove the accidentally committed git directory
if git ls-files | grep -q "chilliez.git/"; then
    echo "   Removing accidentally committed chilliez.git directory..."
    git rm -r chilliez.git/
    echo "   ✅ chilliez.git directory removed from git"
fi

# Check for and remove other common redundant files
REDUNDANT_PATTERNS=(
    "*.DS_Store"
    "target/"
    "node_modules/"
    "venv/"
    "*.log"
    "*.tmp"
    "*.cache"
    "__pycache__/"
    "*.pyc"
    "*.pyo"
    "*.vsix"
    "*.dSYM/"
)

for pattern in "${REDUNDANT_PATTERNS[@]}"; do
    if git ls-files | grep -q "$pattern"; then
        echo "   Removing $pattern files..."
        git ls-files | grep "$pattern" | xargs git rm -r --ignore-unmatch
    fi
done

echo ""
echo "📝 UPDATING .gitignore:"

# Add comprehensive gitignore patterns
cat >> .gitignore << 'EOF'

# Additional ignore patterns added by cleanup script
*.DS_Store
.DS_Store*

# Git directories (prevent accidental commits)
*.git/
.git/

# Build and cache directories
target/
build/
dist/
*.dSYM/

# Python
venv/
__pycache__/
*.pyc
*.pyo
*.pyd

# Node.js
node_modules/
npm-debug.log*

# VS Code
*.vsix
.vscode/settings.json.bak

# Logs and temporary files
*.log
*.tmp
*.cache
*.temp

# OS files
Thumbs.db
.Spotlight-V100
.Trashes
EOF

echo "   ✅ Updated .gitignore with comprehensive patterns"

echo ""
echo "📊 RESULTS:"
echo "Files removed from git tracking:"
git status --porcelain | grep "^D " | wc -l | xargs echo "   Deleted files:"

echo ""
echo "📋 NEXT STEPS:"
echo "1. Review the changes: git status"
echo "2. Commit the cleanup: git commit -m 'Remove large redundant files and update .gitignore'"
echo "3. Push changes: git push"
echo "4. Optional: Clean git history with BFG if needed"
echo ""
echo "⚠️  IMPORTANT: If you want to completely remove these files from git history"
echo "   (to reduce repository size), consider using BFG Repo-Cleaner:"
echo "   https://rtyley.github.io/bfg-repo-cleaner/"
echo ""
echo "💾 Backup branch created: $BACKUP_BRANCH"
echo "   You can restore with: git checkout $BACKUP_BRANCH"
