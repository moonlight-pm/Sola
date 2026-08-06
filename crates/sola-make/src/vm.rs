//! Build and run the Sola QEMU disk image.
//!
//! Pipeline for `cargo make vm build`:
//!   1. Stage `target/release/*` under `var/images/stage/` (no cargo build —
//!      run `cargo make build --release` / `cargo build --release` yourself).
//!   2. `nix build --impure .#sola-vm-qcow2` with `SOLA_VM_STAGE` set.
//!   3. Copy the qcow2 out of the store into `var/images/sola-vm.qcow2`
//!      (writable working copy; store path is read-only).
//!
//! `cargo make vm run` checks nix/QEMU/OVMF and (re)builds the *image* when
//! missing/stale — it does **not** invoke cargo.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio, exit};
use std::time::SystemTime;

const STAGE_DIR: &str = "var/images/stage";
const IMAGE_PATH: &str = "var/images/sola-vm.qcow2";
const OVERLAY_PATH: &str = "var/images/sola-vm-overlay.qcow2";
const FLAKE_ATTR: &str = ".#sola-vm-qcow2";

/// Nix / installer sources that should trigger an image refresh when newer
/// than the local qcow2.
const IMAGE_INPUT_PATHS: &[&str] = &[
    "flake.nix",
    "nix/module.nix",
    "nix/sola.nix",
    "nix/image/configuration.nix",
    "nix/image/quiet-boot.nix",
    "nix/image/installer-session.nix",
    "nix/image/sola-from-stage.nix",
    "nix/image/plymouth/default.nix",
    "crates/sola-assets/icons/sola/flower.svg",
    "crates/sola-install/src/main.rs",
    "crates/sola-install/Cargo.toml",
];

pub struct BuildOpts {
    /// Include the CEF Release tree from `~/.cache/sola/cef-*` (large).
    pub with_cef: bool,
    /// Skip the nix image build (stage only — debugging).
    pub stage_only: bool,
}

pub struct RunOpts {
    /// When true, build/refresh the image if missing or stale.
    pub auto_build: bool,
    /// Force a full rebuild even if an image exists.
    pub force_rebuild: bool,
}

pub fn build(opts: BuildOpts) {
    if let Err(e) = run_build(opts) {
        eprintln!("vm build failed: {e}");
        exit(1);
    }
}

pub fn run(opts: RunOpts) {
    if let Err(e) = run_vm(opts) {
        eprintln!("vm run failed: {e}");
        exit(1);
    }
}

fn run_build(opts: BuildOpts) -> Result<(), String> {
    let root = workspace_root()?;
    env::set_current_dir(&root).map_err(|e| format!("chdir workspace: {e}"))?;

    require_nix()?;

    let stage = root.join(STAGE_DIR);
    println!(">>> staging release tree at {}", stage.display());
    stage_tree(&root, &stage, &opts)?;

    if opts.stage_only {
        println!(">>> --stage-only: skipping nix image build");
        println!("    stage ready at {}", stage.display());
        return Ok(());
    }

    println!(">>> nix build {FLAKE_ATTR} (impure, SOLA_VM_STAGE set)");
    let out_link = root.join("var/images/result-qcow2");
    let status = Command::new("nix")
        .args([
            "build",
            FLAKE_ATTR,
            "--impure",
            "--out-link",
            out_link.to_str().unwrap(),
        ])
        .env("SOLA_VM_STAGE", stage.to_str().unwrap())
        .status()
        .map_err(|e| format!("nix build: {e}"))?;
    if !status.success() {
        return Err(format!("nix build exited {}", status.code().unwrap_or(1)));
    }

    let qcow = find_qcow2(&out_link)?;
    let dest = root.join(IMAGE_PATH);
    println!(">>> copying {} -> {}", qcow.display(), dest.display());
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir images: {e}"))?;
    }
    // Fresh base image; drop any prior overlay so run uses a clean delta.
    let _ = fs::remove_file(root.join(OVERLAY_PATH));
    let _ = fs::remove_file(root.join("var/images/OVMF_VARS.fd"));
    fs::copy(&qcow, &dest).map_err(|e| format!("copy qcow2: {e}"))?;
    // Nix store files are mode 0444; qemu-img/qemu need a writable base or overlay.
    make_owner_writable(&dest)?;

    let size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    println!(
        "✓ image ready: {} ({:.1} GiB)",
        dest.display(),
        size as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!("  run with: cargo make vm run");
    Ok(())
}

