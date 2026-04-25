use protobuf_codegen::Customize;

fn main() {
    println!("Generating code from protobuf.");

    let customize = Customize::default().tokio_bytes(true);

    protobuf_codegen::Codegen::new()
        .customize(customize)
        .cargo_out_dir("protos/")
        .include("src/protos/")
        .input("src/protos/dest_header.proto")
        .run_from_script();
    println!("Finished generating code");
}
