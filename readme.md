# clog-tui 🛡️📓

`clog-tui` is a secure, terminal-based note-keeping application — designed to give you a fast, keyboard-driven interface and complete privacy for all your data.

---

## ✨ Features

- **Terminal UI**: Navigate users, folders, and files entirely from the terminal.
- 🔐 **Fully Encrypted**:  
  - All metadata (folders, filenames, structure) is encrypted using your password.  
  - Every file uses a **unique encryption key and nonce** for maximum security.
- 🧳 **Portable**:  
  - All notes and structure are stored in a single file: `username.clog`  
  - Easy to transfer across systems — just copy the `.clog` file.
- 📁 **Virtual Filesystem**:  
  - Simulates a folder-file structure inside a single secure blob.
- ✏️ **Built-in Editor Support**:  
  - Uses your system's editor (Vim/Nano/etc.) to edit files securely.

---

## 🔒 Security Model

- The entire structure — including folders, files, and filenames — is encrypted with your password.
- Each file has its own:
  - Encryption key
  - Nonce (used once only)
- Without your password, nothing is visible — not even the folder names.

---

## 🚀 Getting Started

```
clog-tui
```

You’ll be guided to:
1. Create or select a user.
2. Enter a password (used for decryption).
3. Start managing your notes securely.

---

## 📦 Portable Storage

All your data is stored in a single `.clog` file inside your system's default application data directory:

**File Name Format:**
```
<username>.clog
```

**Default Locations:**

- **Linux**:  
  `~/.local/share/clog/<username>.clog`

- **macOS**:  
  `~/Library/Application Support/clog/<username>.clog`

- **Windows**:  
  `%APPDATA%\clog\<username>.clog`  
  (Typically `C:\Users\<user>\AppData\Roaming\clog\<username>.clog`)

You can back up or move this file to any system — the app will load it automatically.

---

## 💻 Platforms

Supports:
- Linux (x86_64, ARM)
- Windows (x86_64, ARM)
- macOS (Intel & Apple Silicon)

Prebuilt binaries are available in [Releases](https://github.com/Levi477/clog-tui/releases).

---

## 🛠️ Built With

- [`ratatui`](https://github.com/ratatui-org/ratatui) – Terminal UI library
- Rust 🦀 for performance and safety
- End-to-end encryption

---

## 📁 Example Structure (Virtual)

```
username.clog
└── (Encrypted)
    ├── 2025-05-28/
    │   ├── notes.txt
    │   └── log.md
    ├── personal/
    └── ideas/
```

You only see this after entering your password. All data is encrypted at rest.

---

## 📤 Transferring to Another Machine

Just copy the `.clog` file:
```
scp ~/.local/share/clog/username.clog user@host:~/backup/
```

Then install `clog-tui` on the new system and open it — you're all set.

---

## 🔓 Without Password?

Good luck.  
There is **no fallback**, **no plaintext**, and **no way in** without the correct password.

---

## 📄 License

MIT License — See `LICENSE` file for details.