fn run_vm(opts: RunOpts) -> Result<(), String> {
    let root = workspace_root()?;
    env::set_current_dir(&root).map_err(|e| format!("chdir workspace: {e}"))?;

    println!(">>> checking VM prerequisites");
    require_nix()?;
    // Resolve firmware + emulator early so first-run fetches happen before
    // a multi-minute image build when possible… but image build may still
    // pull nixpkgs. At least fail fast if nix is broken.
    let qemu = resolve_qemu()?;
    let ovmf = resolve_ovmf()?;
    println!("    nix: ok");
    println!("    qemu: {}", qemu.display());
    println!("    ovmf: {}", ovmf.code.display());
    if Path::new("/dev/kvm").exists() {
        println!("    kvm: /dev/kvm");
    } else {
        println!("    kvm: not available (TCG — slower)");
    }
    let preview_w = env::var("SOLA_VM_WIDTH").unwrap_or_else(|_| "1920".into());
    let preview_h = env::var("SOLA_VM_HEIGHT").unwrap_or_else(|_| "1080".into());
    let preview_display = display_backend(&preview_w, &preview_h);
    if preview_display.starts_with("gtk") {
        println!("    display: {preview_display}");
    } else {
        println!(
            "    display: {preview_display} (no DISPLAY/WAYLAND — boot splash needs a graphical session)"
        );
    }

    let image = root.join(IMAGE_PATH);
    let need_build = opts.force_rebuild
        || !image.exists()
        || image_is_stale(&root, &image);

    if need_build {
        if !opts.auto_build {
            if !image.exists() {
                return Err(format!(
                    "missing {} — run `cargo make vm build` or `cargo make vm run` without --no-build",
                    image.display()
                ));
            }
            println!(">>> image present but may be stale; --no-build keeps it");
        } else {
            let reason = if opts.force_rebuild {
                "forced (--rebuild)"
            } else if !image.exists() {
                "image missing"
            } else {
                "image stale vs nix/image or sola-install"
            };
            println!(">>> building image ({reason})");
            println!("    staging from target/release (no cargo — build yourself first)");
            run_build(BuildOpts {
                with_cef: false,
                stage_only: false,
            })?;
        }
    } else {
        println!("    image: {} (up to date)", image.display());
    }

    let image = root.join(IMAGE_PATH);
    if !image.exists() {
        return Err(format!("image still missing at {}", image.display()));
    }

    let overlay = root.join(OVERLAY_PATH);
    ensure_overlay(&image, &overlay)?;

    let mem = env::var("SOLA_VM_MEMORY").unwrap_or_else(|_| "4096".into());
    let smp = env::var("SOLA_VM_SMP").unwrap_or_else(|_| "4".into());
    // Guest framebuffer — default 1920×1080 so installer UI is readable.
    // Override with SOLA_VM_WIDTH / SOLA_VM_HEIGHT.
    let width = env::var("SOLA_VM_WIDTH").unwrap_or_else(|_| "1920".into());
    let height = env::var("SOLA_VM_HEIGHT").unwrap_or_else(|_| "1080".into());

    let kvm = Path::new("/dev/kvm").exists();
    let accel = if kvm { "kvm" } else { "tcg" };
    let cpu = if kvm { "host" } else { "max" };
    let display = display_backend(&width, &height);

    println!(
        ">>> qemu {} accel={accel} {}x{} (overlay {})",
        qemu.display(),
        width,
        height,
        overlay.display()
    );
    let mut cmd = Command::new(&qemu);
    cmd.args([
        "-machine",
        &format!("q35,accel={accel}"),
        "-cpu",
        cpu,
        "-m",
        &mem,
        "-smp",
        &smp,
        "-drive",
        &format!(
            "if=pflash,format=raw,readonly=on,file={}",
            ovmf.code.display()
        ),
    ]);
    if let Some(vars) = &ovmf.vars_template {
        let vars_rw = root.join("var/images/OVMF_VARS.fd");
        if !vars_rw.exists() {
            fs::copy(vars, &vars_rw).map_err(|e| format!("copy OVMF_VARS: {e}"))?;
            make_owner_writable(&vars_rw)?;
        }
        cmd.args([
            "-drive",
            &format!("if=pflash,format=raw,file={}", vars_rw.display()),
        ]);
    }
    cmd.args([
        "-drive",
        &format!("file={},if=virtio,format=qcow2", overlay.display()),
        "-device",
        "virtio-net-pci,netdev=net0",
        "-netdev",
        "user,id=net0,hostfwd=tcp::2222-:22",
        // Prefer virtio-gpu with an explicit mode so the guest is not stuck
        // on a tiny 800×600 default.
        "-device",
        &format!("virtio-vga,xres={width},yres={height}"),
        "-display",
        &display,
        // Live guest serial + QEMU monitor on the host terminal you launched
        // from (engineering visibility). The *product* path is the graphical
        // window (Plymouth / installer) — console=ttyS0 keeps spam off that.
        "-serial",
        "mon:stdio",
    ]);

    let err = {
        use std::os::unix::process::CommandExt;
        cmd.exec()
    };
    Err(format!("failed to exec qemu: {err}"))
}

