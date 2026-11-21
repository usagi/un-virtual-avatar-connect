//! Phase ο-5 introduced non-seeded `flowgraph.random.*` and seeded `flowgraph.noise.*`.
//! SL-1/SL-2 add deterministic `flowgraph.prng.*` and `flowgraph.dist.*` nodes with explicit seed / algorithm / offset.
//!
//! Perlin instances are cached per `seed` (as `u32`) in a process-global `Mutex<HashMap>`.

use crate::flowgraph::node::{
	get_optional_int, get_optional_string, get_required_float, get_required_int, get_required_list, ExecFireSet, InputMap, NodeDescriptor,
	NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;
use noise::{NoiseFn, Perlin};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static PERLIN_CACHE: LazyLock<Mutex<HashMap<u32, Perlin>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn perlin(seed: i64) -> Perlin {
	let u = seed.rem_euclid(u32::MAX as i64) as u32;
	let mut g = PERLIN_CACHE.lock().unwrap();
	*g.entry(u).or_insert_with(|| Perlin::new(u))
}

fn seed_u64(seed: &str) -> u64 {
	let hash = blake3::hash(seed.as_bytes());
	let mut bytes = [0u8; 8];
	bytes.copy_from_slice(&hash.as_bytes()[..8]);
	u64::from_le_bytes(bytes)
}

fn u64_to_unit_f64(v: u64) -> f64 {
	((v >> 11) as f64) / ((1u64 << 53) as f64)
}

fn bounded_i64(v: u64, lo: i64, hi: i64) -> i64 {
	let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
	let span = (hi as i128 - lo as i128 + 1) as u128;
	if span == 0 {
		return lo;
	}
	(lo as i128 + (u128::from(v) % span) as i128) as i64
}

enum Prng {
	SplitMix64 { state: u64 },
	Lcg64 { state: u64 },
	Pcg32 { state: u64, inc: u64 },
	Xoshiro256StarStar { s: [u64; 4] },
}

impl Prng {
	fn new(algorithm: &str, seed: &str) -> Result<Self, NodeExecError> {
		let seed = seed_u64(seed);
		match normalize_prng_algorithm(algorithm).as_str() {
			"splitmix64" => Ok(Self::SplitMix64 { state: seed }),
			"lcg64" => Ok(Self::Lcg64 { state: seed }),
			"pcg32" => Ok(Self::Pcg32 {
				state: seed,
				inc: 0xda3e_39cb_94b9_5bdb,
			}),
			"xoshiro256ss" => {
				let mut sm = Self::SplitMix64 { state: seed };
				Ok(Self::Xoshiro256StarStar {
					s: [sm.next_u64(), sm.next_u64(), sm.next_u64(), sm.next_u64()],
				})
			}
			other => Err(NodeExecError::Generic(anyhow::anyhow!(
				"unknown PRNG algorithm '{other}' (supported: splitmix64, lcg64, pcg32, xoshiro256ss)"
			))),
		}
	}

	fn skip(&mut self, n: usize) {
		for _ in 0..n {
			let _ = self.next_u64();
		}
	}

	fn next_u64(&mut self) -> u64 {
		match self {
			Self::SplitMix64 { state } => {
				*state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
				let mut z = *state;
				z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
				z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
				z ^ (z >> 31)
			}
			Self::Lcg64 { state } => {
				*state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
				*state
			}
			Self::Pcg32 { state, inc } => {
				fn next32(state: &mut u64, inc: u64) -> u32 {
					let old = *state;
					*state = old.wrapping_mul(6364136223846793005).wrapping_add(inc | 1);
					let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
					let rot = (old >> 59) as u32;
					xorshifted.rotate_right(rot)
				}
				let hi = next32(state, *inc) as u64;
				let lo = next32(state, *inc) as u64;
				(hi << 32) | lo
			}
			Self::Xoshiro256StarStar { s } => {
				let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
				let t = s[1] << 17;
				s[2] ^= s[0];
				s[3] ^= s[1];
				s[1] ^= s[2];
				s[0] ^= s[3];
				s[2] ^= t;
				s[3] = s[3].rotate_left(45);
				result
			}
		}
	}
}

fn normalize_prng_algorithm(value: &str) -> String {
	match value.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
		"" | "splitmix" | "splitmix64" => "splitmix64".into(),
		"lcg" | "lcg64" => "lcg64".into(),
		"pcg" | "pcg32" => "pcg32".into(),
		"xoshiro" | "xoshiro256" | "xoshiro256ss" | "xoshiro256starstar" => "xoshiro256ss".into(),
		other => other.into(),
	}
}

