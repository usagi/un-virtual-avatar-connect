//! Standard math and physics constants.
//!
//! These nodes intentionally expose Float ports with explicit SI-oriented names.
//! Quantity-aware variants can be added later without changing these stable names.

use crate::flowgraph::node::{ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;

pub struct ConstantsMathNode;
pub struct ConstantsPhysicsNode;

impl NodeDescriptor for ConstantsMathNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.constants.math".into(),
			title: "Math Constants".into(),
			category: "constants".into(),
			description: Some("Common dimensionless mathematical constants.".into()),
			inputs: vec![],
			outputs: vec![
				float_output("pi", "Pi"),
				float_output("tau", "Tau"),
				float_output("e", "Euler Number"),
				float_output("sqrt2", "Square Root Of 2"),
				float_output("ln2", "Natural Log Of 2"),
				float_output("golden_ratio", "Golden Ratio"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for ConstantsMathNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_properties: &InputMap,
		_inputs: &InputMap,
		_fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		Ok(NodeOutput::new()
			.set_data("pi", SocketValue::Float(std::f64::consts::PI))
			.set_data("tau", SocketValue::Float(std::f64::consts::TAU))
			.set_data("e", SocketValue::Float(std::f64::consts::E))
			.set_data("sqrt2", SocketValue::Float(std::f64::consts::SQRT_2))
			.set_data("ln2", SocketValue::Float(std::f64::consts::LN_2))
			.set_data("golden_ratio", SocketValue::Float((1.0 + 5.0_f64.sqrt()) / 2.0)))
	}
}

impl NodeDescriptor for ConstantsPhysicsNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.constants.physics".into(),
			title: "Physics Constants".into(),
			category: "constants".into(),
			description: Some("Common SI physical constants as Float values with unit-bearing port names.".into()),
			inputs: vec![],
			outputs: vec![
				float_output("c_m_s", "Speed Of Light (m/s)"),
				float_output("G_m3_kg_s2", "Gravitational Constant (m^3/kg/s^2)"),
				float_output("h_J_s", "Planck Constant (J*s)"),
				float_output("hbar_J_s", "Reduced Planck Constant (J*s)"),
				float_output("k_B_J_K", "Boltzmann Constant (J/K)"),
				float_output("e_C", "Elementary Charge (C)"),
				float_output("N_A_mol", "Avogadro Constant (1/mol)"),
				float_output("R_J_mol_K", "Molar Gas Constant (J/mol/K)"),
				float_output("epsilon0_F_m", "Vacuum Electric Permittivity (F/m)"),
				float_output("mu0_N_A2", "Vacuum Magnetic Permeability (N/A^2)"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for ConstantsPhysicsNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_properties: &InputMap,
		_inputs: &InputMap,
		_fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		Ok(NodeOutput::new()
			.set_data("c_m_s", SocketValue::Float(299_792_458.0))
			.set_data("G_m3_kg_s2", SocketValue::Float(6.67430e-11))
			.set_data("h_J_s", SocketValue::Float(6.626_070_15e-34))
			.set_data("hbar_J_s", SocketValue::Float(1.054_571_817e-34))
			.set_data("k_B_J_K", SocketValue::Float(1.380_649e-23))
			.set_data("e_C", SocketValue::Float(1.602_176_634e-19))
			.set_data("N_A_mol", SocketValue::Float(6.022_140_76e23))
			.set_data("R_J_mol_K", SocketValue::Float(8.314_462_618_153_24))
			.set_data("epsilon0_F_m", SocketValue::Float(8.854_187_8128e-12))
			.set_data("mu0_N_A2", SocketValue::Float(1.256_637_062_12e-6)))
	}
}

fn float_output(name: &str, title: &str) -> PortSpec {
	PortSpec::output(name, title, SocketType::Float)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::PureEvalHost;

	fn float(out: &NodeOutput, key: &str) -> f64 {
		match out.data.get(key) {
			Some(SocketValue::Float(value)) => *value,
			other => panic!("expected Float at {key}, got {other:?}"),
		}
	}

	#[tokio::test]
	async fn math_constants_emit_expected_values() {
		let out = ConstantsMathNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &InputMap::new(), &ExecFireSet::new())
			.await
			.unwrap();

		assert_eq!(float(&out, "pi"), std::f64::consts::PI);
		assert_eq!(float(&out, "tau"), std::f64::consts::TAU);
		assert_eq!(float(&out, "e"), std::f64::consts::E);
		assert_eq!(float(&out, "sqrt2"), std::f64::consts::SQRT_2);
		assert_eq!(float(&out, "ln2"), std::f64::consts::LN_2);
		assert!((float(&out, "golden_ratio") - 1.618_033_988_749_895).abs() < 1e-15);
	}

	#[tokio::test]
	async fn physics_constants_emit_si_values() {
		let out = ConstantsPhysicsNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &InputMap::new(), &ExecFireSet::new())
			.await
			.unwrap();

		assert_eq!(float(&out, "c_m_s"), 299_792_458.0);
		assert_eq!(float(&out, "h_J_s"), 6.626_070_15e-34);
		assert_eq!(float(&out, "k_B_J_K"), 1.380_649e-23);
		assert_eq!(float(&out, "e_C"), 1.602_176_634e-19);
		assert_eq!(float(&out, "N_A_mol"), 6.022_140_76e23);
		assert!((float(&out, "R_J_mol_K") - 8.314_462_618_153_24).abs() < 1e-15);
	}
}
