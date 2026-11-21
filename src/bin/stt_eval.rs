fn main() {
	if let Err(error) = virtual_avatar_connect::stt_eval::run() {
		eprintln!("vac-stt-eval: {error:#}");
		std::process::exit(1);
	}
}
