//! Build script for sola-browser-wpe.
//!
//! Two jobs:
//!
//! 1. **RUNPATH.** Same as before: bake `/run/current-system/sw/lib`
//!    into the binary so iced's dlopen-loaded wayland-sys finds
//!    libwayland-client at runtime. Now also bakes the lib paths of
//!    every package we link against (WPEWebKit + libwpe + libwpe-fdo
//!    + GLib + libsoup) so the binary works outside a `nix-shell`.
//!
//! 2. **WPE bindings.** Runs `pkg-config` against the wpe-webkit-2.0
//!    + wpe-1.0 + wpebackend-fdo-1.0 + glib-2.0 module set, emits
//!    `cargo:rustc-link-lib` for each, and invokes `bindgen` against
//!    `src/wpe_wrapper.h` to produce `$OUT_DIR/wpe_bindings.rs`.
//!
//! Requires `pkg-config` + the WPE packages on the build environment.
//! Run via `nix-shell nix/wpewebkit/shell.nix --run "cargo build ..."`.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/wpe_wrapper.h");

    println!("cargo:rustc-link-arg=-Wl,--enable-new-dtags");
    println!("cargo:rustc-link-arg=-Wl,-rpath,/run/current-system/sw/lib");
    // NVIDIA's libEGL_nvidia.so + dispatch lib live here. Without this
    // in RUNPATH, an installed binary outside the nix-shell falls back
    // to whatever Mesa libEGL it finds (or nothing) and the WPE GPU
    // setup fails with "failed to get driver name for fd -1".
    println!("cargo:rustc-link-arg=-Wl,-rpath,/run/opengl-driver/lib");

    // Resolve every WPE-side module via pkg-config. The `probe()`
    // call emits cargo:rustc-link-search / rustc-link-lib for us and
    // returns the include paths bindgen needs. Modules in
    // dependency order — wpe-webkit pulls in the others transitively
    // but we ask for them explicitly so build failures fail loudly
    // and not as a confusing missing-symbol at link time.
    let modules = [
        "glib-2.0",
        // xkbcommon is transitively included via WPEKeymapXKB.h —
        // bindgen needs its header path even though we don't
        // ourselves bind any xkbcommon functions.
        "xkbcommon",
        // libEGL — wpe_fdo_initialize_for_egl_display takes an
        // EGLDisplay. We get a *real* one via the GBM platform path
        // (see probe), not the default display (which lands on Mesa
        // and fails on NVIDIA).
        "egl",
        "gbm",
        "wpe-1.0",
        "wpebackend-fdo-1.0",
        "wpe-webkit-2.0",
    ];

    let mut include_paths = Vec::new();
    let mut lib_paths = Vec::new();
    for m in modules {
        let lib = pkg_config::Config::new()
            .cargo_metadata(true)
            .probe(m)
            .unwrap_or_else(|e| panic!("pkg-config probe of {m} failed: {e}"));
        for p in &lib.include_paths {
            include_paths.push(p.clone());
        }
        for p in &lib.link_paths {
            lib_paths.push(p.clone());
        }
    }

    // Emit the absolute path to libWPEBackend-fdo-1.0.so as a build-
    // time env var the probe binary reads via `env!()`. libwpe's
    // `wpe_loader_init(name)` does a plain `dlopen(name)` — relying
    // on the loader's search path doesn't work because libwpe's own
    // RUNPATH doesn't include our backend store path. Passing the
    // full path sidesteps it.
    //
    // Use `pkg-config --variable=libdir` directly rather than the
    // pkg_config crate's `link_paths` — the latter includes transitive
    // deps' link paths (libwpe's lib dir leaks into wpebackend-fdo's
    // result), and `first()` picks the wrong one. The `--variable`
    // call returns ONLY the queried package's own libdir.
    let wpe_fdo_libdir = std::process::Command::new("pkg-config")
        .args(["--variable=libdir", "wpebackend-fdo-1.0"])
        .output()
        .expect("running pkg-config")
        .stdout;
    let wpe_fdo_libdir = String::from_utf8(wpe_fdo_libdir)
        .expect("pkg-config libdir not utf8")
        .trim()
        .to_string();
    let backend_so = PathBuf::from(&wpe_fdo_libdir).join("libWPEBackend-fdo-1.0.so");
    if !backend_so.exists() {
        panic!(
            "expected backend SO at {} but it does not exist",
            backend_so.display()
        );
    }
    println!("cargo:rustc-env=WPE_BACKEND_FDO_SO={}", backend_so.display());

    // WEBKIT_EXEC_PATH must point at the libexec dir holding the
    // WPEWebProcess / WPENetworkProcess / WPEGPUProcess helpers. WPE
    // looks for them at runtime; in the nix-shell it comes from the
    // shell's env, but an installed binary needs to set it itself.
    // wpe-webkit-2.0.pc exposes `exec_prefix` which points at the
    // package root; the helpers live at `<exec_prefix>/libexec/wpe-webkit-2.0/`.
    let wpe_exec_prefix = std::process::Command::new("pkg-config")
        .args(["--variable=exec_prefix", "wpe-webkit-2.0"])
        .output()
        .expect("running pkg-config for exec_prefix")
        .stdout;
    let wpe_exec_prefix = String::from_utf8(wpe_exec_prefix)
        .expect("pkg-config exec_prefix not utf8")
        .trim()
        .to_string();
    let webkit_exec_path = PathBuf::from(&wpe_exec_prefix).join("libexec/wpe-webkit-2.0");
    if !webkit_exec_path.is_dir() {
        panic!(
            "expected WebKit helper dir at {} but it does not exist",
            webkit_exec_path.display()
        );
    }
    println!(
        "cargo:rustc-env=WEBKIT_EXEC_PATH={}",
        webkit_exec_path.display()
    );

    // Bake every link path into RUNPATH so the produced binary can
    // be run outside the nix-shell that built it.
    for p in &lib_paths {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", p.display());
    }

    let mut builder = bindgen::Builder::default()
        .header("src/wpe_wrapper.h")
        .derive_default(true)
        .generate_comments(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        // Block the most common GLib macros that bindgen mis-handles
        // (variadic GLib types and platform-conditional integer
        // typedefs). We don't need them for our API surface.
        .blocklist_item("FP_INT_.*")
        .blocklist_item("FP_NAN")
        .blocklist_item("FP_INFINITE")
        .blocklist_item("FP_ZERO")
        .blocklist_item("FP_SUBNORMAL")
        .blocklist_item("FP_NORMAL")
        // Restrict generated functions / types to ones whose name
        // starts with a WPE/WebKit/GMain prefix so we don't drag in
        // every glibc declaration through transitive includes.
        .allowlist_function("wpe_.*")
        .allowlist_function("webkit_.*")
        .allowlist_function("g_main_.*")
        .allowlist_function("g_timeout_.*")
        .allowlist_function("g_object_unref")
        .allowlist_function("g_signal_connect_data")
        .allowlist_function("egl.*")
        .allowlist_function("gbm_.*")
        .allowlist_function("open")
        .allowlist_var("EGL_.*")
        .allowlist_var("GBM_.*")
        .allowlist_var("O_.*")
        .allowlist_type("Wpe.*")
        .allowlist_type("WebKit.*")
        .allowlist_type("WPE.*")
        .allowlist_var("WEBKIT_.*");
    for p in &include_paths {
        builder = builder.clang_arg(format!("-I{}", p.display()));
    }

    let bindings = builder
        .generate()
        .expect("bindgen failed to generate WPE bindings");

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    bindings
        .write_to_file(out.join("wpe_bindings.rs"))
        .expect("failed to write wpe_bindings.rs");
}
