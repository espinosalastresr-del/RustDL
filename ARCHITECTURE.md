# Architecture

```
CLI → Application → Download Engine → HTTP Client
                         ↓
                    Storage / State
                         ↓
                    Verification
```

- **rustdl** binary embeds the engine (library split is straightforward later).
- Downloads stream to `*.part`; state in `*.part.json` with atomic renames.
- Resume validates `206` + `Content-Range` against requested offset.
- Single connection by default; multi-connection only in `fast` profile when Range + size known.
- Memory use stays roughly constant regardless of file size.

## Modules

- `cli` — clap interface  
- `config` — profiles, timeouts, retries  
- `downloader` — engine, retry/backoff, range, resume  
- `storage` — state, locks, atomic IO  
- `metadata` — HEAD/Range probes  
- `verification` — streaming hashes  
- `queue` — persistent queue  
- `ui` — progress  

## Library vs CLI

- `rustdl` as **library** (`src/lib.rs`) exposes `Engine`, `Config`, state types.
- Binary is a thin CLI over the library — TUI / daemon / local API can reuse the same engine.

## Multi-connection

When `connections > 1`, server advertises Range, size is known, and data-saver is off:

1. `plan_segments(total, n)` divides the file.
2. Incomplete segments only are fetched concurrently.
3. Writes use seek into a pre-sized `.part` under a mutex.
4. If segments remain incomplete, falls back to single-connection resume from contiguous prefix.

## Auth

- `--header`
- `--basic-auth user:pass`
- `--bearer TOKEN`
