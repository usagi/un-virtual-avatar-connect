use std::{
	env,
	ffi::{CStr, CString},
	path::PathBuf,
	ptr,
	time::{Duration, Instant},
};

use vac_ocr_paddle_sys::{
	unvac_paddleocr_abi_version, unvac_paddleocr_engine_create, unvac_paddleocr_engine_destroy, unvac_paddleocr_last_error,
	unvac_paddleocr_recognize, unvac_paddleocr_text_free, UnvacPaddleOcrEngine, UnvacPaddleOcrImage, UnvacPaddleOcrOptions,
	UnvacPaddleOcrStatus, UnvacPaddleOcrText, UNVAC_PADDLEOCR_ABI_VERSION,
};

#[derive(Debug)]
struct Args {
	model_dir: PathBuf,
	loops: usize,
	width: u32,
	height: u32,
	channels: u32,
	lang: String,
}

fn main() {
	let args = match parse_args() {
		Ok(args) => args,
		Err(e) => {
			eprintln!("{e}");
			print_help();
			std::process::exit(2);
		}
	};

	if let Err(e) = run(args) {
		eprintln!("vac-ocr-paddle-eval: {e}");
		std::process::exit(1);
	}
}

fn run(args: Args) -> Result<(), String> {
	let abi = unsafe { unvac_paddleocr_abi_version() };
	println!("abi_version={abi} expected={UNVAC_PADDLEOCR_ABI_VERSION}");
	if abi != UNVAC_PADDLEOCR_ABI_VERSION {
		return Err(format!("ABI mismatch: dll={abi} rust={UNVAC_PADDLEOCR_ABI_VERSION}"));
	}

	let model_dir = CString::new(args.model_dir.to_string_lossy().as_bytes()).map_err(|_| "model_dir contains NUL".to_string())?;
	let mut engine: *mut UnvacPaddleOcrEngine = ptr::null_mut();
	let create_start = Instant::now();
	let status = unsafe { unvac_paddleocr_engine_create(model_dir.as_ptr(), &mut engine) };
	let create_elapsed = create_start.elapsed();
	print_status("engine_create", status, create_elapsed);
	if status != UnvacPaddleOcrStatus::Ok {
		println!("last_error={}", last_error());
		return Ok(());
	}

	let lang = CString::new(args.lang).map_err(|_| "lang contains NUL".to_string())?;
	let channels = args.channels.max(1);
	let stride = args.width.saturating_mul(channels);
	let frame_len = stride.saturating_mul(args.height) as usize;
	let frame = vec![255_u8; frame_len];
	let image = UnvacPaddleOcrImage {
		data: frame.as_ptr(),
		width: args.width,
		height: args.height,
		stride,
		channels,
	};
	let options = UnvacPaddleOcrOptions {
		language: lang.as_ptr(),
		roi_x: 0,
		roi_y: 0,
		roi_w: 0,
		roi_h: 0,
		return_regions: 1,
	};

	let mut ok = 0_usize;
	let mut not_ready = 0_usize;
	let mut invalid = 0_usize;
	let mut runtime_error = 0_usize;
	let mut total = Duration::ZERO;

	for _ in 0..args.loops {
		let mut text = UnvacPaddleOcrText { text: ptr::null(), len: 0 };
		let start = Instant::now();
		let status = unsafe { unvac_paddleocr_recognize(engine, &image, &options, &mut text) };
		total += start.elapsed();
		match status {
			UnvacPaddleOcrStatus::Ok => {
				ok += 1;
				if !text.text.is_null() {
					let out = unsafe { CStr::from_ptr(text.text) }.to_string_lossy();
					println!("text={out}");
				}
				unsafe { unvac_paddleocr_text_free(text) };
			}
			UnvacPaddleOcrStatus::NotReady => not_ready += 1,
			UnvacPaddleOcrStatus::InvalidArgument => invalid += 1,
			UnvacPaddleOcrStatus::RuntimeError => runtime_error += 1,
		}
	}

	let avg_ms = if args.loops == 0 {
		0.0
	} else {
		total.as_secs_f64() * 1000.0 / args.loops as f64
	};
	println!(
		"recognize_summary loops={} ok={} not_ready={} invalid_argument={} runtime_error={} total_ms={:.3} avg_ms={:.3}",
		args.loops,
		ok,
		not_ready,
		invalid,
		runtime_error,
		total.as_secs_f64() * 1000.0,
		avg_ms
	);
	if ok != args.loops {
		println!("last_error={}", last_error());
	}

	unsafe { unvac_paddleocr_engine_destroy(engine) };
	Ok(())
}

fn print_status(label: &str, status: UnvacPaddleOcrStatus, elapsed: Duration) {
	println!("{label} status={status:?} elapsed_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
}

fn last_error() -> String {
	let ptr = unsafe { unvac_paddleocr_last_error() };
	if ptr.is_null() {
		return String::new();
	}
	unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

fn parse_args() -> Result<Args, String> {
	let mut args = env::args().skip(1);
	let mut out = Args {
		model_dir: PathBuf::from(".vendors/paddleocr-native/models"),
		loops: 1,
		width: 640,
		height: 360,
		channels: 3,
		lang: "japan".into(),
	};

	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--model-dir" => out.model_dir = PathBuf::from(args.next().ok_or("missing value for --model-dir")?),
			"--loops" => out.loops = parse_next(&mut args, "--loops")?,
			"--width" => out.width = parse_next(&mut args, "--width")?,
			"--height" => out.height = parse_next(&mut args, "--height")?,
			"--channels" => out.channels = parse_next(&mut args, "--channels")?,
			"--lang" => out.lang = args.next().ok_or("missing value for --lang")?,
			"-h" | "--help" => {
				print_help();
				std::process::exit(0);
			}
			other => return Err(format!("unknown argument: {other}")),
		}
	}

	Ok(out)
}

fn parse_next<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> Result<T, String> {
	args.next()
		.ok_or_else(|| format!("missing value for {name}"))?
		.parse()
		.map_err(|_| format!("invalid value for {name}"))
}

fn print_help() {
	println!(
		"vac-ocr-paddle-eval\n\n\
Usage:\n\
  vac-ocr-paddle-eval [--model-dir <path>] [--loops <n>] [--width <px>] [--height <px>] [--channels <n>] [--lang <name>]\n\n\
This binary is a development-only PaddleOCR native DLL probe. Use `cargo run -p xtask -- paddleocr-native eval` to set PATH and link variables."
	);
}