fn prng_from_inputs(inputs: &InputMap) -> Result<Prng, NodeExecError> {
	let seed = get_optional_string(inputs, "seed", "")?;
	let algorithm = get_optional_string(inputs, "algorithm", "splitmix64")?;
	let mut rng = Prng::new(&algorithm, &seed)?;
	let offset = get_optional_int(inputs, "offset", 0)?.max(0) as usize;
	rng.skip(offset);
	Ok(rng)
}

fn prng_unit_open01(rng: &mut Prng) -> f64 {
	u64_to_unit_f64(rng.next_u64()).clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON)
}

fn normal_sample(rng: &mut Prng, mean: f64, stddev: f64) -> f64 {
	let u1 = prng_unit_open01(rng);
	let u2 = u64_to_unit_f64(rng.next_u64());
	let r = (-2.0 * u1.ln()).sqrt();
	let theta = 2.0 * std::f64::consts::PI * u2;
	mean + stddev * r * theta.cos()
}

fn json_weight(value: &SocketValue) -> f64 {
	match value {
		SocketValue::Int(v) => (*v as f64).max(0.0),
		SocketValue::Float(v) => {
			if v.is_finite() && *v > 0.0 {
				*v
			} else {
				0.0
			}
		}
		SocketValue::Json(j) => match j {
			serde_json::Value::Number(n) => n.as_f64().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0),
			serde_json::Value::String(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0),
			serde_json::Value::Bool(true) => 1.0,
			_ => 0.0,
		},
		SocketValue::String(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).unwrap_or(0.0),
		SocketValue::Bool(true) => 1.0,
		_ => 0.0,
	}
}

// ---------------------------------------------------------------------
// prng.sequence
// ---------------------------------------------------------------------

pub struct PrngSequenceNode;

impl NodeDescriptor for PrngSequenceNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.prng.sequence".into(),
			title: "PRNG Sequence".into(),
			category: "random".into(),
			description: Some("Seeded deterministic PRNG sequence. algorithms: splitmix64, lcg64, pcg32, xoshiro256ss.".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("count", "Count", SocketType::Int).with_default(SocketValue::Int(1)),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
			],
			outputs: vec![
				PortSpec::output("hex", "U64 Hex", SocketType::List(Box::new(SocketType::String))),
				PortSpec::output("unit", "Unit Float", SocketType::List(Box::new(SocketType::Float))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for PrngSequenceNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let count = get_optional_int(inputs, "count", 1)?.clamp(0, 10_000) as usize;
		let mut hex = Vec::with_capacity(count);
		let mut unit = Vec::with_capacity(count);
		for _ in 0..count {
			let v = rng.next_u64();
			hex.push(SocketValue::String(format!("{v:016x}")));
			unit.push(SocketValue::Float(u64_to_unit_f64(v)));
		}
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(count as i64))
			.set_data("hex", SocketValue::List(hex))
			.set_data("unit", SocketValue::List(unit)))
	}
}

// ---------------------------------------------------------------------
// prng.uniform_int
// ---------------------------------------------------------------------

pub struct PrngUniformIntNode;

