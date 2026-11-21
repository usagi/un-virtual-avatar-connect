use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum WorkerRequest {
	Transcribe { request_id: u64, pcm_16khz: Vec<f32> },
	Shutdown,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum WorkerResponse {
	Ready { protocol_version: u32, backend: String },
	Transcript { request_id: u64, text: String, inference_ms: u64 },
	Error { request_id: Option<u64>, message: String },
}

pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> io::Result<()> {
	let payload = rmp_serde::to_vec_named(value).map_err(io::Error::other)?;
	if payload.len() > MAX_FRAME_BYTES {
		return Err(io::Error::new(io::ErrorKind::InvalidData, "STT IPC frame is too large"));
	}
	writer.write_all(&(payload.len() as u32).to_le_bytes())?;
	writer.write_all(&payload)?;
	writer.flush()
}

pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> io::Result<T> {
	let mut len = [0_u8; 4];
	reader.read_exact(&mut len)?;
	let len = u32::from_le_bytes(len) as usize;
	if len > MAX_FRAME_BYTES {
		return Err(io::Error::new(io::ErrorKind::InvalidData, "STT IPC frame is too large"));
	}
	let mut payload = vec![0_u8; len];
	reader.read_exact(&mut payload)?;
	rmp_serde::from_slice(&payload).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trips_binary_pcm_frame() {
		let request = WorkerRequest::Transcribe {
			request_id: 7,
			pcm_16khz: vec![-1.0, 0.0, 0.5, 1.0],
		};
		let mut bytes = Vec::new();
		write_frame(&mut bytes, &request).unwrap();
		assert_eq!(read_frame::<_, WorkerRequest>(&mut bytes.as_slice()).unwrap(), request);
	}
}
