use std::{env, path::PathBuf};

fn main() {
	println!("cargo:rerun-if-env-changed=UNVAC_PADDLEOCR_LIB_DIR");
	println!("cargo:rerun-if-env-changed=UNVAC_PADDLEOCR_LINK");

	let should_link = env::var_os("CARGO_FEATURE_LINK").is_some() || env::var_os("UNVAC_PADDLEOCR_LINK").is_some();
	if !should_link {
		return;
	}

	let lib_dir = env::var_os("UNVAC_PADDLEOCR_LIB_DIR")
		.map(PathBuf::from)
		.unwrap_or_else(|| PathBuf::from("../../.vendors/paddleocr-native/install/lib"));

	println!("cargo:rustc-link-search=native={}", lib_dir.display());
	println!("cargo:rustc-link-lib=dylib=unvac_paddleocr");
}