fn require_nix() -> Result<(), String> {
    which("nix").map(|_| ()).map_err(|_| {
        "nix not found on PATH — install Nix and enable flakes (`nix build` must work)".into()
    })
}

/// Require pre-built release binaries (no cargo). Caller builds manually.
fn require_release_bins(root: &Path) -> Result<(), String> {
    let release = root.join("target/release");
    let required = ["sola-install", "sola"];
    let mut missing = Vec::new();
    for name in required {
        let p = release.join(name);
        if p.is_file() {
            println!("    release: {}", p.display());
        } else {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "missing target/release/{{{}}} — run `cargo build --release` (or `cargo make build --release`) first",
            missing.join(", ")
        ));
    }
    Ok(())
}

fn image_is_stale(root: &Path, image: &Path) -> bool {
    let Ok(img_mtime) = mtime(image) else {
        return true;
    };
    for rel in IMAGE_INPUT_PATHS {
        let p = root.join(rel);
        if let Ok(t) = mtime(&p) {
            if t > img_mtime {
                println!("    stale: {} newer than image", rel);
                return true;
            }
        }
    }
    let install_bin = root.join("target/release/sola-install");
    if let Ok(t) = mtime(&install_bin) {
        if t > img_mtime {
            println!("    stale: target/release/sola-install newer than image");
            return true;
        }
    }
    false
}

fn mtime(path: &Path) -> Result<SystemTime, String> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(|e| format!("mtime {}: {e}", path.display()))
}

fn stage_tree(root: &Path, stage: &Path, opts: &BuildOpts) -> Result<(), String> {
    if stage.exists() {
        fs::remove_dir_all(stage).map_err(|e| format!("rm stage: {e}"))?;
    }
    let bin_dir = stage.join("bin");
    let share_dir = stage.join("share");
    let cef_dir = stage.join("cef");
    fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir bin: {e}"))?;
    fs::create_dir_all(&share_dir).map_err(|e| format!("mkdir share: {e}"))?;
    fs::create_dir_all(&cef_dir).map_err(|e| format!("mkdir cef: {e}"))?;

    // Stage pre-built release artifacts only — never cargo, never /opt/sola/bin.
    require_release_bins(root)?;
    println!(">>> staging binaries from target/release");
    let mut staged = 0usize;
    for name in crate::discover_binaries() {
        let src = root.join("target/release").join(&name);
        if !src.exists() {
            eprintln!("    skip missing {name}");
            continue;
        }
        let dst = bin_dir.join(&name);
        fs::copy(&src, &dst).map_err(|e| format!("copy {}: {e}", src.display()))?;
        staged += 1;
    }
    // Installer is required for the kiosk path.
    let install_src = root.join("target/release/sola-install");
    let install_dst = bin_dir.join("sola-install");
    if !install_dst.is_file() {
        fs::copy(&install_src, &install_dst)
            .map_err(|e| format!("copy sola-install into stage: {e}"))?;
        staged += 1;
        println!(">>> staged sola-install from target/release");
    }
    if staged == 0 {
        return Err(
            "no binaries staged from target/release — run `cargo build --release` first".into(),
        );
    }

    // Runtime assets: prefer repo first-party icons; optional host share for
    // cursors/packs if already installed (not required).
    let icons = root.join("crates/sola-assets/icons");
    if icons.is_dir() {
        println!(">>> staging crates/sola-assets/icons → share/icons");
        let dest = share_dir.join("icons");
        fs::create_dir_all(&dest).map_err(|e| format!("mkdir icons: {e}"))?;
        copy_dir_contents(&icons, &dest)?;
    }
    let opt_share = Path::new("/opt/sola/share");
    if opt_share.is_dir() {
        println!(">>> merging /opt/sola/share extras (cursors/apps if present)");
        copy_dir_contents(opt_share, &share_dir)?;
    }
    // Flower must exist for installer UI + Plymouth source tracking.
    let flower = share_dir.join("icons/sola/flower.svg");
    if !flower.is_file() {
        let src = root.join("crates/sola-assets/icons/sola/flower.svg");
        if src.is_file() {
            if let Some(parent) = flower.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("mkdir flower parent: {e}"))?;
            }
            fs::copy(&src, &flower).map_err(|e| format!("copy flower.svg: {e}"))?;
            println!(">>> staged flower.svg into share/icons/sola/");
        }
    }

    if opts.with_cef {
        stage_cef(&cef_dir)?;
    } else {
        println!(">>> skipping CEF (pass --with-cef to include ~4G runtime)");
    }

    let n_bins = fs::read_dir(&bin_dir)
        .map(|rd| rd.count())
        .unwrap_or(0);
    if n_bins == 0 {
        return Err("stage has zero binaries".into());
    }
    if !install_dst.is_file() {
        return Err("stage missing sola-install after inject".into());
    }
    println!("    staged {n_bins} binaries (incl. sola-install)");
    Ok(())
}

