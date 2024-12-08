use std::env;
use std::path::PathBuf;

fn main() {
  println!("cargo:rustc-link-lib=zmq");

  let mut bindings = bindgen::Builder::default()
    .header("include/zmq.h")
    .derive_default(true)
    .parse_callbacks(Box::new(bindgen::CargoCallbacks));
  if let Ok(x) = std::env::var("REZMQ_NATIVE_SYSROOT") {
    bindings = bindings.clang_arg(format!("--sysroot={}", x));
  }
  let bindings = bindings.generate().expect("Unable to generate bindings");

  let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
  bindings
    .write_to_file(out_path.join("bindings.rs"))
    .expect("Couldn't write bindings!");
}
