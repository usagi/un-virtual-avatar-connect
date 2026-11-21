use std::{env, error::Error, fs, path::PathBuf, time::Instant};

use image::{imageops::FilterType, ImageReader, RgbImage};
use ort::{session::Session, value::Tensor};

#[derive(Debug)]
struct Args {
	model: Option<PathBuf>,
	det_model: Option<PathBuf>,
	rec_model: Option<PathBuf>,
	cls_model: Option<PathBuf>,
	image: Option<PathBuf>,
	dict: Option<PathBuf>,
	pipeline: bool,
	run_dummy: bool,
	decode_det: bool,
	decode_rec: bool,
	decode_cls: bool,
	det_threshold: f32,
	det_min_area: usize,
	max_boxes: usize,
	merge_lines: bool,
	line_gap: usize,
	rec_bgr: bool,
	ctc_class_offset: usize,
	preprocess: PreprocessProfile,
	width: usize,
	height: usize,
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
		eprintln!("vac-ocr-onnx-eval: {e}");
		std::process::exit(1);
	}
}

fn run(args: Args) -> Result<(), Box<dyn Error>> {
	if args.pipeline {
		return run_pipeline(args);
	}

	let model = args.model.as_ref().ok_or("--model is required unless --pipeline is used")?;
	let start = Instant::now();
	let mut session = Session::builder()?.commit_from_file(model)?;
	let elapsed = start.elapsed();
	println!("model={}", model.display());
	println!("session_load_ms={:.3}", elapsed.as_secs_f64() * 1000.0);

	println!("inputs={}", session.inputs().len());
	for input in session.inputs() {
		println!("  input name={} dtype={}", input.name(), input.dtype());
	}

	println!("outputs={}", session.outputs().len());
	for output in session.outputs() {
		println!("  output name={} dtype={}", output.name(), output.dtype());
	}

	if args.run_dummy {
		run_dummy(&mut session, args.width, args.height)?;
	}

	if let Some(image) = &args.image {
		run_image(
			&mut session,
			image,
			args.width,
			args.height,
			DecodeOptions {
				decode_det: args.decode_det,
				decode_rec: args.decode_rec,
				decode_cls: args.decode_cls,
				det_threshold: args.det_threshold,
				det_min_area: args.det_min_area,
				preprocess: args.preprocess,
				dict_path: args.dict.as_ref(),
				ctc_class_offset: args.ctc_class_offset,
			},
		)?;
	}

	Ok(())
}

fn run_pipeline(args: Args) -> Result<(), Box<dyn Error>> {
	let image_path = args.image.as_ref().ok_or("--image is required with --pipeline")?;
	let det_model = args.det_model.as_ref().ok_or("--det-model is required with --pipeline")?;
	let rec_model = args.rec_model.as_ref().ok_or("--rec-model is required with --pipeline")?;
	let cls_model = args.cls_model.as_ref().ok_or("--cls-model is required with --pipeline")?;
	let dict_path = args.dict.as_ref().ok_or("--dict is required with --pipeline")?;
	let charset = load_rec_dict(dict_path)?;
	let original = ImageReader::open(image_path)?.decode()?.to_rgb8();
	let original_width = original.width() as usize;
	let original_height = original.height() as usize;

	let mut det_session = Session::builder()?.commit_from_file(det_model)?;
	let mut rec_session = Session::builder()?.commit_from_file(rec_model)?;
	let mut cls_session = Session::builder()?.commit_from_file(cls_model)?;

	let det_tensor = rgb_to_chw_tensor(&original, args.width, args.height, PreprocessProfile::Det)?;
	let det_start = Instant::now();
	let det_outputs = det_session.run(ort::inputs![det_tensor])?;
	let det_elapsed = det_start.elapsed();
	let (stats, mut boxes) = decode_det_output(&det_outputs[0], args.det_threshold, args.det_min_area)?;
	if args.merge_lines {
		boxes = merge_line_boxes(boxes, args.line_gap);
	}
	boxes.sort_by_key(|bbox| (bbox.y / 16, bbox.x));
	boxes.truncate(args.max_boxes);

	println!("pipeline_image={}", image_path.display());
	println!("pipeline_source_size={}x{}", original_width, original_height);
	println!("det_infer_ms={:.3}", det_elapsed.as_secs_f64() * 1000.0);
	println!(
		"det_stats min={:.6} max={:.6} mean={:.6} active_pixels={}",
		stats.min, stats.max, stats.mean, stats.active_pixels
	);
	println!("pipeline_merge_lines={}", args.merge_lines);
	println!("pipeline_boxes={}", boxes.len());

	for (index, bbox) in boxes.iter().enumerate() {
		let crop_rect = map_box_to_source(bbox, args.width, args.height, original_width, original_height);
		let crop = image::imageops::crop_imm(
			&original,
			crop_rect.x as u32,
			crop_rect.y as u32,
			crop_rect.width as u32,
			crop_rect.height as u32,
		)
		.to_image();
		let (angle, cls_confidence) = run_cls(&mut cls_session, &crop)?;
		let rec_input = if angle == "180" { image::imageops::rotate180(&crop) } else { crop };
		let (text, rec_confidence) = run_rec(&mut rec_session, &rec_input, &charset, args.rec_bgr, args.ctc_class_offset)?;
		println!(
			"  line[{index}] x={} y={} width={} height={} angle={} cls_confidence={:.6} rec_confidence={:.6} text={}",
			crop_rect.x, crop_rect.y, crop_rect.width, crop_rect.height, angle, cls_confidence, rec_confidence, text
		);
	}

	Ok(())
}

