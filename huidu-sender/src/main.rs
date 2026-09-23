//! `huidu-sender` daemon: drive the Huidu LED sender/receiving cards over `/dev/ttyS1`.
//!
//! Usage:
//!   huidu-sender run [--tty /dev/ttyS1] [--sendcard FILE] [--recvcard FILE ...]
//!                    [--listen 127.0.0.1:7654] [--status-secs 5]
//!   huidu-sender emit <search|status|temp|hdmi|lock N|unlock N>   # print a frame (hex), no device
//!   huidu-sender oneshot <search|status|temp|hdmi>                # send one frame, print reply
//!   huidu-sender strip-fpga <in.img> <out.bin> [--bitrev]         # /boot/fpga.img -> raw PS payload (offline)
//!   huidu-sender load-fpga <in.img> --spidev DEV --nconfig-gpio N \
//!                --nstatus-gpio N --confdone-gpio N [--msb-first]  # our write_fpga (spidev+gpio fallback)
//!
//! `run` performs the power-on sequence (search cards → push param blobs → save →
//! periodic status) and serves a line protocol on --listen for the player/CMS:
//!   brightness <0-255> | status | search | lock <card> | unlock <card>
//!
//! The 512-byte param blobs (scan/gamma/send-card config) are the Huidu tables; supply
//! them as captured files (see src/blob.rs). Without them the daemon still frames and
//! sends control commands and reads status, which is enough to bring the link up and
//! validate the protocol on a bench unit.

#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::net::TcpListener;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::time::{Duration, Instant};

use huidu_sender::protocol;
#[cfg(unix)]
use huidu_sender::{blob, frame};
#[cfg(unix)]
use protocol::Target;

#[cfg_attr(not(unix), allow(dead_code))]
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join("")
}

fn build_named_frame(name: &str, arg: Option<u8>) -> Option<Vec<u8>> {
    Some(match name {
        "search" => protocol::search_recv_cards(),
        "status" => protocol::read_send_card_status(),
        "temp" => protocol::read_temp(),
        "hdmi" => protocol::check_hdmi(),
        "lock" => protocol::lock_last_frame(arg?),
        "unlock" => protocol::unlock_last_frame(arg?),
        _ => return None,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");
    match cmd {
        "emit" => {
            let name = args.get(2).map(String::as_str).unwrap_or("");
            let arg = args.get(3).and_then(|s| s.parse::<u8>().ok());
            match build_named_frame(name, arg) {
                Some(f) => println!("{}", hex(&f)),
                None => { eprintln!("unknown command: {name}"); std::process::exit(2); }
            }
        }
        "oneshot" => run_oneshot(&args),
        "run" => run_daemon(&args),
        "strip-fpga" => run_strip_fpga(&args),
        "load-fpga" => run_load_fpga(&args),
        _ => {
            eprintln!("usage: huidu-sender <run|emit|oneshot|strip-fpga|load-fpga> ...  (see source header)");
            std::process::exit(2);
        }
    }
}

/// `strip-fpga <in.img> <out.bin> [--bitrev]` — parse `/boot/fpga.img`, drop the
/// 8-byte Huidu wrapper, and write the raw passive-serial payload. Feed the
/// output to the kernel `altera-ps-spi` firmware interface (default, driver
/// bit-reverses itself) or pass `--bitrev` to emit the LSB-first wire bytes.
/// Offline — no hardware.
fn run_strip_fpga(args: &[String]) {
    use huidu_sender::fpga_load::{ps_wire_bytes, BitOrder, FpgaImage};
    let inp = match args.get(2) { Some(p) => p, None => { eprintln!("usage: strip-fpga <in.img> <out.bin> [--bitrev]"); std::process::exit(2); } };
    let outp = match args.get(3) { Some(p) => p, None => { eprintln!("usage: strip-fpga <in.img> <out.bin> [--bitrev]"); std::process::exit(2); } };
    let bitrev = args.iter().any(|a| a == "--bitrev");
    let raw = std::fs::read(inp).unwrap_or_else(|e| { eprintln!("read {inp}: {e}"); std::process::exit(1); });
    let img = FpgaImage::parse(&raw).unwrap_or_else(|e| { eprintln!("parse {inp}: {e}"); std::process::exit(1); });
    let bytes = if bitrev { ps_wire_bytes(&img.payload, BitOrder::LsbFirst) } else { img.payload.clone() };
    std::fs::write(outp, &bytes).unwrap_or_else(|e| { eprintln!("write {outp}: {e}"); std::process::exit(1); });
    eprintln!(
        "wrote {} payload bytes to {outp} ({}, ps-sync {})",
        bytes.len(),
        if bitrev { "LSB-first wire order" } else { "raw (driver bit-reverses)" },
        if img.has_ps_sync() { "found" } else { "NOT FOUND — check image" },
    );
}

#[cfg(unix)]
fn run_load_fpga(args: &[String]) {
    use huidu_sender::fpga_io::{SpiDev, SpidevPsIo, SysfsGpio};
    use huidu_sender::fpga_load::{load, BitOrder, FpgaImage};

    let inp = match args.get(2) { Some(p) => p.clone(), None => { eprintln!("usage: load-fpga <img> --spidev DEV --nconfig-gpio N --nstatus-gpio N --confdone-gpio N [--msb-first] [--speed HZ]"); std::process::exit(2); } };
    let spidev = opt(args, "--spidev").unwrap_or("/dev/spidev0.0");
    let speed: u32 = opt(args, "--speed").and_then(|s| s.parse().ok()).unwrap_or(12_000_000);
    let order = if args.iter().any(|a| a == "--msb-first") { BitOrder::MsbFirst } else { BitOrder::LsbFirst };
    let gpio = |k: &str| opt(args, k).and_then(|s| s.parse::<u32>().ok())
        .unwrap_or_else(|| { eprintln!("{k} <global-gpio-number> is required (read from the live unit)"); std::process::exit(2); });
    let nconfig_n = gpio("--nconfig-gpio");
    let nstatus_n = gpio("--nstatus-gpio");
    let confdone_n = gpio("--confdone-gpio");

    let raw = std::fs::read(&inp).unwrap_or_else(|e| { eprintln!("read {inp}: {e}"); std::process::exit(1); });
    let img = FpgaImage::parse(&raw).unwrap_or_else(|e| { eprintln!("parse {inp}: {e}"); std::process::exit(1); });

    let spi = SpiDev::open(spidev, speed).unwrap_or_else(|e| { eprintln!("open {spidev}: {e}"); std::process::exit(1); });
    let nconfig = SysfsGpio::export(nconfig_n, "out").unwrap_or_else(|e| { eprintln!("nconfig gpio{nconfig_n}: {e}"); std::process::exit(1); });
    let nstatus = SysfsGpio::export(nstatus_n, "in").unwrap_or_else(|e| { eprintln!("nstatus gpio{nstatus_n}: {e}"); std::process::exit(1); });
    let confdone = SysfsGpio::export(confdone_n, "in").unwrap_or_else(|e| { eprintln!("confdone gpio{confdone_n}: {e}"); std::process::exit(1); });
    let mut io = SpidevPsIo::new(spi, nconfig, nstatus, confdone);

    eprintln!("loading {} payload bytes via {spidev} @ {speed} Hz ({:?})...", img.payload.len(), order);
    match load(&mut io, &img, order) {
        Ok(()) => println!("FPGA configured OK (CONF_DONE asserted)"),
        Err(e) => { eprintln!("load failed: {e}"); std::process::exit(1); }
    }
}

#[cfg(not(unix))]
fn run_load_fpga(_args: &[String]) {
    eprintln!("load-fpga requires a Unix host with spidev + sysfs GPIO (use strip-fpga for the offline step)");
    std::process::exit(1);
}

#[cfg_attr(not(unix), allow(dead_code))]
fn opt<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1)).map(String::as_str)
}