impl NodeDescriptor for PrngUniformIntNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.prng.uniform_int".into(),
			title: "PRNG uniform (int)".into(),
			category: "random".into(),
			description: Some("Seeded deterministic uniform integer in [lo, hi] inclusive.".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("lo", "Low", SocketType::Int),
				PortSpec::input("hi", "High", SocketType::Int),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Int)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for PrngUniformIntNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let lo = get_required_int(inputs, "lo")?;
		let hi = get_required_int(inputs, "hi")?;
		Ok(NodeOutput::new().set_data("value", SocketValue::Int(bounded_i64(rng.next_u64(), lo, hi))))
	}
}

// ---------------------------------------------------------------------
// prng.uniform_float
// ---------------------------------------------------------------------

pub struct PrngUniformFloatNode;

impl NodeDescriptor for PrngUniformFloatNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.prng.uniform_float".into(),
			title: "PRNG uniform (float)".into(),
			category: "random".into(),
			description: Some("Seeded deterministic uniform float in [lo, hi).".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("lo", "Low", SocketType::Float),
				PortSpec::input("hi", "High", SocketType::Float),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

// ---------------------------------------------------------------------
// dist.normal
// ---------------------------------------------------------------------

pub struct DistNormalNode;

impl NodeDescriptor for DistNormalNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dist.normal".into(),
			title: "Distribution normal".into(),
			category: "random".into(),
			description: Some("Seeded normal distribution sample using Box-Muller.".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("mean", "Mean", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("stddev", "Std dev", SocketType::Float).with_default(SocketValue::Float(1.0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for DistNormalNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let mean = inputs
			.get("mean")
			.map(|_| get_required_float(inputs, "mean"))
			.transpose()?
			.unwrap_or(0.0);
		let stddev = inputs
			.get("stddev")
			.map(|_| get_required_float(inputs, "stddev"))
			.transpose()?
			.unwrap_or(1.0);
		if !mean.is_finite() || !stddev.is_finite() || stddev < 0.0 {
			return Err(NodeExecError::Generic(anyhow::anyhow!(
				"dist.normal: mean/stddev must be finite and stddev >= 0"
			)));
		}
		let value = if stddev == 0.0 {
			mean
		} else {
			normal_sample(&mut rng, mean, stddev)
		};
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(value)))
	}
}

// ---------------------------------------------------------------------
// dist.exponential
// ---------------------------------------------------------------------

pub struct DistExponentialNode;

impl NodeDescriptor for DistExponentialNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dist.exponential".into(),
			title: "Distribution exponential".into(),
			category: "random".into(),
			description: Some("Seeded exponential distribution sample. lambda must be > 0.".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("lambda", "Lambda", SocketType::Float).with_default(SocketValue::Float(1.0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for DistExponentialNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let lambda = inputs
			.get("lambda")
			.map(|_| get_required_float(inputs, "lambda"))
			.transpose()?
			.unwrap_or(1.0);
		if !lambda.is_finite() || lambda <= 0.0 {
			return Err(NodeExecError::Generic(anyhow::anyhow!(
				"dist.exponential: lambda must be finite and > 0"
			)));
		}
		let value = -prng_unit_open01(&mut rng).ln() / lambda;
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(value)))
	}
}

// ---------------------------------------------------------------------
// dist.poisson
// ---------------------------------------------------------------------

pub struct DistPoissonNode;

impl NodeDescriptor for DistPoissonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dist.poisson".into(),
			title: "Distribution poisson".into(),
			category: "random".into(),
			description: Some("Seeded Poisson distribution sample. Knuth method, lambda <= 1000.".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("lambda", "Lambda", SocketType::Float).with_default(SocketValue::Float(1.0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Int)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for DistPoissonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let lambda = inputs
			.get("lambda")
			.map(|_| get_required_float(inputs, "lambda"))
			.transpose()?
			.unwrap_or(1.0);
		if !lambda.is_finite() || lambda < 0.0 || lambda > 1000.0 {
			return Err(NodeExecError::Generic(anyhow::anyhow!(
				"dist.poisson: lambda must be finite, >= 0, and <= 1000"
			)));
		}
		if lambda == 0.0 {
			return Ok(NodeOutput::new().set_data("value", SocketValue::Int(0)));
		}
		let threshold = (-lambda).exp();
		let mut k = 0i64;
		let mut p = 1.0;
		while p > threshold && k < 100_000 {
			k += 1;
			p *= prng_unit_open01(&mut rng);
		}
		Ok(NodeOutput::new().set_data("value", SocketValue::Int(k - 1)))
	}
}

