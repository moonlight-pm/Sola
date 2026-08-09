//! Build and run the Sola installer ISO.
//!
//! Pipeline for `cargo make iso build`:
//!   1. Stage `target/release` (same tree as `vm build`, via `SOLA_VM_STAGE`).
//!   2. `nix build --impure .#sola-iso`.
//!   3. Copy ISO into `var/images/sola.iso`.
//!
//! `cargo make iso run` boots the ISO in QEMU with a blank install target disk.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::time::SystemTime;

use crate::vm::{
    TARGET_DISK_PATH, display_backend, ensure_target_disk, has_installed_image,
    make_owner_writable, prepare_stage, resolve_ovmf, resolve_qemu, workspace_root,
};

const ISO_PATH: &str = "var/images/sola.iso";
const FLAKE_ATTR: &str = ".#sola-iso";
const ISO_RESULT_LINK: &str = "var/images/result-iso";

/// Inputs that should force an ISO rebuild when newer than the local ISO.
const ISO_INPUT_PATHS: &[&str] = &[
    "flake.nix",
    "nix/module.nix",
    "nix/sola.nix",
    "nix/image/iso.nix",
    "nix/image/live-common.nix",
    "nix/image/quiet-boot.nix",
    "nix/image/installer-session.nix",
    "nix/image/installed-system.nix",
    "nix/image/installed-session.nix",
    "nix/image/install-tools.nix",
    "nix/image/sola-install-apply.sh",
    "nix/image/sola-from-stage.nix",
    "nix/image/plymouth/default.nix",
    "nix/image/plymouth/gen-frames.py",
    "crates/sola-install/src/main.rs",
    "crates/sola-install/src/apply.rs",
    "crates/sola-install/Cargo.toml",
];

pub struct IsoBuildOpts {
    pub stage_only: bool,
}

pub struct IsoRunOpts {
    pub auto_build: bool,
    pub force_rebuild: bool,
}

pub fn build(opts: IsoBuildOpts) {
    if let Err(e) = run_build(opts) {
        eprintln!("iso build failed: {e}");
        exit(1);
    }
}

pub fn run(opts: IsoRunOpts) {
    if let Err(e) = run_iso(opts) {
        eprintln!("iso run failed: {e}");
        exit(1);
    }
}

fn run_build(opts: IsoBuildOpts) -> Result<(), String> {
    let (root, stage) = prepare_stage()?;

    if opts.stage_only {
        println!(">>> --stage-only: skipping nix ISO build");
        println!("    stage ready at {}", stage.display());
        return Ok(());
    }

    println!(">>> nix build {FLAKE_ATTR} (impure, SOLA_VM_STAGE set)");
    let out_link = root.join(ISO_RESULT_LINK);
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

    let iso_src = find_iso(&out_link)?;
    let dest = root.join(ISO_PATH);
    println!(">>> copying {} -> {}", iso_src.display(), dest.display());
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir images: {e}"))?;
    }
    fs::copy(&iso_src, &dest).map_err(|e| format!("copy iso: {e}"))?;
    make_owner_writable(&dest)?;

    let size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    println!(
        "✓ ISO ready: {} ({:.1} GiB)",
        dest.display(),
        size as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!("  run with: cargo make iso run");
    Ok(())
}