fn run_dummy(session: &mut Session, width: usize, height: usize) -> ort::Result<()> {
	let len = 3 * width * height;
	let data = vec![0.0_f32; len];
	let tensor = Tensor::<f32>::from_array(([1, 3, height, width], data))?;
	let start = Instant::now();
	let outputs = session.run(ort::inputs![tensor])?;
	let elapsed = start.elapsed();
	println!("dummy_infer_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
	println!("dummy_outputs={}", outputs.len());
	for (name, value) in outputs.iter() {
		println!("  output name={} dtype={}", name, value.dtype());
	}
	Ok(())
}

struct DecodeOptions<'a> {
	decode_det: bool,
	decode_rec: bool,
	decode_cls: bool,
	det_threshold: f32,
	det_min_area: usize,
	preprocess: PreprocessProfile,
	dict_path: Option<&'a PathBuf>,
	ctc_class_offset: usize,
}

#[derive(Debug, Clone, Copy)]
enum PreprocessProfile {
	Det,
	RecRgb,
	RecBgr,
}

fn run_image(
	session: &mut Session,
	image_path: &PathBuf,
	width: usize,
	height: usize,
	options: DecodeOptions<'_>,
) -> Result<(), Box<dyn Error>> {
	let tensor = image_to_chw_tensor(image_path, width, height, options.preprocess)?;
	let start = Instant::now();
	let outputs = session.run(ort::inputs![tensor])?;
	let elapsed = start.elapsed();
	println!("image={}", image_path.display());
	println!("image_infer_ms={:.3}", elapsed.as_secs_f64() * 1000.0);
	println!("image_outputs={}", outputs.len());
	for (name, value) in outputs.iter() {
		println!("  output name={} dtype={}", name, value.dtype());
	}

	if options.decode_det {
		let (stats, boxes) = decode_det_output(&outputs[0], options.det_threshold, options.det_min_area)?;
		println!(
			"det_stats min={:.6} max={:.6} mean={:.6} active_pixels={}",
			stats.min, stats.max, stats.mean, stats.active_pixels
		);
		println!("det_boxes={}", boxes.len());
		for (i, bbox) in boxes.iter().enumerate() {
			println!(
				"  box[{i}] x={} y={} width={} height={} area={} score={:.6}",
				bbox.x, bbox.y, bbox.width, bbox.height, bbox.area, bbox.score
			);
		}
	}

	if options.decode_rec {
		let dict_path = options.dict_path.ok_or("--dict is required with --decode-rec")?;
		let charset = load_rec_dict(dict_path)?;
		let (text, confidence) = decode_rec_output(&outputs[0], &charset, options.ctc_class_offset)?;
		println!("rec_text={text}");
		println!("rec_confidence={confidence:.6}");
	}

	if options.decode_cls {
		let (angle, confidence) = decode_cls_output(&outputs[0])?;
		println!("cls_angle={angle}");
		println!("cls_confidence={confidence:.6}");
	}

	Ok(())
}

