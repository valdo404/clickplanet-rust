fn main() {
    prost_build::compile_protos(&["proto/clicks.proto"], &["proto/"])
        .unwrap();
}
