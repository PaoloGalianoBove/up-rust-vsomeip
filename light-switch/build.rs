// build.rs
fn main() {
    tonic_prost_build::compile_protos("protos/light-switch.proto").unwrap();
}