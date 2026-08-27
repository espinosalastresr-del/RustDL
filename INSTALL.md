# Installation

## 1. Prebuilt binary (recommended on iSH)

1. Open the GitHub **Releases** page for this project.
2. Download the asset matching your platform (for iSH: **linux-musl** ARM or the target documented in the release notes).
3. Verify checksum:

```bash
sha256sum -c SHA256SUMS
```

4. Install:

```bash
chmod +x rustdl
mkdir -p ~/.local/bin
mv rustdl ~/.local/bin/
# ensure PATH
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.profile
source ~/.profile
rustdl --version
rustdl --help
```

## 2. Compile on iSH

```bash
apk update
apk add git curl build-base openssl-dev
# Install Rust (rustup) if not present — may be slow on device
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

git clone <REPOSITORY_URL> rustdl
cd rustdl
cargo build --release
mkdir -p ~/.local/bin
cp target/release/rustdl ~/.local/bin/
rustdl --version
```

**Note:** Compiling on-device is heavy. Prefer Releases or GitHub Actions.

## 3. GitHub Actions (no PC)

1. Fork the repository on GitHub.
2. Enable **Actions**.
3. Run workflow **CI** or **Release**.
4. Download the artifact / release asset on the iPhone (Safari / Files).
5. Copy into iSH (e.g. via shared folder or `curl` the release URL).
6. `chmod +x` and place on `PATH` as above.

## 4. First download

```bash
rustdl https://example.com/file.bin --safe -y
ls ~/Downloads
```

## 5. After iSH is killed by iOS

```bash
rustdl list
rustdl resume
# or
rustdl <same-url>
```

Partial files live as `filename.part` next to the final name; state as `filename.part.json`.