fn run_rec(
	session: &mut Session,
	image: &RgbImage,
	charset: &[String],
	bgr: bool,
	ctc_class_offset: usize,
) -> Result<(String, f32), Box<dyn Error>> {
	let width = rec_width_for_crop(image);
	let profile = if bgr {
		PreprocessProfile::RecBgr
	} else {
		PreprocessProfile::RecRgb
	};
	let tensor = rgb_to_chw_tensor(image, width, 48, profile)?;
	let outputs = session.run(ort::inputs![tensor])?;
	decode_rec_output(&outputs[0], charset, ctc_class_offset)
}

fn run_cls(session: &mut Session, image: &RgbImage) -> Result<(&'static str, f32), Box<dyn Error>> {
	let tensor = rgb_to_chw_tensor(image, 320, 48, PreprocessProfile::RecRgb)?;
	let outputs = session.run(ort::inputs![tensor])?;
	decode_cls_output(&outputs[0])
}

fn rec_width_for_crop(image: &RgbImage) -> usize {
	let width = image.width().max(1) as usize;
	let height = image.height().max(1) as usize;
	let scaled = (width * 48).div_ceil(height);
	let clamped = scaled.clamp(32, 960);
	clamped.div_ceil(8) * 8
}

#[derive(Debug)]
struct DetBox {
	x: usize,
	y: usize,
	width: usize,
	height: usize,
	area: usize,
	score: f32,
}

fn merge_line_boxes(mut boxes: Vec<DetBox>, line_gap: usize) -> Vec<DetBox> {
	boxes.sort_by_key(|bbox| (bbox.y + bbox.height / 2, bbox.x));
	let mut merged: Vec<DetBox> = Vec::new();
	for bbox in boxes {
		let Some(last) = merged.last_mut() else {
			merged.push(bbox);
			continue;
		};
		let last_center = last.y + last.height / 2;
		let bbox_center = bbox.y + bbox.height / 2;
		let same_line = last_center.abs_diff(bbox_center) <= last.height.max(bbox.height);
		let last_right = last.x + last.width;
		let close_enough = bbox.x <= last_right + line_gap;
		if same_line && close_enough {
			let x = last.x.min(bbox.x);
			let y = last.y.min(bbox.y);
			let right = last_right.max(bbox.x + bbox.width);
			let bottom = (last.y + last.height).max(bbox.y + bbox.height);
			let total_area = last.area + bbox.area;
			last.score = ((last.score * last.area as f32) + (bbox.score * bbox.area as f32)) / total_area as f32;
			last.x = x;
			last.y = y;
			last.width = right - x;
			last.height = bottom - y;
			last.area = total_area;
		} else {
			merged.push(bbox);
		}
	}
	merged
}

#[derive(Debug)]
struct Rect {
	x: usize,
	y: usize,
	width: usize,
	height: usize,
}

fn map_box_to_source(bbox: &DetBox, det_width: usize, det_height: usize, source_width: usize, source_height: usize) -> Rect {
	let x = bbox.x * source_width / det_width;
	let y = bbox.y * source_height / det_height;
	let right = ((bbox.x + bbox.width) * source_width / det_width).min(source_width);
	let bottom = ((bbox.y + bbox.height) * source_height / det_height).min(source_height);
	let pad_x = ((right.saturating_sub(x)) / 10).max(2);
	let pad_y = ((bottom.saturating_sub(y)) / 2).max(2);
	let x = x.saturating_sub(pad_x);
	let y = y.saturating_sub(pad_y);
	let right = (right + pad_x).min(source_width);
	let bottom = (bottom + pad_y).min(source_height);
	Rect {
		x,
		y,
		width: (right - x).max(1),
		height: (bottom - y).max(1),
	}
}