// ---------------------------------------------------------------------
// dist.categorical
// ---------------------------------------------------------------------

pub struct DistCategoricalNode;

impl NodeDescriptor for DistCategoricalNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dist.categorical".into(),
			title: "Distribution categorical".into(),
			category: "random".into(),
			description: Some("Seeded categorical sample from non-negative weights. Optional values maps index to JSON value.".into()),
			inputs: vec![
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("algorithm", "Algorithm", SocketType::String).with_default(SocketValue::String("splitmix64".into())),
				PortSpec::input("offset", "Offset", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("weights", "Weights", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("values", "Values", SocketType::List(Box::new(SocketType::Json)))
					.with_default(SocketValue::List(Vec::new())),
			],
			outputs: vec![
				PortSpec::output("index", "Index", SocketType::Int),
				PortSpec::output("value", "Value", SocketType::Json),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for DistCategoricalNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let weights = get_required_list(inputs, "weights")?;
		let values = inputs.get("values").and_then(|v| v.as_list().ok()).unwrap_or(&[]);
		if weights.is_empty() {
			return Ok(NodeOutput::new()
				.set_data("index", SocketValue::Int(-1))
				.set_data("value", SocketValue::Json(serde_json::Value::Null)));
		}
		let numeric = weights.iter().map(json_weight).collect::<Vec<_>>();
		let total: f64 = numeric.iter().sum();
		let index = if total <= 0.0 {
			(rng.next_u64() as usize) % weights.len()
		} else {
			let mut needle = u64_to_unit_f64(rng.next_u64()) * total;
			let mut picked = weights.len().saturating_sub(1);
			for (idx, weight) in numeric.iter().copied().enumerate() {
				if weight <= 0.0 {
					continue;
				}
				if needle < weight {
					picked = idx;
					break;
				}
				needle -= weight;
			}
			picked
		};
		let value = values
			.get(index)
			.map(|value| match value {
				SocketValue::Json(j) => j.clone(),
				other => serde_json::json!(format!("{other:?}")),
			})
			.unwrap_or_else(|| serde_json::json!(index as i64));
		Ok(NodeOutput::new()
			.set_data("index", SocketValue::Int(index as i64))
			.set_data("value", SocketValue::Json(value)))
	}
}

#[async_trait]
impl PureNode for PrngUniformFloatNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut rng = prng_from_inputs(inputs)?;
		let mut lo = get_required_float(inputs, "lo")?;
		let mut hi = get_required_float(inputs, "hi")?;
		if !lo.is_finite() || !hi.is_finite() {
			return Err(NodeExecError::Generic(anyhow::anyhow!("prng.uniform_float: lo/hi must be finite")));
		}
		if lo > hi {
			std::mem::swap(&mut lo, &mut hi);
		}
		let value = if lo == hi {
			lo
		} else {
			lo + (hi - lo) * u64_to_unit_f64(rng.next_u64())
		};
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(value)))
	}
}

// ---------------------------------------------------------------------
// random.uniform_int
// ---------------------------------------------------------------------

pub struct RandomUniformIntNode;

impl NodeDescriptor for RandomUniformIntNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.random.uniform_int".into(),
			title: "Random uniform (int)".into(),
			category: "random".into(),
			description: Some("Uniform random integer in [lo, hi] inclusive.".into()),
			inputs: vec![
				PortSpec::input("lo", "Low", SocketType::Int),
				PortSpec::input("hi", "High", SocketType::Int),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Int)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for RandomUniformIntNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut lo = get_required_int(inputs, "lo")?;
		let mut hi = get_required_int(inputs, "hi")?;
		if lo > hi {
			std::mem::swap(&mut lo, &mut hi);
		}
		let v = rand::random_range(lo..=hi);
		Ok(NodeOutput::new().set_data("value", SocketValue::Int(v)))
	}
}

