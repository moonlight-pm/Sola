//! Signal's protos (`proto/`, copied from Signal-Android) into Rust, by
//! protox: no `protoc` on the machine.

const PROTOS: &[&str] =
    &["Provisioning.proto", "SignalService.proto", "DeviceName.proto", "Groups.proto", "StorageService.proto"];

fn main() {
    println!("cargo:rerun-if-changed=proto");
    let descriptors = protox::compile(PROTOS, ["proto"]).expect("Signal's protos compile");
    prost_build::Config::new()
        .compile_fds(descriptors)
        .expect("Signal's protos generate");
}
