//! 256-bit intermediate math: reserves are in yoctoNEAR (1e24) and token units (1e18), their products overflow u128.

#![allow(clippy::manual_div_ceil, clippy::assign_op_pattern)]

uint::construct_uint! {
    pub struct U256(4);
}

/// floor(a * b / c), panics if the result does not fit in u128 or c == 0.
pub fn mul_div(a: u128, b: u128, c: u128) -> u128 {
    assert!(c > 0, "division by zero");
    let r = U256::from(a) * U256::from(b) / U256::from(c);
    assert!(r <= U256::from(u128::MAX), "overflow");
    r.as_u128()
}

pub fn bps(amount: u128, bps: u16) -> u128 {
    mul_div(amount, bps as u128, 10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mul_div_handles_products_above_u128() {
        let near = 30 * 10u128.pow(24);
        let supply = 1_000_000_000 * 10u128.pow(18);
        assert_eq!(mul_div(near, supply, supply), near);
        assert_eq!(bps(1_000, 1_000), 100);
    }
}
