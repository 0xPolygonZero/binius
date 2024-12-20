// Copyright 2024 Irreducible Inc.

use rayon::prelude::{IndexedParallelIterator, IntoParallelRefMutIterator, ParallelIterator};

use crate::{Error, ExtensionField, Field, PackedExtension, PackedField};

pub fn ext_base_mul<PE, F>(lhs: &mut [PE], rhs: &[PE::PackedSubfield]) -> Result<(), Error>
where
	PE: PackedExtension<F>,
	PE::Scalar: ExtensionField<F>,
	F: Field,
{
	ext_base_op(lhs, rhs, |lhs, broadcasted_rhs| PE::cast_ext(lhs.cast_base() * broadcasted_rhs))
}

pub fn ext_base_mul_par<PE, F>(lhs: &mut [PE], rhs: &[PE::PackedSubfield]) -> Result<(), Error>
where
	PE: PackedExtension<F>,
	PE::Scalar: ExtensionField<F>,
	F: Field,
{
	ext_base_op_par(lhs, rhs, |lhs, broadcasted_rhs| {
		PE::cast_ext(lhs.cast_base() * broadcasted_rhs)
	})
}

unsafe fn get_rhs_at_pe_idx<PE, F>(rhs: &[PE::PackedSubfield], i: usize) -> PE::PackedSubfield
where
	PE: PackedExtension<F>,
	PE::Scalar: ExtensionField<F>,
	F: Field,
{
	let bottom_most_scalar_idx = i * PE::WIDTH;
	let bottom_most_scalar_idx_in_subfield_arr = bottom_most_scalar_idx / PE::PackedSubfield::WIDTH;
	let bottom_most_scalar_idx_within_packed_subfield =
		bottom_most_scalar_idx % PE::PackedSubfield::WIDTH;
	let block_idx = bottom_most_scalar_idx_within_packed_subfield / PE::WIDTH;

	rhs[bottom_most_scalar_idx_in_subfield_arr].spread_unchecked(PE::LOG_WIDTH, block_idx)
}

/// Refer to the functions above for examples of closures to pass
/// Func takes in the following parameters
///
/// Note that this function overwrites the lhs buffer, copy that data before
/// invoking this function if you need to use it elsewhere
///
/// lhs: PE::WIDTH extension field scalars
///
/// broadcasted_rhs: a broadcasted version of PE::WIDTH subfield scalars
/// with each one occurring PE::PackedSubfield::WIDTH/PE::WIDTH times in  a row
/// such that the bits of the broadcasted scalars align with the lhs scalars
pub fn ext_base_op<PE, F, Func>(
	lhs: &mut [PE],
	rhs: &[PE::PackedSubfield],
	op: Func,
) -> Result<(), Error>
where
	PE: PackedExtension<F>,
	PE::Scalar: ExtensionField<F>,
	F: Field,
	Func: Fn(PE, PE::PackedSubfield) -> PE,
{
	if lhs.len() != rhs.len() * PE::Scalar::DEGREE {
		return Err(Error::MismatchedLengths);
	}

	lhs.iter_mut().enumerate().for_each(|(i, lhs_elem)| {
		// SAFETY: Width of PackedSubfield is always >= the width of the field implementing PackedExtension
		let broadcasted_rhs = unsafe { get_rhs_at_pe_idx::<PE, F>(rhs, i) };

		*lhs_elem = op(*lhs_elem, broadcasted_rhs);
	});
	Ok(())
}

/// A multithreaded version of the funcion directly above, use for long arrays
/// on the prover side
pub fn ext_base_op_par<PE, F, Func>(
	lhs: &mut [PE],
	rhs: &[PE::PackedSubfield],
	op: Func,
) -> Result<(), Error>
where
	PE: PackedExtension<F>,
	PE::Scalar: ExtensionField<F>,
	F: Field,
	Func: Fn(PE, PE::PackedSubfield) -> PE + std::marker::Sync,
{
	if lhs.len() != rhs.len() * PE::Scalar::DEGREE {
		return Err(Error::MismatchedLengths);
	}

	lhs.par_iter_mut().enumerate().for_each(|(i, lhs_elem)| {
		// SAFETY: Width of PackedSubfield is always >= the width of the field implementing PackedExtension
		let broadcasted_rhs = unsafe { get_rhs_at_pe_idx::<PE, F>(rhs, i) };

		*lhs_elem = op(*lhs_elem, broadcasted_rhs);
	});

	Ok(())
}

