use clap::{parser::ValueSource, ArgAction, CommandFactory};
use std::ffi::OsString;

/// Re-encode explicitly supplied options using the actual startup grammar.
/// A value belonging to an option must never be mistaken for positional CONF.
pub(super) fn inherited_options(argv: impl IntoIterator<Item = OsString>) -> Result<Vec<OsString>, String> {
	let command = crate::Args::command();
	let matches = command
		.clone()
		.try_get_matches_from(argv)
		.map_err(|_| "起動引数を再確認できないため、再起動を中止しました".to_string())?;
	let mut result = Vec::new();
	for argument in command.get_arguments() {
		let id = argument.get_id().as_str();
		if id == "CONF" || id == "restart_parent_pid" || matches.value_source(id) != Some(ValueSource::CommandLine) {
			continue;
		}
		let long = argument.get_long().ok_or_else(|| format!("再起動で引き継げないオプション: {id}"))?;
		match argument.get_action() {
			ArgAction::SetTrue | ArgAction::SetFalse => result.push(format!("--{long}").into()),
			ArgAction::Set => {
				let values: Vec<_> = matches.get_raw(id).into_iter().flatten().collect();
				if values.len() != 1 {
					return Err(format!("再起動で引き継げない値数: {id}"));
				}
				let mut encoded = OsString::from(format!("--{long}="));
				encoded.push(values[0]);
				result.push(encoded);
			}
			_ => return Err(format!("再起動で引き継げないオプション形式: {id}")),
		}
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::Parser;

	#[test]
	fn restart_preserves_option_values_and_replaces_only_conf_and_parent() {
		for argv in [
			vec!["vac", "--persona-id", "partner", "old.toml", "-D", "--restart-parent-pid", "123"],
			vec!["vac", "--persona-id", "partner", "-D"],
			vec!["vac", "--persona-id=partner", "--restart-parent-pid=123", "-D", "old.toml"],
		] {
			let inherited = inherited_options(argv.into_iter().map(OsString::from)).unwrap();
			let mut restarted: Vec<OsString> = ["vac", "new.toml", "--restart-parent-pid=456"]
				.into_iter()
				.map(Into::into)
				.collect();
			restarted.extend(inherited);
			let parsed = crate::Args::try_parse_from(restarted).unwrap();
			assert_eq!(parsed.conf, "new.toml");
			assert_eq!(parsed.persona_id.as_deref(), Some("partner"));
			assert!(parsed.debug);
			assert_eq!(parsed.restart_parent_pid, Some(456));
		}
	}

	#[test]
	fn defaults_are_not_added_and_delimited_conf_is_removed() {
		for argv in [vec!["vac"], vec!["vac", "--", "-custom.toml"]] {
			assert!(inherited_options(argv.into_iter().map(OsString::from)).unwrap().is_empty());
		}
	}

	#[test]
	fn unicode_spaces_and_hyphen_values_roundtrip() {
		for value in ["共演者 その2", "-partner", "a=b"] {
			let inherited = inherited_options([OsString::from("vac"), OsString::from(format!("--persona-id={value}"))]).unwrap();
			let parsed =
				crate::Args::try_parse_from([OsString::from("vac"), OsString::from("next.toml")].into_iter().chain(inherited)).unwrap();
			assert_eq!(parsed.persona_id.as_deref(), Some(value));
		}
	}

	#[test]
	fn invalid_arguments_are_rejected_before_spawn() {
		assert!(inherited_options(["vac", "--persona-id"].into_iter().map(OsString::from)).is_err());
	}
}