#[derive(Debug)]
struct DetStats {
	min: f32,
	max: f32,
	mean: f32,
	active_pixels: usize,
}

fn decode_det_output(value: &ort::value::DynValue, threshold: f32, min_area: usize) -> Result<(DetStats, Vec<DetBox>), Box<dyn Error>> {
	let (shape, data) = value.try_extract_tensor::<f32>()?;
	let dims = shape.as_ref();
	if dims.len() != 4 || dims[0] != 1 || dims[1] != 1 {
		return Err(format!("expected det output shape [1,1,H,W], got {dims:?}").into());
	}
	let height = dims[2] as usize;
	let width = dims[3] as usize;
	let mut min = f32::INFINITY;
	let mut max = f32::NEG_INFINITY;
	let mut sum = 0.0_f32;
	let mut active_pixels = 0_usize;
	for &value in data {
		min = min.min(value);
		max = max.max(value);
		sum += value;
		if value >= threshold {
			active_pixels += 1;
		}
	}
	let stats = DetStats {
		min,
		max,
		mean: sum / data.len() as f32,
		active_pixels,
	};
	let mut visited = vec![false; width * height];
	let mut boxes = Vec::new();

	for y in 0..height {
		for x in 0..width {
			let start = y * width + x;
			if visited[start] || data[start] < threshold {
				continue;
			}
			let mut stack = vec![(x, y)];
			visited[start] = true;
			let mut min_x = x;
			let mut max_x = x;
			let mut min_y = y;
			let mut max_y = y;
			let mut area = 0_usize;
			let mut score_sum = 0.0_f32;

			while let Some((cx, cy)) = stack.pop() {
				let idx = cy * width + cx;
				area += 1;
				score_sum += data[idx];
				min_x = min_x.min(cx);
				max_x = max_x.max(cx);
				min_y = min_y.min(cy);
				max_y = max_y.max(cy);

				for (nx, ny) in neighbors4(cx, cy, width, height) {
					let nidx = ny * width + nx;
					if !visited[nidx] && data[nidx] >= threshold {
						visited[nidx] = true;
						stack.push((nx, ny));
					}
				}
			}

			if area >= min_area {
				boxes.push(DetBox {
					x: min_x,
					y: min_y,
					width: max_x - min_x + 1,
					height: max_y - min_y + 1,
					area,
					score: score_sum / area as f32,
				});
			}
		}
	}

	boxes.sort_by(|a, b| b.area.cmp(&a.area));
	Ok((stats, boxes))
}

fn neighbors4(x: usize, y: usize, width: usize, height: usize) -> impl Iterator<Item = (usize, usize)> {
	let mut out = [(usize::MAX, usize::MAX); 4];
	let mut len = 0;
	if x > 0 {
		out[len] = (x - 1, y);
		len += 1;
	}
	if y > 0 {
		out[len] = (x, y - 1);
		len += 1;
	}
	if x + 1 < width {
		out[len] = (x + 1, y);
		len += 1;
	}
	if y + 1 < height {
		out[len] = (x, y + 1);
		len += 1;
	}
	out.into_iter().take(len)
}

fn image_to_chw_tensor(
	image_path: &PathBuf,
	width: usize,
	height: usize,
	profile: PreprocessProfile,
) -> Result<Tensor<f32>, Box<dyn Error>> {
	let img = ImageReader::open(image_path)?.decode()?.to_rgb8();
	rgb_to_chw_tensor(&img, width, height, profile)
}