#[cfg(unix)]
fn run_oneshot(args: &[String]) {
    use huidu_sender::serial::Serial;
    let tty = opt(args, "--tty").unwrap_or("/dev/ttyS1");
    let name = args.get(2).map(String::as_str).unwrap_or("status");
    let f = match build_named_frame(name, None) {
        Some(f) => f,
        None => { eprintln!("unknown command: {name}"); std::process::exit(2); }
    };
    let port = Serial::open(tty).unwrap_or_else(|e| { eprintln!("open {tty}: {e}"); std::process::exit(1); });
    port.write_all(&f).unwrap();
    println!("sent {} ({} B)", name, f.len());
    // read a reply for up to 1s
    let mut acc = Vec::new();
    let mut tmp = [0u8; 512];
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        match port.read(&mut tmp) {
            Ok(0) => {}
            Ok(n) => {
                acc.extend_from_slice(&tmp[..n]);
                if let Ok(fr) = frame::decode(&acc) {
                    if let Some((t, fc, ok, hdmi)) = protocol::parse_status(&fr.payload) {
                        println!("reply target={t} func=0x{fc:04x} ok={ok} hdmi={hdmi}");
                    } else {
                        println!("reply {} bytes: {}", fr.payload.len(), hex(&fr.payload[..fr.payload.len().min(32)]));
                    }
                    return;
                }
            }
            Err(e) => { eprintln!("read: {e}"); break; }
        }
    }
    println!("(no decodable reply within 1s)");
}

#[cfg(not(unix))]
fn run_oneshot(_args: &[String]) {
    eprintln!("oneshot requires a Unix host with /dev/ttyS1");
    std::process::exit(1);
}

