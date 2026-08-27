# Configuration

Priority: **CLI > environment > `~/.rustdl/config.toml` > defaults**

## Paths

```
~/.rustdl/
  config.toml
  history.jsonl
  queue.json
  logs/
  state/
  locks/
  cache/

~/Downloads/          # default download dir
```

## Environment

- `RUSTDL_DOWNLOAD_DIR`
- `RUSTDL_PROFILE`
- `RUSTDL_RETRIES`
- `RUSTDL_LIMIT_RATE`

## Profiles

| Profile | Connections | Retries | Idle timeout | Use case |
|---------|-------------|---------|--------------|----------|
| resilient | 1 | infinite | high | Mobile / iSH default |
| stable | 1 | high | medium | Home Wi‑Fi |
| fast | 4 | moderate | low | Fast stable link |

`--safe` forces the resilient profile.