fn rgb_to_chw_tensor(img: &RgbImage, width: usize, height: usize, profile: PreprocessProfile) -> Result<Tensor<f32>, Box<dyn Error>> {
	let resized = image::imageops::resize(img, width as u32, height as u32, FilterType::Triangle);
	let (mean, std) = match profile {
		PreprocessProfile::Det => ([0.485_f32, 0.456, 0.406], [0.229_f32, 0.224, 0.225]),
		PreprocessProfile::RecRgb | PreprocessProfile::RecBgr => ([0.5_f32, 0.5, 0.5], [0.5_f32, 0.5, 0.5]),
	};
	let plane = width * height;
	let mut data = vec![0.0_f32; 3 * plane];
	for (x, y, pixel) in resized.enumerate_pixels() {
		let offset = y as usize * width + x as usize;
		for channel in 0..3 {
			let source_channel = match profile {
				PreprocessProfile::RecBgr => 2 - channel,
				PreprocessProfile::Det | PreprocessProfile::RecRgb => channel,
			};
			data[channel * plane + offset] = (f32::from(pixel[source_channel]) / 255.0 - mean[channel]) / std[channel];
		}
	}
	Ok(Tensor::<f32>::from_array(([1, 3, height, width], data))?)
}

fn load_rec_dict(path: &PathBuf) -> Result<Vec<String>, Box<dyn Error>> {
	let content = fs::read_to_string(path)?;
	let chars = content
		.lines()
		.map(str::trim_end)
		.filter(|line| !line.is_empty())
		.map(ToOwned::to_owned)
		.collect::<Vec<_>>();
	if chars.is_empty() {
		return Err(format!("empty recognition dictionary: {}", path.display()).into());
	}
	Ok(chars)
}

fn decode_rec_output(value: &ort::value::DynValue, charset: &[String], ctc_class_offset: usize) -> Result<(String, f32), Box<dyn Error>> {
	let (shape, data) = value.try_extract_tensor::<f32>()?;
	let dims = shape.as_ref();
	if dims.len() != 3 || dims[0] != 1 {
		return Err(format!("expected rec output shape [1,T,C], got {dims:?}").into());
	}
	let steps = dims[1] as usize;
	let classes = dims[2] as usize;
	let mut text = String::new();
	let mut previous = 0_usize;
	let mut score_sum = 0.0_f32;
	let mut score_count = 0_usize;
	for t in 0..steps {
		let row = &data[t * classes..(t + 1) * classes];
		let (class, score) = argmax(row);
		if class != 0 && class != previous {
			if let Some(index) = class.checked_sub(ctc_class_offset) {
				if let Some(ch) = charset.get(index) {
					text.push_str(ch);
				}
			}
			score_sum += score;
			score_count += 1;
		}
		previous = class;
	}
	let confidence = if score_count == 0 { 0.0 } else { score_sum / score_count as f32 };
	Ok((text, confidence))
}

fn decode_cls_output(value: &ort::value::DynValue) -> Result<(&'static str, f32), Box<dyn Error>> {
	let (shape, data) = value.try_extract_tensor::<f32>()?;
	let dims = shape.as_ref();
	if dims.len() != 2 || dims[0] != 1 || dims[1] != 2 {
		return Err(format!("expected cls output shape [1,2], got {dims:?}").into());
	}
	let (index, confidence) = argmax(data);
	let angle = if index == 0 { "0" } else { "180" };
	Ok((angle, confidence))
}

fn argmax(values: &[f32]) -> (usize, f32) {
	values
		.iter()
		.copied()
		.enumerate()
		.max_by(|(_, a), (_, b)| a.total_cmp(b))
		.unwrap_or((0, f32::NAN))
}

