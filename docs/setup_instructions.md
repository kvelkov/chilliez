# Python Virtual Environment & Cargo Setup (macOS, Python-only)

This guide explains how to create and use a Python virtual environment, install dependencies, set up Cargo (Rust’s package manager), navigate project folders, and properly exit the virtual environment.  
**Platform:** macOS  
**Languages:** Python, Rust

---

## 1. How to Start the Python Virtual Environment

If a virtual environment is already created (e.g., `venv/`):

```sh
source /Users/kiril/Documents/rhodes/src/rust/solana-arb-bot/venv/bin/activate
```
You should see `(venv)` in your terminal prompt.


## 2. How to Install Python Virtual Environment (`venv`) (if not yet created)

Create a virtual environment with Python (ensure you have Python 3.6+):

```sh
cd /Users/kiril/Documents/rhodes/src/rust/solana-arb-bot
python3 -m venv venv
source venv/bin/activate
```

---

## 3. How to Install Python on Mac

If you do not have Python 3 installed, use Homebrew:

```sh
brew install python
```
MAC OS comes with Python 2.x by default. Make sure to use `python3` for all commands.

---

## 4. How to Install Cargo (Rust’s package manager)

You can install Rust and Cargo via the official installer (`rustup`):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
Follow the interactive prompts.  
After installation, restart your terminal and confirm with:

```sh
cargo --version
```

---

## 5. How to Move to a Specific Folder (in Terminal)

Use `cd` to "change directory". For example, to enter the project folder:

```sh
cd /Users/kiril/Documents/rhodes/src/rust/solana-arb-bot
```

---

## 6. How to Exit the Virtual Environment

Simply run:

```sh
deactivate
```
This returns you to your system Python environment.

---

## Quick Reference

- Move to project folder:  
  `cd /Users/kiril/Documents/rhodes/src/rust/solana-arb-bot`
- Activate venv:  
  `source venv/bin/activate`
- Install dependencies (while in venv, optional):  
  `pip install -r requirements.txt`
- Exit venv:  
  `deactivate`
- Install Rust/Cargo:  
  [rustup.rs](https://rustup.rs)
- Check tool versions:  
  `python3 --version`  
  `cargo --version`  

---

For any issues, consult the respective documentation or contact the project maintainer.