fn stage_cef(cef_dir: &Path) -> Result<(), String> {
    let version = fs::read_to_string("cef-version")
        .map_err(|e| format!("read cef-version: {e}"))?
        .trim()
        .to_string();
    let home = env::var("HOME").map_err(|e| format!("HOME unset: {e}"))?;
    let cef_release = PathBuf::from(format!("{home}/.cache/sola/cef-{version}/Release"));
    if !cef_release.is_dir() {
        return Err(format!(
            "CEF cache not found at {} — run `cargo make install-cef` or omit --with-cef",
            cef_release.display()
        ));
    }
    println!(">>> staging CEF from {}", cef_release.display());
    copy_dir_contents(&cef_release, cef_dir)?;
    Ok(())
}

fn ensure_overlay(base: &Path, overlay: &Path) -> Result<(), String> {
    if overlay.exists() {
        return Ok(());
    }
    let qemu_img = resolve_qemu_img()?;
    println!(
        ">>> creating overlay {} -> {}",
        overlay.display(),
        base.display()
    );
    let status = Command::new(&qemu_img)
        .args([
            "create",
            "-f",
            "qcow2",
            "-b",
            base.to_str().unwrap(),
            "-F",
            "qcow2",
            overlay.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("qemu-img: {e}"))?;
    if !status.success() {
        return Err("qemu-img create overlay failed".into());
    }
    Ok(())
}

fn find_qcow2(out_link: &Path) -> Result<PathBuf, String> {
    if out_link.is_file() && out_link.extension().is_some_and(|e| e == "qcow2") {
        return Ok(out_link.to_path_buf());
    }
    if out_link.is_dir() {
        let mut found = Vec::new();
        for entry in fs::read_dir(out_link).map_err(|e| format!("read result: {e}"))? {
            let entry = entry.map_err(|e| format!("read result entry: {e}"))?;
            let p = entry.path();
            if p.extension().is_some_and(|e| e == "qcow2") {
                found.push(p);
            }
        }
        if found.len() == 1 {
            return Ok(found.remove(0));
        }
        if found.is_empty() {
            return walk_find_qcow2(out_link);
        }
        return Err(format!(
            "multiple qcow2 files in {}: {:?}",
            out_link.display(),
            found
        ));
    }
    Err(format!(
        "could not locate qcow2 under {}",
        out_link.display()
    ))
}

fn walk_find_qcow2(dir: &Path) -> Result<PathBuf, String> {
    fn walk(dir: &Path, acc: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let p = entry.path();
            if p.is_dir() {
                walk(&p, acc)?;
            } else if p.extension().is_some_and(|e| e == "qcow2") {
                acc.push(p);
            }
        }
        Ok(())
    }
    let mut acc = Vec::new();
    walk(dir, &mut acc)?;
    match acc.len() {
        1 => Ok(acc.remove(0)),
        0 => Err(format!("no qcow2 under {}", dir.display())),
        _ => Err(format!("multiple qcow2 under {}: {:?}", dir.display(), acc)),
    }
}

struct Ovmf {
    code: PathBuf,
    vars_template: Option<PathBuf>,
}

fn resolve_qemu() -> Result<PathBuf, String> {
    if let Ok(p) = which("qemu-system-x86_64") {
        return Ok(p);
    }
    println!(">>> qemu-system-x86_64 not on PATH; resolving via nix");
    let out = capture(
        "nix",
        &[
            "build",
            "--no-link",
            "--print-out-paths",
            "nixpkgs#qemu_kvm",
        ],
    )?;
    let store = out.lines().next().unwrap_or("").trim();
    if store.is_empty() {
        return Err("nix build nixpkgs#qemu_kvm produced no path".into());
    }
    let bin = PathBuf::from(store).join("bin/qemu-system-x86_64");
    if !bin.exists() {
        return Err(format!("qemu binary missing at {}", bin.display()));
    }
    Ok(bin)
}

