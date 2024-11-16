// Copyright 2024 Irreducible Inc.

use super::lasso::lasso;

use crate::{
	builder::ConstraintSystemBuilder,
	helpers::{make_underliers, underliers_unpack_scalars_mut},
};
use anyhow::Result;
use binius_core::oracle::OracleId;
use binius_field::{
	as_packed_field::{PackScalar, PackedType},
	underlier::{UnderlierType, WithUnderlier},
	BinaryField, BinaryField16b, BinaryField32b, BinaryField8b, ExtensionField,
	PackedFieldIndexable, TowerField,
};
use bytemuck::{must_cast_slice, Pod};
use itertools::izip;

type B8 = BinaryField8b;
type B16 = BinaryField16b;
type B32 = BinaryField32b;

const T_LOG_SIZE: usize = 16;

pub fn u8mul<U, F, FBase>(
	builder: &mut ConstraintSystemBuilder<U, F, FBase>,
	name: impl ToString + Clone,
	mult_a: OracleId,
	mult_b: OracleId,
	log_size: usize,
) -> Result<OracleId, anyhow::Error>
where
	U: Pod
		+ UnderlierType
		+ PackScalar<B8>
		+ PackScalar<B16>
		+ PackScalar<B32>
		+ PackScalar<F>
		+ PackScalar<FBase>,
	PackedType<U, B8>: PackedFieldIndexable,
	PackedType<U, B16>: PackedFieldIndexable,
	PackedType<U, B32>: PackedFieldIndexable,
	F: TowerField
		+ BinaryField
		+ ExtensionField<B8>
		+ ExtensionField<B16>
		+ ExtensionField<B32>
		+ ExtensionField<FBase>,
	FBase: TowerField,
{
	builder.push_namespace(name.clone());

	let product = builder.add_committed("product", log_size, B16::TOWER_LEVEL);

	let lookup_t = builder.add_committed("lookup_t", T_LOG_SIZE, B32::TOWER_LEVEL);

	let lookup_u = builder.add_linear_combination(
		"lookup_u",
		log_size,
		[
			(mult_a, <F as TowerField>::basis(3, 3)?),
			(mult_b, <F as TowerField>::basis(3, 2)?),
			(product, <F as TowerField>::basis(3, 0)?),
		],
	)?;

	let channel = builder.add_channel();

	let mut u_to_t_mapping = None;

	if let Some(witness) = builder.witness() {
		let mut product_witness = make_underliers::<_, B16>(log_size);
		let mut lookup_u_witness = make_underliers::<_, B32>(log_size);
		let mut lookup_t_witness = make_underliers::<_, B32>(T_LOG_SIZE);
		let mut u_to_t_mapping_witness = vec![0; 1 << log_size];

		let mult_a_ext = witness.get::<B8>(mult_a)?;
		let mult_a_ints =
			must_cast_slice::<_, u8>(WithUnderlier::to_underliers_ref(mult_a_ext.evals()));

		let mult_b_ext = witness.get::<B8>(mult_b)?;
		let mult_b_ints =
			must_cast_slice::<_, u8>(WithUnderlier::to_underliers_ref(mult_b_ext.evals()));

		let product_scalars = underliers_unpack_scalars_mut::<_, B16>(&mut product_witness);
		let lookup_u_scalars = underliers_unpack_scalars_mut::<_, B32>(&mut lookup_u_witness);
		let lookup_t_scalars = underliers_unpack_scalars_mut::<_, B32>(&mut lookup_t_witness);

		for (a, b, lookup_u, product, u_to_t) in izip!(
			mult_a_ints,
			mult_b_ints,
			lookup_u_scalars.iter_mut(),
			product_scalars.iter_mut(),
			u_to_t_mapping_witness.iter_mut()
		) {
			let a_int = *a as usize;
			let b_int = *b as usize;
			let ab_product = a_int * b_int;
			let lookup_index = a_int << 8 | b_int;
			*lookup_u = B32::new((lookup_index << 16 | ab_product) as u32);

			*product = B16::new(ab_product as u16);
			*u_to_t = lookup_index;
		}

		for (i, lookup_t) in lookup_t_scalars.iter_mut().enumerate() {
			let a_int = (i >> 8) & 0xff;
			let b_int = i & 0xff;
			let ab_product = a_int * b_int;
			let lookup_index = a_int << 8 | b_int;
			assert_eq!(lookup_index, i);
			*lookup_t = B32::new((lookup_index << 16 | ab_product) as u32);
		}

		witness.set_owned::<B16, _>([(product, product_witness)])?;
		witness
			.set_owned::<B32, _>([(lookup_u, lookup_u_witness), (lookup_t, lookup_t_witness)])?;

		u_to_t_mapping = Some(u_to_t_mapping_witness);
	}

	lasso::<_, _, _, B32, B32, T_LOG_SIZE>(
		builder,
		format!("{} lasso", name.to_string()),
		log_size,
		u_to_t_mapping,
		lookup_u,
		lookup_t,
		channel,
	)?;

	builder.pop_namespace();
	Ok(product)
}