#[cfg(test)]
mod tests {
	use proptest::prelude::*;

	use crate::{
		ext_base_mul, ext_base_mul_par,
		packed::{get_packed_slice, set_packed_slice},
		underlier::WithUnderlier,
		BinaryField128b, BinaryField16b, BinaryField8b, PackedBinaryField16x16b,
		PackedBinaryField2x128b, PackedBinaryField32x8b, PackedField,
	};

	fn strategy_8b_scalars() -> impl Strategy<Value = [BinaryField8b; 32]> {
		any::<[<BinaryField8b as WithUnderlier>::Underlier; 32]>()
			.prop_map(|arr| arr.map(<BinaryField8b>::from_underlier))
	}

	fn strategy_16b_scalars() -> impl Strategy<Value = [BinaryField16b; 32]> {
		any::<[<BinaryField16b as WithUnderlier>::Underlier; 32]>()
			.prop_map(|arr| arr.map(<BinaryField16b>::from_underlier))
	}

	fn strategy_128b_scalars() -> impl Strategy<Value = [BinaryField128b; 32]> {
		any::<[<BinaryField128b as WithUnderlier>::Underlier; 32]>()
			.prop_map(|arr| arr.map(<BinaryField128b>::from_underlier))
	}

	fn pack_slice<P: PackedField>(scalar_slice: &[P::Scalar]) -> Vec<P> {
		let mut packed_slice = vec![P::default(); scalar_slice.len() / P::WIDTH];

		for (i, scalar) in scalar_slice.iter().enumerate() {
			set_packed_slice(&mut packed_slice, i, *scalar);
		}

		packed_slice
	}

	proptest! {
		#[test]
		fn test_base_ext_mul_8(base_scalars in strategy_8b_scalars(), ext_scalars in strategy_128b_scalars()){
			let base_packed = pack_slice::<PackedBinaryField32x8b>(&base_scalars);
			let mut ext_packed = pack_slice::<PackedBinaryField2x128b>(&ext_scalars);

			ext_base_mul(&mut ext_packed, &base_packed).unwrap();

			for (i, (base, ext)) in base_scalars.iter().zip(ext_scalars).enumerate(){
				assert_eq!(ext * *base, get_packed_slice(&ext_packed, i));
			}
		}

		#[test]
		fn test_base_ext_mul_16(base_scalars in strategy_16b_scalars(), ext_scalars in strategy_128b_scalars()){
			let base_packed = pack_slice::<PackedBinaryField16x16b>(&base_scalars);
			let mut ext_packed = pack_slice::<PackedBinaryField2x128b>(&ext_scalars);

			ext_base_mul(&mut ext_packed, &base_packed).unwrap();

			for (i, (base, ext)) in base_scalars.iter().zip(ext_scalars).enumerate(){
				assert_eq!(ext * *base, get_packed_slice(&ext_packed, i));
			}
		}


		#[test]
		fn test_base_ext_mul_par_8(base_scalars in strategy_8b_scalars(), ext_scalars in strategy_128b_scalars()){
			let base_packed = pack_slice::<PackedBinaryField32x8b>(&base_scalars);
			let mut ext_packed = pack_slice::<PackedBinaryField2x128b>(&ext_scalars);

			ext_base_mul_par(&mut ext_packed, &base_packed).unwrap();

			for (i, (base, ext)) in base_scalars.iter().zip(ext_scalars).enumerate(){
				assert_eq!(ext * *base, get_packed_slice(&ext_packed, i));
			}
		}

		#[test]
		fn test_base_ext_mul_par_16(base_scalars in strategy_16b_scalars(), ext_scalars in strategy_128b_scalars()){
			let base_packed = pack_slice::<PackedBinaryField16x16b>(&base_scalars);
			let mut ext_packed = pack_slice::<PackedBinaryField2x128b>(&ext_scalars);

			ext_base_mul_par(&mut ext_packed, &base_packed).unwrap();

			for (i, (base, ext)) in base_scalars.iter().zip(ext_scalars).enumerate(){
				assert_eq!(ext * *base, get_packed_slice(&ext_packed, i));
			}
		}
	}
}
