fn main() {
    tonic_prost_build::compile_protos("proto/fixture.proto").expect("compile gRPC fixture");
}
