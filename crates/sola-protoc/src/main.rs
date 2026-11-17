//! The part of `protoc`'s command line that prost-build, tonic-prost-build
//! and rust-protobuf use: `.proto` files in, one descriptor set out.

use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Default)]
struct Args {
    includes: Vec<PathBuf>,
    out: Option<PathBuf>,
    include_imports: bool,
    include_source_info: bool,
    files: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--version") {
        // rust-protobuf reads the version to choose its flags.
        println!("libprotoc 33.0");
        return ExitCode::SUCCESS;
    }
    match parse(argv).and_then(compile) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("protoc (sola-protoc): {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse(argv: Vec<String>) -> Result<Args, String> {
    let mut args = Args::default();
    let mut argv = argv.into_iter();
    while let Some(arg) = argv.next() {
        let mut value = |rest: &str| match rest {
            "" => argv.next().ok_or(format!("{arg} needs a value")),
            rest => Ok(rest.trim_start_matches('=').to_string()),
        };
        if arg == "--include_imports" {
            args.include_imports = true;
        } else if arg == "--include_source_info" {
            args.include_source_info = true;
        } else if arg == "--experimental_allow_proto3_optional" {
            // Always on.
        } else if let Some(rest) = arg.strip_prefix("--proto_path") {
            args.includes.push(value(rest)?.into());
        } else if let Some(rest) = arg.strip_prefix("--descriptor_set_out") {
            args.out = Some(value(rest)?.into());
        } else if let Some(rest) = arg.strip_prefix("-I") {
            args.includes.push(value(rest)?.into());
        } else if let Some(rest) = arg.strip_prefix("-o") {
            args.out = Some(value(rest)?.into());
        } else if arg.starts_with('-') {
            return Err(format!("{arg}: not a flag this protoc knows"));
        } else {
            args.files.push(arg.into());
        }
    }
    Ok(args)
}

fn compile(args: Args) -> Result<(), String> {
    let out = args.out.ok_or("no --descriptor_set_out")?;
    let mut compiler = protox::Compiler::new(args.includes).map_err(|e| e.to_string())?;
    compiler
        .include_imports(args.include_imports)
        .include_source_info(args.include_source_info)
        .open_files(args.files)
        .map_err(|e| e.to_string())?;
    std::fs::write(&out, compiler.encode_file_descriptor_set())
        .map_err(|e| format!("{}: {e}", out.display()))
}
