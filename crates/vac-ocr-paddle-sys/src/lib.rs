#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int};

pub const UNVAC_PADDLEOCR_ABI_VERSION: u32 = 1;

#[repr(C)]
pub struct UnvacPaddleOcrEngine {
	_private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnvacPaddleOcrStatus {
	Ok = 0,
	NotReady = 1,
	InvalidArgument = 2,
	RuntimeError = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnvacPaddleOcrImage {
	pub data: *const u8,
	pub width: u32,
	pub height: u32,
	pub stride: u32,
	pub channels: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnvacPaddleOcrOptions {
	pub language: *const c_char,
	pub roi_x: c_int,
	pub roi_y: c_int,
	pub roi_w: c_int,
	pub roi_h: c_int,
	pub return_regions: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UnvacPaddleOcrText {
	pub text: *const c_char,
	pub len: u32,
}

#[rustfmt::skip]
unsafe extern "C" {
	pub fn unvac_paddleocr_abi_version() -> u32;
	pub fn unvac_paddleocr_engine_create(model_dir: *const c_char, out_engine: *mut *mut UnvacPaddleOcrEngine) -> UnvacPaddleOcrStatus;
	pub fn unvac_paddleocr_engine_destroy(engine: *mut UnvacPaddleOcrEngine);
	pub fn unvac_paddleocr_recognize(
		engine: *mut UnvacPaddleOcrEngine,
		image: *const UnvacPaddleOcrImage,
		options: *const UnvacPaddleOcrOptions,
		out_text: *mut UnvacPaddleOcrText,
	) -> UnvacPaddleOcrStatus;
	pub fn unvac_paddleocr_text_free(text: UnvacPaddleOcrText);
	pub fn unvac_paddleocr_last_error() -> *const c_char;
}
