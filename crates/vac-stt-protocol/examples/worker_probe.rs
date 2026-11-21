use std::io::BufReader;
use std::process::{Command, Stdio};
use vac_stt_protocol::{read_frame, write_frame, WorkerRequest, WorkerResponse, PROTOCOL_VERSION};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut args = std::env::args_os().skip(1);
	let worker = args
		.next()
		.ok_or("usage: worker_probe <worker> <model> <wav> [language] [threads] [runs] [start_ms] [end_ms]")?;
	let model = args.next().ok_or("model path is required")?;
	let wav = args.next().ok_or("WAV path is required")?;
	let language = args.next().unwrap_or_else(|| "ja".into());
	let threads = args.next().map(|value| value.to_string_lossy().parse::<usize>()).transpose()?;
	let runs = args
		.next()
		.map(|value| value.to_string_lossy().parse::<u64>())
		.transpose()?
		.unwrap_or(1)
		.max(1);
	let start_ms = args
		.next()
		.map(|value| value.to_string_lossy().parse::<usize>())
		.transpose()?
		.unwrap_or(0);
	let end_ms = args.next().map(|value| value.to_string_lossy().parse::<usize>()).transpose()?;

	let mut wav_reader = hound::WavReader::open(wav)?;
	let spec = wav_reader.spec();
	if spec.sample_rate != 16_000 || spec.channels != 1 || spec.bits_per_sample != 16 {
		return Err(format!(
			"probe requires 16 kHz mono 16-bit PCM WAV, got {} Hz {} ch {} bit",
			spec.sample_rate, spec.channels, spec.bits_per_sample
		)
		.into());
	}
	let pcm_16khz = wav_reader
		.samples::<i16>()
		.map(|sample| sample.map(|value| value as f32 / 32_768.0))
		.collect::<Result<Vec<_>, _>>()?;
	let start_sample = start_ms.saturating_mul(16).min(pcm_16khz.len());
	let end_sample = end_ms
		.map(|value| value.saturating_mul(16))
		.unwrap_or(pcm_16khz.len())
		.min(pcm_16khz.len());
	if end_sample <= start_sample {
		return Err(format!("invalid audio range: {start_ms}..{} ms", end_ms.unwrap_or(pcm_16khz.len() / 16)).into());
	}
	let pcm_16khz = pcm_16khz[start_sample..end_sample].to_vec();

	let mut command = Command::new(worker);
	command.arg("--model").arg(model).arg("--language").arg(language);
	if let Some(threads) = threads {
		command.arg("--threads").arg(threads.to_string());
	}
	let mut child = command
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::inherit())
		.spawn()?;
	let mut stdin = child.stdin.take().ok_or("worker stdin is unavailable")?;
	let mut stdout = BufReader::new(child.stdout.take().ok_or("worker stdout is unavailable")?);
	let ready = read_frame::<_, WorkerResponse>(&mut stdout)?;
	match &ready {
		WorkerResponse::Ready { protocol_version, .. } if *protocol_version == PROTOCOL_VERSION => {}
		_ => return Err(format!("unexpected handshake: {ready:?}").into()),
	}
	let mut transcripts = Vec::with_capacity(runs as usize);
	for request_id in 1..=runs {
		write_frame(
			&mut stdin,
			&WorkerRequest::Transcribe {
				request_id,
				pcm_16khz: pcm_16khz.clone(),
			},
		)?;
		transcripts.push(read_frame::<_, WorkerResponse>(&mut stdout)?);
	}
	write_frame(&mut stdin, &WorkerRequest::Shutdown)?;
	drop(stdin);
	let status = child.wait()?;
	if !status.success() {
		return Err(format!("worker exited with {status}").into());
	}
	println!("handshake={ready:?}");
	for transcript in transcripts {
		println!("response={transcript:?}");
	}
	Ok(())
}