fn resolve_qemu_img() -> Result<PathBuf, String> {
    if let Ok(p) = which("qemu-img") {
        return Ok(p);
    }
    let qemu = resolve_qemu()?;
    let img = qemu
        .parent()
        .map(|d| d.join("qemu-img"))
        .ok_or_else(|| "qemu parent missing".to_string())?;
    if img.exists() {
        return Ok(img);
    }
    Err("qemu-img not found (install qemu or ensure nixpkgs#qemu_kvm)".into())
}

fn resolve_ovmf() -> Result<Ovmf, String> {
    let candidates = [
        "/run/libvirt/nix-ovmf/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_CODE.fd",
        "/usr/share/edk2/ovmf/OVMF_CODE.fd",
        "/run/current-system/sw/share/OVMF/OVMF_CODE.fd",
    ];
    for code in candidates {
        let code = PathBuf::from(code);
        if code.exists() {
            let vars = code.with_file_name("OVMF_VARS.fd");
            return Ok(Ovmf {
                code,
                vars_template: vars.exists().then_some(vars),
            });
        }
    }

    println!(">>> OVMF not on host; resolving via nixpkgs#OVMF.fd");
    let out = capture(
        "nix",
        &[
            "build",
            "--no-link",
            "--print-out-paths",
            "nixpkgs#OVMF.fd",
        ],
    )?;
    let store = out.lines().next().unwrap_or("").trim();
    if store.is_empty() {
        return Err("nix build nixpkgs#OVMF.fd produced no path".into());
    }
    let base = PathBuf::from(store);
    let code = [
        base.join("FV/OVMF_CODE.fd"),
        base.join("OVMF_CODE.fd"),
        base.join("FV/OVMF_CODE.fd.fd"),
    ]
    .into_iter()
    .find(|p| p.exists())
    .ok_or_else(|| format!("OVMF_CODE.fd not found under {store}"))?;
    let vars = [base.join("FV/OVMF_VARS.fd"), base.join("OVMF_VARS.fd")]
        .into_iter()
        .find(|p| p.exists());
    Ok(Ovmf {
        code,
        vars_template: vars,
    })
}

/// QEMU `-display` value. Guest resolution is set on `virtio-vga`
/// (`xres`/`yres`); GTK is left at 1:1 so text stays sharp.
fn display_backend(_width: &str, _height: &str) -> String {
    if env::var_os("DISPLAY").is_some() || env::var_os("WAYLAND_DISPLAY").is_some() {
        // zoom-to-fit=off: do not shrink a 1080p guest into a tiny window.
        "gtk,zoom-to-fit=off,gl=off".into()
    } else {
        "none".into()
    }
}

fn workspace_root() -> Result<PathBuf, String> {
    let mut dir = env::current_dir().map_err(|e| format!("cwd: {e}"))?;
    loop {
        if dir.join("flake.nix").exists() && dir.join("Cargo.toml").exists() {
            return Ok(dir);
        }
        if !dir.pop() {
            return Err(
                "could not find workspace root (flake.nix + Cargo.toml)".into(),
            );
        }
    }
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("mkdir {}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("read_dir {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let ft = entry
            .file_type()
            .map_err(|e| format!("file_type: {e}"))?;
        if ft.is_dir() {
            copy_dir_contents(&from, &to)?;
        } else if ft.is_symlink() {
            if from.is_dir() {
                copy_dir_contents(&from, &to)?;
            } else {
                fs::copy(&from, &to)
                    .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))?;
            }
        } else {
            fs::copy(&from, &to)
                .map_err(|e| format!("copy {} -> {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

fn make_owner_writable(path: &Path) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    let mut perms = meta.permissions();
    let mode = perms.mode() | 0o200;
    perms.set_mode(mode);
    fs::set_permissions(path, perms)
        .map_err(|e| format!("chmod u+w {}: {e}", path.display()))?;
    Ok(())
}

fn which(name: &str) -> Result<PathBuf, String> {
    let out = Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("command -v: {e}"))?;
    if !out.status.success() {
        return Err(format!("{name} not found"));
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if p.is_empty() {
        return Err(format!("{name} not found"));
    }
    Ok(PathBuf::from(p))
}

fn capture(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| format!("{cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd} {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_backend_is_stable_string() {
        let d = display_backend("1920", "1080");
        assert!(d.starts_with("gtk") || d == "none");
    }
}