// ---------------------------------------------------------------------
// random.uniform_float
// ---------------------------------------------------------------------

pub struct RandomUniformFloatNode;

impl NodeDescriptor for RandomUniformFloatNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.random.uniform_float".into(),
			title: "Random uniform (float)".into(),
			category: "random".into(),
			description: Some("Uniform random float in [lo, hi) half-open interval.".into()),
			inputs: vec![
				PortSpec::input("lo", "Low", SocketType::Float),
				PortSpec::input("hi", "High", SocketType::Float),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for RandomUniformFloatNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut lo = get_required_float(inputs, "lo")?;
		let mut hi = get_required_float(inputs, "hi")?;
		if !lo.is_finite() || !hi.is_finite() {
			return Err(NodeExecError::Generic(anyhow::anyhow!(
				"random.uniform_float: lo/hi must be finite"
			)));
		}
		if lo > hi {
			std::mem::swap(&mut lo, &mut hi);
		}
		if lo == hi {
			return Ok(NodeOutput::new().set_data("value", SocketValue::Float(lo)));
		}
		let v: f64 = rand::random_range(lo..hi);
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(v)))
	}
}

// ---------------------------------------------------------------------
// random.normal
// ---------------------------------------------------------------------

pub struct RandomNormalNode;

impl NodeDescriptor for RandomNormalNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.random.normal".into(),
			title: "Random normal".into(),
			category: "random".into(),
			description: Some(
				"Gaussian sample (Box–Muller) with given mean and stddev. stddev must be non-negative; 0 yields mean.".into(),
			),
			inputs: vec![
				PortSpec::input("mean", "Mean", SocketType::Float),
				PortSpec::input("stddev", "Std dev", SocketType::Float),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

fn box_muller_pair() -> (f64, f64) {
	let u1 = loop {
		let u: f64 = rand::random_range(f64::MIN_POSITIVE..1.0);
		if u > 0.0 {
			break u;
		}
	};
	let u2: f64 = rand::random();
	let r = (-2.0 * u1.ln()).sqrt();
	let theta = 2.0 * std::f64::consts::PI * u2;
	(r * theta.cos(), r * theta.sin())
}

#[async_trait]
impl PureNode for RandomNormalNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mean = get_required_float(inputs, "mean")?;
		let stddev = get_required_float(inputs, "stddev")?;
		if !mean.is_finite() {
			return Err(NodeExecError::Generic(anyhow::anyhow!("random.normal: mean must be finite")));
		}
		if stddev < 0.0 || !stddev.is_finite() {
			return Err(NodeExecError::Generic(anyhow::anyhow!(
				"random.normal: stddev must be finite and >= 0"
			)));
		}
		if stddev == 0.0 {
			return Ok(NodeOutput::new().set_data("value", SocketValue::Float(mean)));
		}
		let (z0, _) = box_muller_pair();
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(mean + stddev * z0)))
	}
}

// ---------------------------------------------------------------------
// noise.perlin_1d
// ---------------------------------------------------------------------

pub struct NoisePerlin1dNode;

impl NodeDescriptor for NoisePerlin1dNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.noise.perlin_1d".into(),
			title: "Perlin 1D".into(),
			category: "noise".into(),
			description: Some("1D Perlin noise at coordinate `t` with integer `seed` (cached Perlin per seed).".into()),
			inputs: vec![
				PortSpec::input("t", "t", SocketType::Float),
				PortSpec::input("seed", "Seed", SocketType::Int).with_default(SocketValue::Int(0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for NoisePerlin1dNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let t = get_required_float(inputs, "t")?;
		let seed = get_required_int(inputs, "seed")?;
		if !t.is_finite() {
			return Err(NodeExecError::Generic(anyhow::anyhow!("noise.perlin_1d: t must be finite")));
		}
		let p = perlin(seed);
		let v = p.get([t]);
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(v)))
	}
}

