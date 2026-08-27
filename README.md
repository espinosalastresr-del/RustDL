# rustdl

**Resilient CLI download manager** written in Rust, optimized for unstable networks and **iSH on iOS**.

> A poor connection should only make a download take longer — never force you to lose progress.

## Features

- **Resume** via HTTP Range (with validation — never appends a full `200` response to a partial file)
- **Checkpoints** every N seconds / N MB (atomic state writes)
- **Unlimited retries** with exponential backoff + jitter + `Retry-After`
- **Profiles**: `resilient` (default), `stable`, `fast`
- **`--safe`** mode for extremely unstable mobile links
- Streaming downloads — **O(1) memory** vs file size
- SHA-256 / SHA-512 / SHA-1 / MD5 verification
- Queue, history, locks, JSON progress
- Designed around iOS killing iSH: `.part` + state survive process death

## Quick start

```bash
# Download
rustdl https://example.com/large.iso --safe

# Resume after interruption
rustdl resume

# With checksum
rustdl https://example.com/file.iso --sha256 abcdef...

# Limit rate
rustdl URL --limit-rate 500K
```

## Installation

See **[INSTALL.md](INSTALL.md)** for:

- Prebuilt binary from GitHub Releases
- Compile on iSH
- GitHub Actions (no PC required)

## Documentation

| Doc | Description |
|-----|-------------|
| [INSTALL.md](INSTALL.md) | Install on iSH / Linux / macOS / Windows |
| [USAGE.md](USAGE.md) | Commands and examples |
| [CONFIG.md](CONFIG.md) | Config file, env vars, profiles |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | Common failures |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Design |

## Priority order

1. File integrity  
2. Progress preservation  
3. Resume  
4. Error tolerance  
5. Efficient data use  
6. Low resource use  
7. Compatibility  
8. Speed  

## License

MIT OR Apache-2.0
