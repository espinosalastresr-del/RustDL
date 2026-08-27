# Troubleshooting

| Problem | What to do |
|---------|------------|
| DNS / timeout | Wait; retries with backoff are automatic. Increase `--idle-timeout`. |
| Server without Range | Resume not possible; use `--restart` or download in one go on a stable link. |
| ETag changed | File may have changed on server. `--force-resume` (risk) or `--restart`. |
| HTTP 403/404 | Non-retryable. Check URL / auth headers. |
| HTTP 429 | Honors `Retry-After`. |
| HTTP 5xx | Retried automatically. |
| Disk full | Free space; tool checks before start when size known. |
| iSH closed | Run `rustdl resume` — progress is in `.part`. |
| Checksum fail | File not renamed to final name; re-download. |
| Two instances | Lock prevents concurrent download of same path. |
| TLS errors | Fix system certs; only as last resort `--insecure`. |
| Binary won't run | Wrong arch; use musl build for iSH Alpine. |

## iSH limits

iOS may suspend or kill iSH at any time. rustdl cannot prevent that. Design assumes termination is recoverable via `.part` + state files.
