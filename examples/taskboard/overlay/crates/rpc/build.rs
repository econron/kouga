fn main() {
    tonic_prost_build::compile_protos("../../proto/taskboard.proto")
        .expect("compile taskboard.proto");
}
