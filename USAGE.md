# Usage

```bash
rustdl [OPTIONS] [URL]
rustdl <SUBCOMMAND>
```

## Common examples

```bash
# Simple
rustdl https://example.com/file.iso

# Maximum resilience
rustdl https://example.com/file.iso --safe

# Checksum
rustdl URL --sha256 HASH

# Custom name & directory
rustdl URL -O myfile.iso -o ~/Downloads

# Rate limit
rustdl URL --limit-rate 1M

# Resume all incomplete
rustdl resume

# Resume one id
rustdl resume a84f29

# Queue
rustdl add URL1
rustdl add URL2
rustdl start

# List / info / history
rustdl list
rustdl info a84f29
rustdl history

# Verify existing file
rustdl verify ./file.iso --sha256 HASH

# JSON progress (scripts)
rustdl URL --json --silent

# Completions
rustdl completion bash > /etc/bash_completion.d/rustdl
```

## Important flags

| Flag | Meaning |
|------|---------|
| `--safe` | Resilient profile + unlimited retries |
| `--profile resilient\|stable\|fast` | Network profile |
| `--retries N\|infinite` | Retry budget |
| `--force-resume` | Resume even if ETag changed |
| `--restart` | Discard partial and start over |
| `--overwrite` | Replace existing final file |
| `-y` / `--yes` | Skip resume prompt |
| `--insecure` | Disable TLS verify (warning printed) |
| `--header "K: V"` | Custom headers (repeatable) |

## Signals

`Ctrl+C` saves checkpoint and exits cleanly. Next run can resume.
