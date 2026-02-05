use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=../client/Cargo.toml");
    println!("cargo:rerun-if-changed=../client/src");
    println!("cargo:rerun-if-changed=../client/web/index.html");
    println!("cargo:rerun-if-changed=../client/web/tailwind.input.css");
    
    println!("cargo:warning=Building WASM client...");
    
    // Run wasm-pack directly like client.sh does, but use separate target dir to avoid file lock
    let mut cmd = Command::new("wasm-pack");
    cmd.args(["build", "--target", "web", "--out-dir", "./web/pkg", "--target-dir", "../../target/wasm"])
       .current_dir("../client");
    
    let status = cmd.status().expect("Failed to execute wasm-pack");
    
    if !status.success() {
        panic!("WASM client build failed");
    }

    println!("cargo:warning=WASM client built successfully - assets will be embedded");
}