fn run_iso(opts: IsoRunOpts) -> Result<(), String> {
    let root = workspace_root()?;
    env::set_current_dir(&root).map_err(|e| format!("chdir workspace: {e}"))?;

    println!(">>> checking ISO run prerequisites");
    let qemu = resolve_qemu()?;
    let ovmf = resolve_ovmf()?;
    println!("    qemu: {}", qemu.display());
    println!("    ovmf: {}", ovmf.code.display());

    let iso = root.join(ISO_PATH);
    let need_build = opts.force_rebuild || !iso.exists() || iso_is_stale(&root, &iso);

    if need_build {
        if !opts.auto_build {
            if !iso.exists() {
                return Err(format!(
                    "missing {} — run `cargo make iso build` first",
                    iso.display()
                ));
            }
            println!(">>> ISO present but may be stale; --no-build keeps it");
        } else {
            let reason = if opts.force_rebuild {
                "forced (--rebuild)"
            } else if !iso.exists() {
                "ISO missing"
            } else {
                "ISO stale"
            };
            println!(">>> building ISO ({reason})");
            run_build(IsoBuildOpts { stage_only: false })?;
        }
    } else {
        println!("    ISO: {} (up to date)", iso.display());
    }

    let iso = root.join(ISO_PATH);
    if !iso.exists() {
        return Err(format!("ISO still missing at {}", iso.display()));
    }

    // Keep an installed target so guest "Reboot" can land on the new system.
    // Wipe only when nothing is installed yet (fresh dogfood). Force wipe:
    //   rm var/images/sola-install-target.qcow2 && cargo make iso run
    let target = root.join(TARGET_DISK_PATH);
    let already_installed = has_installed_image(&root);
    if already_installed {
        let sz = fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        println!(
            ">>> keeping installed target ({:.1} GiB) — HD preferred on boot",
            sz as f64 / (1024.0 * 1024.0 * 1024.0)
        );
    } else {
        if target.exists() {
            // Tiny leftover / failed install — start clean.
            println!(">>> replacing non-installed target {}", target.display());
            fs::remove_file(&target).map_err(|e| format!("remove target: {e}"))?;
        }
        ensure_target_disk(&target)?;
    }

    let mem = env::var("SOLA_VM_MEMORY").unwrap_or_else(|_| "4096".into());
    let smp = env::var("SOLA_VM_SMP").unwrap_or_else(|_| "4".into());
    let width = env::var("SOLA_VM_WIDTH").unwrap_or_else(|_| "1920".into());
    let height = env::var("SOLA_VM_HEIGHT").unwrap_or_else(|_| "1080".into());
    let kvm = Path::new("/dev/kvm").exists();
    let accel = if kvm { "kvm" } else { "tcg" };
    let cpu = if kvm { "host" } else { "max" };
    let display = display_backend(&width, &height);

    println!(
        ">>> qemu {} accel={accel} {}x{} (ISO + target; HD boot first)",
        qemu.display(),
        width,
        height
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
    // Fresh NVRAM so we don't inherit stale Boot#### from other dogfood runs.
    // Prefer *disk* (bootindex=0) over ISO (bootindex=1): empty disk fails
    // through to the ISO; after apply, reboot lands on the installed system
    // without removing the CD (same as a real machine with install media still
    // inserted when the new ESP is preferred).
    if let Some(vars) = &ovmf.vars_template {
        let vars_rw = root.join("var/images/OVMF_VARS-iso.fd");
        println!(">>> fresh OVMF NVRAM for ISO session ({})", vars_rw.display());
        fs::copy(vars, &vars_rw).map_err(|e| format!("copy OVMF_VARS-iso: {e}"))?;
        make_owner_writable(&vars_rw)?;
        cmd.args([
            "-drive",
            &format!("if=pflash,format=raw,file={}", vars_rw.display()),
        ]);
    }
    cmd.args([
        "-device",
        "virtio-scsi-pci,id=scsi0",
        "-drive",
        &format!(
            "if=none,id=hd0,format=qcow2,file={}",
            target.display()
        ),
        "-device",
        "virtio-blk-pci,drive=hd0,bootindex=0",
        "-drive",
        &format!(
            "if=none,id=cd0,media=cdrom,readonly=on,format=raw,file={}",
            iso.display()
        ),
        "-device",
        "scsi-cd,bus=scsi0.0,drive=cd0,bootindex=1",
        "-device",
        "virtio-net-pci,netdev=net0",
        "-netdev",
        "user,id=net0,hostfwd=tcp::2222-:22",
        "-device",
        &format!("virtio-vga,xres={width},yres={height}"),
        "-display",
        &display,
        "-serial",
        "mon:stdio",
    ]);

    let err = {
        use std::os::unix::process::CommandExt;
        cmd.exec()
    };
    Err(format!("failed to exec qemu: {err}"))
}

fn find_iso(out_link: &Path) -> Result<PathBuf, String> {
    if out_link.is_file()
        && out_link
            .extension()
            .is_some_and(|e| e == "iso")
    {
        return Ok(out_link.to_path_buf());
    }
    if out_link.is_dir() {
        let mut found = Vec::new();
        walk_iso(out_link, &mut found)?;
        return match found.len() {
            1 => Ok(found.remove(0)),
            0 => Err(format!("no .iso under {}", out_link.display())),
            _ => Err(format!(
                "multiple .iso under {}: {:?}",
                out_link.display(),
                found
            )),
        };
    }
    Err(format!("could not locate ISO under {}", out_link.display()))
}

fn walk_iso(dir: &Path, acc: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            walk_iso(&p, acc)?;
        } else if p.extension().is_some_and(|e| e == "iso") {
            acc.push(p);
        }
    }
    Ok(())
}

fn iso_is_stale(root: &Path, iso: &Path) -> bool {
    let Ok(iso_mtime) = mtime(iso) else {
        return true;
    };
    for rel in ISO_INPUT_PATHS {
        let p = root.join(rel);
        if let Ok(t) = mtime(&p) {
            if t > iso_mtime {
                println!("    stale: {} newer than ISO", rel);
                return true;
            }
        }
    }
    let install_bin = root.join("target/release/sola-install");
    if let Ok(t) = mtime(&install_bin) {
        if t > iso_mtime {
            println!("    stale: target/release/sola-install newer than ISO");
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