#[cfg(unix)]
fn run_daemon(args: &[String]) {
    use huidu_sender::serial::Serial;

    let tty = opt(args, "--tty").unwrap_or("/dev/ttyS1").to_string();
    let listen = opt(args, "--listen").unwrap_or("127.0.0.1:7654").to_string();
    let status_secs: u64 = opt(args, "--status-secs").and_then(|s| s.parse().ok()).unwrap_or(5);
    let sendcard_path = opt(args, "--sendcard").map(PathBuf::from);
    let recvcard_paths: Vec<PathBuf> = args
        .iter().enumerate()
        .filter(|(_, a)| a.as_str() == "--recvcard")
        .filter_map(|(i, _)| args.get(i + 1)).map(PathBuf::from).collect();

    let port = Serial::open(&tty).unwrap_or_else(|e| { eprintln!("open {tty}: {e}"); std::process::exit(1); });
    eprintln!("huidu-sender: opened {tty} @115200 8N1");

    // Load the send-card blob (holds brightness/global config), if provided.
    let mut sendcard = sendcard_path.as_ref().and_then(|p| match blob::load(p) {
        Ok(b) => { eprintln!("loaded send-card blob {}", p.display()); Some(b) }
        Err(e) => { eprintln!("send-card blob {}: {e}", p.display()); None }
    });

    // ── Power-on sequence (PLAYER_PIPELINE.md §2.7) ──
    let _ = port.write_all(&protocol::search_recv_cards());
    eprintln!("sent search (0x0100)");
    for (i, p) in recvcard_paths.iter().enumerate() {
        match blob::load(p) {
            Ok(b) => {
                // basic param / scan / gamma etc. carry the recv-card blob (func varies;
                // here we push it as a recv-card param frame — refine per file type).
                let _ = port.write_all(&protocol::param_frame(Target::RecvCard, i as u8, 0, protocol::func::SEARCH_OR_BASIC, 0, &b));
                let _ = port.write_all(&protocol::save_to_recv_card(i as u8));
                eprintln!("pushed recv-card blob {} (card {i})", p.display());
            }
            Err(e) => eprintln!("recv-card blob {}: {e}", p.display()),
        }
    }
    if let Some(b) = sendcard.as_ref() {
        let _ = port.write_all(&protocol::send_card_param(b));
        eprintln!("pushed send-card blob (0x0000)");
    }

    // ── Control server (line protocol) + periodic status ──
    let listener = TcpListener::bind(&listen).unwrap_or_else(|e| { eprintln!("bind {listen}: {e}"); std::process::exit(1); });
    listener.set_nonblocking(true).ok();
    eprintln!("listening on {listen} (commands: brightness N | status | search | lock N | unlock N)");

    let mut last_status = Instant::now();
    loop {
        // accept a client (non-blocking), handle its lines
        if let Ok((stream, _)) = listener.accept() {
            stream.set_nonblocking(false).ok();
            let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
            let mut w = stream.try_clone().expect("clone");
            let reader = BufReader::new(stream);
            for line in reader.lines() {
                let line = match line { Ok(l) => l, Err(_) => break };
                let reply = handle_line(&port, &mut sendcard, line.trim());
                let _ = writeln!(w, "{reply}");
                if reply == "bye" { break; }
            }
            let _ = peer;
        }

        // periodic send-card status read
        if last_status.elapsed() >= Duration::from_secs(status_secs) {
            let _ = port.write_all(&protocol::read_send_card_status());
            last_status = Instant::now();
        }

        // drain any device bytes and log decoded status
        let mut tmp = [0u8; 512];
        if let Ok(n) = port.read(&mut tmp) {
            if n > 0 {
                if let Ok(fr) = frame::decode(&tmp[..n]) {
                    if let Some((t, fc, ok, hdmi)) = protocol::parse_status(&fr.payload) {
                        eprintln!("status: target={t} func=0x{fc:04x} ok={ok} hdmi={hdmi}");
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn handle_line(port: &huidu_sender::serial::Serial, sendcard: &mut Option<blob::Blob>, line: &str) -> String {
    let mut it = line.split_whitespace();
    match it.next() {
        Some("brightness") => {
            let level: u8 = match it.next().and_then(|s| s.parse().ok()) { Some(v) => v, None => return "err: brightness <0-255>".into() };
            match sendcard.as_mut() {
                Some(b) => {
                    if blob::patch_brightness(b, level) {
                        let _ = port.write_all(&protocol::send_card_param(b));
                        format!("ok brightness={level}")
                    } else {
                        "err: brightness offset unknown (see src/blob.rs) — needs a hardware-confirmed offset".into()
                    }
                }
                None => "err: no --sendcard blob loaded".into(),
            }
        }
        Some("status") => { let _ = port.write_all(&protocol::read_send_card_status()); "ok status-requested".into() }
        Some("search") => { let _ = port.write_all(&protocol::search_recv_cards()); "ok search-sent".into() }
        Some("lock") => match it.next().and_then(|s| s.parse::<u8>().ok()) {
            Some(c) => { let _ = port.write_all(&protocol::lock_last_frame(c)); format!("ok lock {c}") }
            None => "err: lock <card>".into(),
        },
        Some("unlock") => match it.next().and_then(|s| s.parse::<u8>().ok()) {
            Some(c) => { let _ = port.write_all(&protocol::unlock_last_frame(c)); format!("ok unlock {c}") }
            None => "err: unlock <card>".into(),
        },
        Some("quit") | Some("exit") => "bye".into(),
        Some(other) => format!("err: unknown command '{other}'"),
        None => "err: empty".into(),
    }
}

#[cfg(not(unix))]
fn run_daemon(_args: &[String]) {
    eprintln!("run requires a Unix host with /dev/ttyS1");
    std::process::exit(1);
}
