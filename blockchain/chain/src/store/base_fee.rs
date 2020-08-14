use blocks::Tipset;
use ipld_blockstore::BlockStore;
use message::Message;
use num_bigint::BigInt;
use std::convert::From;

const BlockMessageLimit: usize = 10000;
const BlockGasLimit: i64 = 10_000_000_000;
const BlockGasTarget: i64 = (BlockGasLimit / 2) as i64;
const BaseFeeMaxChangeDenom: i64 = 8; // 12.5%;
const InitialBaseFee: i64 = 100000000;
const MinimumBaseFee: i64 = 100;

fn compute_next_base_fee(base_fee: &BigInt, gas_limit_used: i64, no_of_blocks: usize) -> BigInt {
    let delta = gas_limit_used / no_of_blocks as i64 - BlockGasTarget;
    let mut change = base_fee * BigInt::from(delta);
    change /= BlockGasTarget;
    change /= BaseFeeMaxChangeDenom;
    let mut next_base_fee = base_fee + change;
    if next_base_fee < BigInt::from(MinimumBaseFee) {
        next_base_fee = BigInt::from(MinimumBaseFee);
    }
    next_base_fee
}

pub fn compute_base_fee<DB>(db: &DB, ts: &Tipset) -> Result<BigInt, crate::Error>
where
    DB: BlockStore,
{
    let mut total_limit = 0;
    for b in ts.blocks() {
        let (msg1, msg2) = crate::block_messages(db, &b)?;
        for m in msg1 {
            total_limit += m.gas_limit();
        }
        for m in msg2 {
            total_limit += m.gas_limit();
        }
    }
    let parent_base_fee = ts.blocks()[0].parent_base_fee();
    Ok(compute_next_base_fee(
        parent_base_fee,
        total_limit,
        ts.blocks().len(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn construct_tests() -> Vec<(i64, i64, usize, i64)> {
        // (base_fee, limit_used, no_of_blocks, output)
        let mut cases = Vec::new();
        cases.push((100_000_000, 0, 1, 87_500_000));
        cases.push((100_000_000, 0, 5, 87_500_000));
        cases.push((100_000_000, BlockGasTarget, 1, 100_000_000));
        cases.push((100_000_000, BlockGasTarget * 2, 2, 100_000_000));
        cases.push((100_000_000, BlockGasLimit * 2, 2, 112_500_000));
        cases.push((100_000_000, BlockGasLimit * 15 / 10, 2, 106_250_000));
        cases
    }

    #[test]
    fn run_base_fee_tests() {
        let cases = construct_tests();

        for case in cases {
            let output = compute_next_base_fee(&case.0.into(), case.1, case.2);
            assert_eq!(BigInt::from(case.3), output);
        }
    }
}
