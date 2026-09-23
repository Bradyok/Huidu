# zbin-split

Turns a Huidu `.zbin` firmware bundle into a tree of small files you can read and store in plain git
(no LFS). Rebuilds the **byte-identical** `.zbin`, or a valid **modified** `.zbin` after you edit files.

```sh
cargo build --release --manifest-path tools/zbin-split/Cargo.toml
Z=tools/zbin-split/target/release/zbin-split

$Z unpack firmware/official/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0
$Z verify firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0            # strict: must equal the original
$Z status firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0            # modified / removed / added files
$Z pack   firmware/unpacked/BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0 out.zbin   # edits allowed; --strict refuses them
```

`unpack` always ends by rebuilding from disk and comparing. It fails if the rebuild isn't exact.

## Output layout

| Path | What |
|---|---|
| `manifest.json` | Tree of nodes (`zip`, `vendor`, `gzip`, `tar`, `tar_member`, `deflate`, `token_deflate`, `file`, `raw`, `zeros`) with a size and SHA-256 at every level |
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

## Editing firmware

Edit, delete or add files under `tree/`, then `pack`. You can also edit `…/_header.xml`, the
HDPLAYER/MAGICPLAYER XML.

- **Deleting** a file removes that member from its tar or ZIP. For tar, any GNU long-name record goes
  with it. For ZIP, the central directory record goes too and the entry counts are updated.
- **Adding** a file puts it in the innermost tar or ZIP whose `*.d/` folder contains it. Its member
  name is the path relative to that folder, so a file at `…/PX30_BoxPlayerD15.tar.d/BoxPlayer/x.sh`
  lands in `PX30_BoxPlayerD15.tar.gz` as `BoxPlayer/x.sh`.
  - New tar members copy owner, group and mtime from an existing member (uid 1011 `hdplayer` here).
    Mode is 0755 for `*.sh`, ELF and `#!` files, 0644 otherwise. Names over 100 bytes get a GNU
    long-name record.
  - New ZIP entries are deflated, with the timestamp and attributes of the archive's first entry.
  - `Thumbs.db`, `desktop.ini`, `.DS_Store`, `*~`, `*.swp`, `*.orig` and `*.rej` are never added. Unchanged layers are still re-emitted exactly. Every layer that contains an edit regenerates
its derived fields:

| Layer | Regenerated |
|---|---|
| DEFLATE (ZIP entry / gzip body) | recompressed (zlib level 9) |
| tar member | size field + header checksum, 512-byte padding |
| gzip | CRC32 + ISIZE trailer |
| ZIP | local header or data descriptor CRC/sizes, central directory CRC/sizes/offsets, EOCD |
| HDPLAYER `.bin` | XML length, MD5 of (length + XML + payload) |
| MAGICPLAYER `.bin` | XML length, MD5 of payload |
| `.zbin` `fileInfo.xml` | `size="…"` of each `.bin` |

`pack` lists the edited files and says whether the output is the original or a modified image.

Verified on the real 7.11.18.0 image. I edited `upgrade.sh`, deleted `image/1.png`, and added a
script plus a file with a 100+ byte path to `PX30_BoxPlayerD15.tar.gz`. I also removed and added
entries in the MagicPlayer ZIP. Python's `zipfile`, `gzip`, `tarfile` and `hashlib`, Info-ZIP
`unzip -t` and GNU tar 1.35 all accept the result, and `hello.sh` extracts as
`rwxr-xr-x hdplayer/hdplayer`.

## Tests

`cargo test --release --manifest-path tools/zbin-split/Cargo.toml` builds a synthetic `.zbin` with
the vendor layering: ZIP of HDPLAYER (tar.gz of tar.gz) and MAGICPLAYER (ZIP), plus `fileInfo.xml`.
The tests check:
- the exact rebuild;
- edits, removals and additions (including GNU long names and junk-file filtering), read back with
  the independent `zip` and `tar` crates;
- that reverting an edit restores the exact image;
- `.partNNN` splitting;
- the `rawdeflate` round trip at zlib levels 0–9.

They don't need the 331 MiB vendor file.

Limits:
- **Only file members can be added or deleted.** You can't delete a whole nested archive's folder to
  drop that archive.
- **APKs need re-signing.** An edited APK keeps its now-stale v2 signature block; `pack` warns, and
  you must re-sign that APK before Android will install it.
- **ZIP64 archives and ZIP entries using methods other than store/deflate can't be edited.**

[preflate-rs]: https://github.com/microsoft/preflate-rs