fn parse_args() -> Result<Args, String> {
	let mut raw = env::args().skip(1);
	let mut model = None;
	let mut det_model = None;
	let mut rec_model = None;
	let mut cls_model = None;
	let mut image = None;
	let mut dict = None;
	let mut pipeline = false;
	let mut run_dummy = false;
	let mut decode_det = false;
	let mut decode_rec = false;
	let mut decode_cls = false;
	let mut det_threshold = 0.3_f32;
	let mut det_min_area = 16_usize;
	let mut max_boxes = 32_usize;
	let mut merge_lines = false;
	let mut line_gap = 12_usize;
	let mut rec_bgr = false;
	let mut ctc_class_offset = 1_usize;
	let mut preprocess = None;
	let mut width = 320_usize;
	let mut height = 48_usize;
	while let Some(arg) = raw.next() {
		match arg.as_str() {
			"--model" => model = Some(PathBuf::from(raw.next().ok_or("missing value for --model")?)),
			"--det-model" => det_model = Some(PathBuf::from(raw.next().ok_or("missing value for --det-model")?)),
			"--rec-model" => rec_model = Some(PathBuf::from(raw.next().ok_or("missing value for --rec-model")?)),
			"--cls-model" => cls_model = Some(PathBuf::from(raw.next().ok_or("missing value for --cls-model")?)),
			"--image" => image = Some(PathBuf::from(raw.next().ok_or("missing value for --image")?)),
			"--dict" => dict = Some(PathBuf::from(raw.next().ok_or("missing value for --dict")?)),
			"--pipeline" => pipeline = true,
			"--run-dummy" => run_dummy = true,
			"--decode-det" => decode_det = true,
			"--decode-rec" => decode_rec = true,
			"--decode-cls" => decode_cls = true,
			"--det-threshold" => det_threshold = parse_next(&mut raw, "--det-threshold")?,
			"--det-min-area" => det_min_area = parse_next(&mut raw, "--det-min-area")?,
			"--max-boxes" => max_boxes = parse_next(&mut raw, "--max-boxes")?,
			"--merge-lines" => merge_lines = true,
			"--line-gap" => line_gap = parse_next(&mut raw, "--line-gap")?,
			"--preprocess" => {
				preprocess = Some(match raw.next().ok_or("missing value for --preprocess")?.as_str() {
					"det" => PreprocessProfile::Det,
					"rec" | "cls" | "rgb" => PreprocessProfile::RecRgb,
					"bgr" => PreprocessProfile::RecBgr,
					other => return Err(format!("unknown --preprocess value: {other}")),
				})
			}
			"--rec-bgr" => rec_bgr = true,
			"--ctc-class-offset" => ctc_class_offset = parse_next(&mut raw, "--ctc-class-offset")?,
			"--width" => width = parse_next(&mut raw, "--width")?,
			"--height" => height = parse_next(&mut raw, "--height")?,
			"-h" | "--help" => {
				print_help();
				std::process::exit(0);
			}
			other => return Err(format!("unknown argument: {other}")),
		}
	}
	Ok(Args {
		model,
		det_model,
		rec_model,
		cls_model,
		image,
		dict,
		pipeline,
		run_dummy,
		decode_det,
		decode_rec,
		decode_cls,
		det_threshold,
		det_min_area,
		max_boxes,
		merge_lines,
		line_gap,
		rec_bgr,
		ctc_class_offset,
		preprocess: preprocess.unwrap_or(if decode_det && !decode_rec && !decode_cls {
			PreprocessProfile::Det
		} else {
			PreprocessProfile::RecRgb
		}),
		width,
		height,
	})
}

fn parse_next<T: std::str::FromStr>(args: &mut impl Iterator<Item = String>, name: &str) -> Result<T, String> {
	args.next()
		.ok_or_else(|| format!("missing value for {name}"))?
		.parse()
		.map_err(|_| format!("invalid value for {name}"))
}

fn print_help() {
	println!(
		"vac-ocr-onnx-eval\n\n\
Usage:\n\
  vac-ocr-onnx-eval --model <path> [--run-dummy --width <px> --height <px>]\n\
  vac-ocr-onnx-eval --model <path> --image <path> --width <px> --height <px> [--decode-det] [--preprocess det]\n\
  vac-ocr-onnx-eval --model <path> --image <path> --width <px> --height <px> [--decode-rec --dict <path>]\n\
  vac-ocr-onnx-eval --model <path> --image <path> --width <px> --height <px> [--decode-cls]\n\
  vac-ocr-onnx-eval --pipeline --image <path> --det-model <path> --rec-model <path> --cls-model <path> --dict <path> [--rec-bgr] [--ctc-class-offset <n>] [--merge-lines --line-gap <px>]\n\n\
This binary is a development-only ONNX Runtime probe for evaluating PaddleOCR ONNX model loading through the `ort` crate."
	);
}
