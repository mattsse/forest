// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

fn main() {
    protoc_rust_grpc::run(protoc_rust_grpc::Args {
        out_dir: "src",
        includes: &["proto", "proto/api-common-protos"],
        input: &["proto/api.proto", "proto/common.proto"],
        rust_protobuf: true, // also generate protobuf messages, not just services
        ..Default::default()
    })
    .expect("protoc-rust-grpc");
}