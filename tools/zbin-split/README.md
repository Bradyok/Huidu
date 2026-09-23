# zbin-split

Turns a Huidu `.zbin` firmware bundle into a tree of small files you can read and store in plain git
(no LFS), then rebuilds the **byte-identical** `.zbin` so it can be flashed.

```sh
cargo build --release --manifest-path tools/zbin-split/Cargo.toml
Z=tools/zbin-split/target/release/zbin-split

$Z unpack firmware/official/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0
$Z verify firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0
$Z pack   firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0 out.zbin   # sha256 == original
```

`unpack` always ends by rebuilding from disk and comparing. It fails if the rebuild isn't exact.

## Output layout

| Path | What |
|---|---|
| `manifest.json` | Tree of nodes (`seq`, `raw`, `zeros`, `file`, `deflate`, `token_deflate`) with a size and SHA-256 at every level |
| `tree/` | The exploded content, at readable paths. A container `X` becomes `X.d/`, and a gzip `X.tar.gz` becomes `X.tar.d/` |
| `meta/` | Content-addressed blobs: preflate corrections, token scripts, glue larger than 1 KiB |

Files larger than 45 MiB are stored as `name.part000`, `name.part001`, … so every file stays under
GitHub's 50 MiB warning and 100 MiB limit.

## How exact rebuild works

- **Containers** (ZIP, tar, gzip, HDPLAYER/MAGICPLAYER headers). Every byte that isn't file content,
  such as local headers, tar headers, padding, data descriptors, the central directory and gzip
  trailers, is stored verbatim. Rebuilding is plain concatenation.
- **DEFLATE made by zlib or gzip** (the inner `*.tar.gz`, APKs). [preflate-rs] stores the plaintext
  plus a small correction stream that re-derives the original encoder's exact choices.
- **DEFLATE made by Go's `compress/flate`** (the outer `.zbin`, written by Go's `archive/zip`). Go
  emits incomplete Huffman trees that preflate rejects. `rawdeflate.rs` records a token script
  instead (block types, raw dynamic-header bits, pad bits, matches) and replays it bit for bit.
- If nothing reproduces a stream, it is kept as an opaque leaf (`*.deflate`). The output is still
  exact, just not exploded.

`pack` checks the SHA-256 of every file it reads. If you edit something under `tree/`, it stops and
says which file changed. Headers and CRCs are raw bytes, so edited firmware needs its CRC32, MD5
and sizes regenerated. That is a separate step and not part of this tool yet.

[preflate-rs]: https://github.com/microsoft/preflate-rs
