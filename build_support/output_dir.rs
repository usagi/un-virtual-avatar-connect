use std::path::{Path, PathBuf};

/// Resolve Cargo's build-script output layout, without assuming a fixed depth.
/// Cargo 1.100: <profile>/build/<package>/<hash>/out.
/// Older Cargo: <profile>/build/<package>-<hash>/out.
pub fn profile_dir_from_out_dir(out: &Path) -> Option<PathBuf> {
	if out.file_name()? != "out" {
		return None;
	}
	let unit = out.parent()?;
	let parent = unit.parent()?;
	let build = if let Some(candidate) = parent.parent().filter(|p| p.file_name().is_some_and(|n| n == "build")) {
		candidate
	} else if parent.file_name()? == "build" {
		parent
	} else {
		return None;
	};
	Some(build.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn resolves_old_and_new_layouts_without_changing_profile_or_target() {
		for (out, expected) in [
			("target/debug/build/vac-123/out", "target/debug"),
			("target/debug/build/vac/123/out", "target/debug"),
			("custom/release/build/un-motion-supervisor/123/out", "custom/release"),
			(
				"custom/x86_64-pc-windows-msvc/review/build/vac/123/out",
				"custom/x86_64-pc-windows-msvc/review",
			),
			("build/debug/build/build/123/out", "build/debug"),
		] {
			assert_eq!(profile_dir_from_out_dir(Path::new(out)), Some(PathBuf::from(expected)));
		}
	}
	#[test]
	fn rejects_unknown_layouts_instead_of_copying_into_the_wrong_directory() {
		for out in [
			"target/debug/deps/foo",
			"target/debug/build/vac/123",
			"target/debug/unknown/vac/123/out",
		] {
			assert_eq!(profile_dir_from_out_dir(Path::new(out)), None);
		}
	}
}
