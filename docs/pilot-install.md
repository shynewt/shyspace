# Installing the shyspace pilot

shyspace is in a small pilot. Install it once, as below; after that it keeps itself up to date.

## 1. Download

Open the latest release: <https://github.com/shynewt/shyspace/releases/latest>. Under **Assets**, take the file for your computer:

| Computer | File |
|---|---|
| Mac with Apple Silicon (M1 or newer) | `shyspace_…_aarch64.dmg` |
| Windows | `shyspace_…_x64-setup.exe` |
| Linux, any | `shyspace_…_amd64.AppImage` |
| Fedora | `shyspace-…x86_64.rpm` |
| Ubuntu or Debian | `shyspace_…_amd64.deb` |

Linux needs Ubuntu 24.04, Debian 13, Fedora 40 or newer. Intel Macs aren't supported yet.

## 2. Install

**Mac.** Open the `.dmg` and drag shyspace to Applications, then open it from there. The pilot isn't signed with an Apple identity, so macOS blocks it the first time: go to System Settings › Privacy & Security, find "shyspace was blocked", and choose **Open Anyway**. If macOS says the app is damaged, run this once in Terminal, then open it again:

```sh
xattr -cr /Applications/shyspace.app
```

**Windows.** Run the `-setup.exe`. Windows may say it protected your PC, because the pilot isn't signed: choose **More info**, then **Run anyway**. Use the setup file, not the `.msi`; installing both gives you two copies.

**Linux AppImage.** Make it runnable and start it:

```sh
chmod +x shyspace_*.AppImage
./shyspace_*.AppImage
```

If it doesn't start, install FUSE 2 (Ubuntu: `sudo apt install libfuse2t64`, Fedora: `sudo dnf install fuse-libs`).

**Fedora `.rpm` / Ubuntu `.deb`.** Install it with your package manager:

```sh
sudo dnf install ./shyspace-*.x86_64.rpm      # Fedora
sudo apt install ./shyspace_*_amd64.deb      # Ubuntu, Debian
```

## 3. Join the workspace

Ask the admin for an invite code. In shyspace choose **Join**, paste the code, and type your name. The app shows a safety code: read it to the admin (in person or on a call), and keep the window open until they let you in.

## Updates

shyspace looks for updates by itself. When one is ready it shows **Update ready**; choose **Restart** when it suits you. It never restarts during a huddle, and your messages and pages are kept.

- On Linux with the `.rpm` or `.deb`, each update asks for your password.
- On a Mac, after an update macOS may ask again to use the microphone, or for your login password so shyspace can open its key. Allow both (choose **Always Allow** for the password).

## Something wrong?

Tell the admin what you saw, and on which computer. Please don't post invite codes or screenshots of the safety code anywhere public.
