//! 再起動 API のパス解決・プロファイル一覧用ラベル・パス同値判定。

use std::path::{Path, PathBuf};

/// 新 conf path の安全性を検証したうえで絶対パスに解決する。
///
/// 受け入れるパターン:
///   - 絶対パス: 現設定フォルダー内、または管理領域内の正規プロファイルのconf.toml
///   - 相対: 現 conf の親ディレクトリからの相対（パス区切りを含むなら親ディレクトリ制約のみ）
///     - `..` で親を辿って現ディレクトリ外に脱出する指定は弾く
pub(crate) fn resolve_new_conf_path(current_conf: &Path, requested: &str) -> Result<PathBuf, String> {
	let root = crate::profile_store::user_config_root().ok();
	resolve_new_conf_path_at(current_conf, requested, root.as_deref())
}

fn resolve_new_conf_path_at(current_conf: &Path, requested: &str, managed_root: Option<&Path>) -> Result<PathBuf, String> {
	let p = Path::new(requested);
	let resolved = if p.is_absolute() {
		p.to_path_buf()
	} else {
		let parent = current_conf
			.parent()
			.ok_or_else(|| "current conf has no parent directory".to_string())?;
		parent.join(p)
	};

	// 正規化して脱出チェック。canonicalize は存在チェック付き。
	let canon = std::fs::canonicalize(&resolved).map_err(|e| format!("cannot resolve conf path {}: {e}", resolved.display()))?;
	if crate::profile_store::is_managed_metadata_path(&canon) {
		return Err("管理情報ファイルは起動用の設定として使えません。".to_string());
	}

	let parent_canon = current_conf.parent().and_then(|p| std::fs::canonicalize(p).ok());
	if let Some(parent_canon) = parent_canon {
		let managed_target = p.is_absolute() && managed_root.is_some_and(|root| crate::profile_store::lifecycle::managed_conf_at(root, &canon).is_some());
		if !canon.starts_with(&parent_canon) && !managed_target {
			return Err(format!(
				"conf path {} is outside the current conf directory ({})",
				canon.display(),
				parent_canon.display()
			));
		}
	}

	if canon.extension().and_then(|e| e.to_str()) != Some("toml") {
		return Err(format!("conf path {} is not a .toml file", canon.display()));
	}
	if !canon.is_file() {
		return Err(format!("conf path {} is not a regular file", canon.display()));
	}

	Ok(canon)
}

/// `conf.example-twitch.toml` → "Conf Example Twitch" 程度。過度に凝らない。
pub(crate) fn label_from_filename(filename: &str) -> String {
	let stem = filename.trim_end_matches(".toml");
	// ドット/ダッシュ/アンダースコアで区切って単語化し、先頭大文字化。
	stem.split(['.', '-', '_'])
		.filter(|s| !s.is_empty())
		.map(|s| {
			let mut chars = s.chars();
			match chars.next() {
				Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
				None => String::new(),
			}
		})
		.collect::<Vec<_>>()
		.join(" ")
}

/// ディレクトリ/シンボリックリンク/大文字小文字を正規化したうえで 2 つのパスを比較する。
/// 失敗時は素の比較にフォールバック。
pub(crate) fn paths_equivalent(a: &Path, b: &Path) -> bool {
	match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
		(Ok(aa), Ok(bb)) => aa == bb,
		_ => a == b,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn managed_profile_switch_preserves_path_boundaries() {
		let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
		let fixture = std::env::temp_dir().join(format!("vac-restart-path-{}-{nonce}", std::process::id()));
		let root = fixture.join("store");
		let current = root.join("profiles/first/conf.toml");
		let next = root.join("profiles/second/conf.toml");
		let outside = fixture.join("outside/conf.toml");
		let nested = root.join("profiles/second/nested/conf.toml");
		let metadata = root.join("profiles/second/profile.toml");
		for file in [&current, &next, &outside, &nested, &metadata] {
			std::fs::create_dir_all(file.parent().unwrap()).unwrap();
			std::fs::write(file, "workers = 1\n").unwrap();
		}
		assert_eq!(resolve_new_conf_path_at(&current, &next.to_string_lossy(), Some(&root)).unwrap(), std::fs::canonicalize(&next).unwrap());
		assert_eq!(resolve_new_conf_path_at(&current, "conf.toml", Some(&root)).unwrap(), std::fs::canonicalize(&current).unwrap());
		for file in [&next, &outside, &nested, &metadata] {
			assert!(resolve_new_conf_path_at(&current, &file.to_string_lossy(), None).is_err());
		}
		for file in [&outside, &nested, &metadata] {
			assert!(resolve_new_conf_path_at(&current, &file.to_string_lossy(), Some(&root)).is_err());
		}
		assert!(resolve_new_conf_path_at(&current, "../second/conf.toml", Some(&root)).is_err());
		assert!(resolve_new_conf_path_at(&current, &next.to_string_lossy(), Some(&fixture.join("another-store"))).is_err());
		std::fs::remove_dir_all(fixture).unwrap();
	}

	#[test]
	fn label_from_filename_basic() {
		assert_eq!(label_from_filename("conf.toml"), "Conf");
		assert_eq!(label_from_filename("conf.example-twitch.toml"), "Conf Example Twitch");
		assert_eq!(label_from_filename("streaming_kaltsit.toml"), "Streaming Kaltsit");
		assert_eq!(label_from_filename(".toml"), "");
	}
}
