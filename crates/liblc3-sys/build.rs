//! Compiles liblc3 1.1.3 (its `src/makefile.mk` list) from `.cache/liblc3/` (run `just liblc3`)
//! as the LC3 port's oracle: strict IEEE float. liblc3's own builds (Makefile and meson) add
//! `-ffast-math`, which leaves the operation order (reassociation, FMA contraction) to the
//! compiler, so its output differs between compilers and CPUs (measured: both differ from the
//! strict build on the first MDCT frame); here `-fno-fast-math -ffp-contract=off` fixes it to
//! the source order, the order the Rust port keeps. On aarch64, `+nosimd` compiles out the
//! NEON kernels (`mdct_neon.h`, `ltpf_neon.h`: another operation order, with FMA); GCC 12
//! re-defines `__ARM_NEON` after a `-U`, so the target feature goes, not the macro. The
//! oracle is the same portable C on every target.
//!
//! On x86-64 a second copy, the speed baseline as distributions build it (meson's release
//! build: `-O3 -ffast-math`, LTO, hidden visibility, baseline x86-64; liblc3's Makefile: the
//! same flags), LTO-linked into one relocatable object (`-r -flto`), its public API renamed
//! `lc3fast_*` and everything else made local (`objcopy`), so both copies link into one
//! benchmark (`fast` in `lib.rs`).
use std::path::PathBuf;
use std::process::Command;

const SRC: &[&str] = &[
    "attdet", "bits", "bwdet", "energy", "lc3", "ltpf", "mdct", "plc", "sns", "spec", "tables",
    "tns",
];

/// The gitignored `.cache/` of this checkout, or of the main checkout when building in a
/// `git worktree`.
fn cache_dir() -> PathBuf {
    let ws = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if ws.join(".cache/liblc3").exists() {
        return ws.join(".cache");
    }
    let out = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(&ws)
        .output()
        .ok()
        .filter(|o| o.status.success());
    match out {
        Some(o) => PathBuf::from(String::from_utf8_lossy(&o.stdout).trim())
            .parent()
            .map_or_else(|| ws.join(".cache"), |main| main.join(".cache")),
        None => ws.join(".cache"),
    }
}

fn main() {
    let root = cache_dir().join("liblc3/liblc3-1.1.3");
    assert!(
        root.join("src/lc3.c").exists(),
        "liblc3 not found at {}: run `just liblc3`",
        root.display()
    );
    let mut b = cc::Build::new();
    b.std("c11")
        .opt_level(2)
        .include(root.join("include"))
        .include(root.join("src"))
        .flag("-fno-fast-math")
        .flag("-ffp-contract=off")
        .warnings(false);
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") {
        b.flag("-march=armv8-a+nosimd");
    }
    for s in SRC {
        b.file(root.join(format!("src/{s}.c")));
    }
    b.compile("lc3");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") {
        fast(&root);
    }
    println!("cargo::rustc-check-cfg=cfg(liblc3_fast)");
    println!("cargo:rerun-if-env-changed=OBJCOPY");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", root.join("src").display());
}

/// The public API `lc3.h` exports (renamed `lc3fast_*` in the fast copy).
const API: &[&str] = &[
    "lc3_frame_samples",
    "lc3_frame_bytes",
    "lc3_encoder_size",
    "lc3_setup_encoder",
    "lc3_encode",
    "lc3_decoder_size",
    "lc3_setup_decoder",
    "lc3_decode",
];

/// `liblc3fast.a`: the distribution build (`-O3 -ffast-math`, LTO), API renamed, the rest
/// local. Benchmark-only: without GCC and binutils (`OBJCOPY`, the cc crate's archiver) it is
/// skipped with a warning (`fast::API` is `None`), and the oracle still builds.
fn fast(root: &std::path::Path) {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let mut b = cc::Build::new();
    b.std("c11")
        .opt_level(3)
        .include(root.join("include"))
        .include(root.join("src"))
        .flag("-ffast-math")
        .flag("-fvisibility=hidden")
        .flag("-flto")
        .warnings(false);
    let cc = b.get_compiler();
    if !cc.is_like_gnu() || cc.is_like_clang() {
        println!("cargo:warning=liblc3 fast copy skipped: its LTO link needs GCC");
        return;
    }
    for s in SRC {
        b.file(root.join(format!("src/{s}.c")));
    }
    let objs = b.compile_intermediates();
    let merged = out.join("lc3fast.o");
    let ok = |c: &mut Command| c.status().is_ok_and(|s| s.success());
    // the LTO link into one relocatable object, by the compiler that made the objects
    let mut link = Command::new(cc.path());
    link.args(["-r", "-flto", "-O3", "-ffast-math", "-fvisibility=hidden"])
        .args(["-flinker-output=nolto-rel", "-o"])
        .arg(&merged)
        .args(&objs);
    let mut oc = Command::new(std::env::var("OBJCOPY").unwrap_or_else(|_| "objcopy".into()));
    for f in API {
        oc.arg(format!("--keep-global-symbol=lc3fast_{}", &f[4..]))
            .arg(format!("--redefine-sym={f}=lc3fast_{}", &f[4..]));
    }
    oc.arg(&merged);
    let lib = out.join("liblc3fast.a");
    let _ = std::fs::remove_file(&lib);
    let mut ar = b.get_archiver();
    ar.arg("crs").arg(&lib).arg(&merged);
    for (step, c) in [
        ("LTO link", &mut link),
        ("objcopy", &mut oc),
        ("ar", &mut ar),
    ] {
        if !ok(c) {
            println!("cargo:warning=liblc3 fast copy skipped: {step} failed");
            return;
        }
    }
    println!("cargo:rustc-link-lib=static=lc3fast");
    println!("cargo:rustc-cfg=liblc3_fast");
}