// ---------------------------------------------------------------------
// noise.perlin_2d
// ---------------------------------------------------------------------

pub struct NoisePerlin2dNode;

impl NodeDescriptor for NoisePerlin2dNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.noise.perlin_2d".into(),
			title: "Perlin 2D".into(),
			category: "noise".into(),
			description: Some("2D Perlin noise at `(x, y)` with integer `seed`.".into()),
			inputs: vec![
				PortSpec::input("x", "x", SocketType::Float),
				PortSpec::input("y", "y", SocketType::Float),
				PortSpec::input("seed", "Seed", SocketType::Int).with_default(SocketValue::Int(0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for NoisePerlin2dNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let x = get_required_float(inputs, "x")?;
		let y = get_required_float(inputs, "y")?;
		let seed = get_required_int(inputs, "seed")?;
		if !x.is_finite() || !y.is_finite() {
			return Err(NodeExecError::Generic(anyhow::anyhow!("noise.perlin_2d: x/y must be finite")));
		}
		let p = perlin(seed);
		let v = p.get([x, y]);
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(v)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn uniform_int_range_inclusive() {
		let n = RandomUniformIntNode;
		let inputs: InputMap = [("lo".into(), SocketValue::Int(10)), ("hi".into(), SocketValue::Int(10))]
			.into_iter()
			.collect();
		let out = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("value").unwrap().as_i64().unwrap(), 10);
	}

	#[tokio::test]
	async fn prng_sequence_is_seeded_and_deterministic() {
		let n = PrngSequenceNode;
		let inputs: InputMap = [
			("seed".into(), SocketValue::String("doctor".into())),
			("algorithm".into(), SocketValue::String("xoshiro256ss".into())),
			("count".into(), SocketValue::Int(3)),
		]
		.into_iter()
		.collect();
		let a = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let b = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(a.data.get("hex"), b.data.get("hex"));
		assert_eq!(a.data.get("count"), Some(&SocketValue::Int(3)));
		assert_eq!(a.data.get("unit").unwrap().as_list().unwrap().len(), 3);
	}

	#[tokio::test]
	async fn prng_uniform_int_offset_advances_sequence() {
		let n = PrngUniformIntNode;
		let base: InputMap = [
			("seed".into(), SocketValue::String("fixed".into())),
			("algorithm".into(), SocketValue::String("pcg32".into())),
			("lo".into(), SocketValue::Int(10)),
			("hi".into(), SocketValue::Int(20)),
		]
		.into_iter()
		.collect();
		let a = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&base,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let mut offset = base.clone();
		offset.insert("offset".into(), SocketValue::Int(1));
		let b = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&offset,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let av = a.data.get("value").unwrap().as_i64().unwrap();
		let bv = b.data.get("value").unwrap().as_i64().unwrap();
		assert!((10..=20).contains(&av));
		assert!((10..=20).contains(&bv));
		let seq = PrngSequenceNode;
		let seq_inputs: InputMap = [
			("seed".into(), SocketValue::String("fixed".into())),
			("algorithm".into(), SocketValue::String("pcg32".into())),
			("count".into(), SocketValue::Int(2)),
		]
		.into_iter()
		.collect();
		let seq_out = seq
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&seq_inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let hex = seq_out.data.get("hex").unwrap().as_list().unwrap();
		assert_ne!(hex[0], hex[1]);
	}

	#[tokio::test]
	async fn prng_uniform_float_is_repeatable_and_bounded() {
		let n = PrngUniformFloatNode;
		let inputs: InputMap = [
			("seed".into(), SocketValue::String("float".into())),
			("algorithm".into(), SocketValue::String("lcg64".into())),
			("lo".into(), SocketValue::Float(-1.0)),
			("hi".into(), SocketValue::Float(1.0)),
		]
		.into_iter()
		.collect();
		let a = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let b = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(a.data.get("value"), b.data.get("value"));
		let v = a.data.get("value").unwrap().as_f64().unwrap();
		assert!((-1.0..1.0).contains(&v), "v={v}");
	}

	#[tokio::test]
	async fn dist_normal_is_repeatable_and_zero_stddev_is_mean() {
		let n = DistNormalNode;
		let inputs: InputMap = [
			("seed".into(), SocketValue::String("normal".into())),
			("mean".into(), SocketValue::Float(10.0)),
			("stddev".into(), SocketValue::Float(2.0)),
		]
		.into_iter()
		.collect();
		let a = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let b = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(a.data.get("value"), b.data.get("value"));

		let zero: InputMap = [
			("seed".into(), SocketValue::String("normal".into())),
			("mean".into(), SocketValue::Float(10.0)),
			("stddev".into(), SocketValue::Float(0.0)),
		]
		.into_iter()
		.collect();
		let out = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&zero,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("value"), Some(&SocketValue::Float(10.0)));
	}

	#[tokio::test]
	async fn dist_exponential_is_repeatable_and_non_negative() {
		let n = DistExponentialNode;
		let inputs: InputMap = [
			("seed".into(), SocketValue::String("exp".into())),
			("lambda".into(), SocketValue::Float(1.5)),
		]
		.into_iter()
		.collect();
		let a = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let b = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(a.data.get("value"), b.data.get("value"));
		let v = a.data.get("value").unwrap().as_f64().unwrap();
		assert!(v.is_finite() && v >= 0.0, "v={v}");
	}

	#[tokio::test]
	async fn dist_poisson_lambda_zero_is_zero() {
		let n = DistPoissonNode;
		let inputs: InputMap = [
			("seed".into(), SocketValue::String("poisson".into())),
			("lambda".into(), SocketValue::Float(0.0)),
		]
		.into_iter()
		.collect();
		let out = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("value"), Some(&SocketValue::Int(0)));
	}

	#[tokio::test]
	async fn dist_categorical_uses_positive_weights_and_values() {
		let n = DistCategoricalNode;
		let inputs: InputMap = [
			("seed".into(), SocketValue::String("cat".into())),
			(
				"weights".into(),
				SocketValue::List(vec![
					SocketValue::Json(serde_json::json!(0)),
					SocketValue::Json(serde_json::json!(10)),
				]),
			),
			(
				"values".into(),
				SocketValue::List(vec![
					SocketValue::Json(serde_json::json!("a")),
					SocketValue::Json(serde_json::json!("b")),
				]),
			),
		]
		.into_iter()
		.collect();
		let out = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("index"), Some(&SocketValue::Int(1)));
		assert_eq!(out.data.get("value"), Some(&SocketValue::Json(serde_json::json!("b"))));
	}

	#[tokio::test]
	async fn uniform_float_half_open() {
		let n = RandomUniformFloatNode;
		let inputs: InputMap = [("lo".into(), SocketValue::Float(0.0)), ("hi".into(), SocketValue::Float(1.0))]
			.into_iter()
			.collect();
		let out = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let v = out.data.get("value").unwrap().as_f64().unwrap();
		assert!((0.0..1.0).contains(&v), "v={v}");
	}

	#[tokio::test]
	async fn normal_zero_stddev_is_mean() {
		let n = RandomNormalNode;
		let inputs: InputMap = [("mean".into(), SocketValue::Float(3.5)), ("stddev".into(), SocketValue::Float(0.0))]
			.into_iter()
			.collect();
		let out = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert!((out.data.get("value").unwrap().as_f64().unwrap() - 3.5).abs() < 1e-12);
	}

	#[tokio::test]
	async fn perlin_1d_deterministic_for_seed() {
		let n = NoisePerlin1dNode;
		let inputs: InputMap = [("t".into(), SocketValue::Float(0.25)), ("seed".into(), SocketValue::Int(42))]
			.into_iter()
			.collect();
		let a = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let b = n
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(a.data.get("value"), b.data.get("value"));
		let v = a.data.get("value").unwrap().as_f64().unwrap();
		assert!(v.is_finite() && v.abs() <= 2.0, "perlin sample: {v}");
	}
}
