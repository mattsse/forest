// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

mod common;

use actor::{
    multisig::{
        AddSignerParams, ChangeNumApprovalsThresholdParams, ConstructorParams, Method,
        ProposalHashData, ProposeParams, RemoveSignerParams, State, SwapSignerParams, Transaction,
        TxnID, TxnIDParams,
    },
    Set, ACCOUNT_ACTOR_CODE_ID, INIT_ACTOR_ADDR, INIT_ACTOR_CODE_ID, MULTISIG_ACTOR_CODE_ID,
    SYSTEM_ACTOR_ADDR,
};
use address::Address;
use clock::ChainEpoch;
use common::*;
use db::MemoryDB;
use encoding::blake2b_256;
use ipld_blockstore::BlockStore;
use ipld_hamt::{BytesKey, Hamt};
use message::UnsignedMessage;
use vm::{ActorError, ExitCode, Serialized, TokenAmount, METHOD_SEND};

const RECEIVER: u64 = 100;
const ANNE: u64 = 101;
const BOB: u64 = 102;
const CHARLIE: u64 = 103;

fn construct_and_verify<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    signers: Vec<Address>,
    num_approvals_threshold: i64,
    unlock_duration: ChainEpoch,
) {
    let params = ConstructorParams {
        signers: signers,
        num_approvals_threshold: num_approvals_threshold,
        unlock_duration: unlock_duration,
    };

    rt.expect_validate_caller_addr(&[*INIT_ACTOR_ADDR]);
    assert!(rt
        .call(
            &*MULTISIG_ACTOR_CODE_ID,
            Method::Constructor as u64,
            &Serialized::serialize(&params).unwrap()
        )
        .is_ok());
    rt.verify();
}

fn propose<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    to: Address,
    value: TokenAmount,
    method: u64,
    params: Serialized,
) -> Result<Serialized, ActorError> {
    let call_params = ProposeParams {
        to,
        value,
        method,
        params,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::Propose as u64,
        &Serialized::serialize(&call_params).unwrap(),
    )
}

fn approve<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    txn_id: i64,
    params: [u8; 32],
) -> Result<Serialized, ActorError> {
    let params = TxnIDParams {
        id: TxnID(txn_id),
        proposal_hash: params,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::Approve as u64,
        &Serialized::serialize(&params).unwrap(),
    )
}

fn cancel<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    txn_id: i64,
    params: [u8; 32],
) -> Result<Serialized, ActorError> {
    let params = TxnIDParams {
        id: TxnID(txn_id),
        proposal_hash: params,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::Cancel as u64,
        &Serialized::serialize(&params).unwrap(),
    )
}

fn add_signer<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    signer: Address,
    increase: bool,
) -> Result<Serialized, ActorError> {
    let params = AddSignerParams {
        signer: signer,
        increase: increase,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::AddSigner as u64,
        &Serialized::serialize(&params).unwrap(),
    )
}

fn remove_signer<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    signer: Address,
    decrease: bool,
) -> Result<Serialized, ActorError> {
    let params = RemoveSignerParams {
        signer: signer,
        decrease: decrease,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::RemoveSigner as u64,
        &Serialized::serialize(&params).unwrap(),
    )
}
fn swap_signers<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    old_signer: Address,
    new_signer: Address,
) -> Result<Serialized, ActorError> {
    let params = SwapSignerParams {
        from: old_signer,
        to: new_signer,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::SwapSigner as u64,
        &Serialized::serialize(&params).unwrap(),
    )
}
fn change_num_approvals_threshold<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    new_threshold: i64,
) -> Result<Serialized, ActorError> {
    let params = ChangeNumApprovalsThresholdParams {
        new_threshold: new_threshold,
    };
    rt.call(
        &*MULTISIG_ACTOR_CODE_ID,
        Method::ChangeNumApprovalsThreshold as u64,
        &Serialized::serialize(&params).unwrap(),
    )
}

fn make_proposal_hash(
    approved: Vec<Address>,
    to: Address,
    value: TokenAmount,
    method: u64,
    params: &[u8],
) -> [u8; 32] {
    let hash_data = ProposalHashData {
        requester: approved[0],
        to,
        value,
        method,
        params: params.to_vec(),
    };
    let serial_data = Serialized::serialize(hash_data).unwrap();
    blake2b_256(serial_data.bytes())
}

fn assert_transactions<'a, BS: BlockStore>(
    rt: &mut MockRuntime<'a, BS>,
    expected: Vec<Transaction>,
) {
    let state: State = rt.get_state().unwrap();
    let map: Hamt<BytesKey, _> = Hamt::load(&state.pending_txs, rt.store).unwrap();

    let mut count = 0;
    assert!(map
        .for_each(|_, value: Transaction| {
            assert_eq!(value, expected[count]);
            count += 1;
            Ok(())
        })
        .is_ok());

    assert_eq!(count, expected.len());
}

mod construction_tests {

    use super::*;
    fn construct_runtime<'a, BS: BlockStore>(bs: &'a BS) -> MockRuntime<'a, BS> {
        let receiver = Address::new_id(RECEIVER);
        let message = UnsignedMessage::builder()
            .to(receiver.clone())
            .from(SYSTEM_ACTOR_ADDR.clone())
            .build()
            .unwrap();
        let mut rt = MockRuntime::new(bs, message);
        rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());
        rt.expect_validate_caller_addr(&[*INIT_ACTOR_ADDR]);

        return rt;
    }

    fn check_construct_state<'a, BS: BlockStore>(
        rt: &mut MockRuntime<BS>,
        params: ConstructorParams,
    ) -> State {
        assert!(rt
            .call(
                &*MULTISIG_ACTOR_CODE_ID,
                Method::Constructor as u64,
                &Serialized::serialize(&params).unwrap()
            )
            .is_ok());
        rt.verify();
        let state: State = rt.get_state().unwrap();
        assert_eq!(params.signers, state.signers);
        assert_eq!(params.signers, state.signers);
        assert_eq!(
            params.num_approvals_threshold,
            state.num_approvals_threshold
        );
        state
    }

    #[test]
    fn simple() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs);
        let params = ConstructorParams {
            signers: vec![
                Address::new_id(ANNE),
                Address::new_id(BOB),
                Address::new_id(CHARLIE),
            ],
            num_approvals_threshold: 2,
            unlock_duration: 0,
        };
        let state = check_construct_state(&mut rt, params);

        assert_eq!(TokenAmount::from(0u8), state.initial_balance);
        assert_eq!(0, state.unlock_duration);
        assert_eq!(0, state.start_epoch);
        let txns = Set::from_root(rt.store, &state.pending_txs)
            .unwrap()
            .collect_keys()
            .unwrap();
        assert_eq!(txns.len(), 0);
    }

    #[test]
    fn vesting() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs);
        rt.epoch = 1234;
        let params = ConstructorParams {
            signers: vec![
                Address::new_id(ANNE),
                Address::new_id(BOB),
                Address::new_id(CHARLIE),
            ],
            num_approvals_threshold: 3,
            unlock_duration: 100,
        };

        let state = check_construct_state(&mut rt, params);

        assert_eq!(TokenAmount::from(0u8), state.initial_balance);
        assert_eq!(100, state.unlock_duration);
        assert_eq!(1234, state.start_epoch);
    }
    #[test]
    fn zero_signers() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs);
        rt.epoch = 1234;
        let params = ConstructorParams {
            signers: vec![],
            num_approvals_threshold: 1,
            unlock_duration: 1,
        };
        assert_eq!(
            ExitCode::ErrIllegalArgument,
            rt.call(
                &*MULTISIG_ACTOR_CODE_ID,
                Method::Constructor as u64,
                &Serialized::serialize(&params).unwrap(),
            )
            .unwrap_err()
            .exit_code()
        );
        rt.verify();
    }
}

mod test_vesting {
    use super::*;
    const UNLOCK_DURATION: i64 = 10;
    const INITIAL_BALANCE: u64 = 100;
    const DARLENE: u64 = 103;

    fn construct_runtime<'a, BS: BlockStore>(
        bs: &'a BS,
        num_approvals: i64,
    ) -> MockRuntime<'a, BS> {
        let receiver = Address::new_id(RECEIVER);
        let initial_balance = TokenAmount::from(INITIAL_BALANCE);
        let message = UnsignedMessage::builder()
            .to(receiver.clone())
            .value(initial_balance.clone())
            .from(SYSTEM_ACTOR_ADDR.clone())
            .build()
            .unwrap();
        let mut rt = MockRuntime::new(bs, message);
        rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());
        rt.balance = initial_balance.clone();
        rt.received = initial_balance;
        construct_and_verify(
            &mut rt,
            vec![
                Address::new_id(ANNE),
                Address::new_id(BOB),
                Address::new_id(CHARLIE),
            ],
            num_approvals,
            UNLOCK_DURATION,
        );
        let anne = Address::new_id(ANNE);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), anne.clone());
        rt.received = TokenAmount::from(0u8);
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        return rt;
    }

    fn darlene_propose<'a, BS: BlockStore>(
        rt: &mut MockRuntime<BS>,
        init_bal: u64,
    ) -> Result<Serialized, ActorError> {
        let darlene = Address::new_id(DARLENE);
        let initial_balance = TokenAmount::from(init_bal);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let v = propose(
            rt,
            darlene,
            initial_balance.clone(),
            METHOD_SEND,
            fake_params.clone(),
        );
        rt.verify();
        v
    }

    fn approve_call<'a, BS: BlockStore>(
        rt: &mut MockRuntime<BS>,
        init_bal: u64,
        exp_send: bool,
    ) -> Result<Serialized, ActorError> {
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(BOB));
        let darlene = Address::new_id(DARLENE);
        let initial_balance = TokenAmount::from(init_bal);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();

        if exp_send {
            rt.expect_send(
                darlene.clone(),
                METHOD_SEND,
                fake_params.clone(),
                initial_balance.clone(),
                Serialized::default(),
                ExitCode::Ok,
            );
        }
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        let proposal_hash_data = make_proposal_hash(
            vec![Address::new_id(ANNE)],
            darlene,
            initial_balance,
            METHOD_SEND,
            fake_params.bytes(),
        );
        let v = approve(rt, 0, proposal_hash_data);
        rt.verify();
        v
    }

    #[test]
    fn happy_path() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 2);

        assert!(darlene_propose(&mut rt, INITIAL_BALANCE).is_ok());

        rt.epoch = UNLOCK_DURATION;

        assert!(approve_call(&mut rt, INITIAL_BALANCE, true).is_ok());
    }

    #[test]
    fn partial_vesting() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 2);
        assert!(darlene_propose(&mut rt, INITIAL_BALANCE / 2).is_ok());

        rt.epoch = UNLOCK_DURATION / 2;

        assert!(approve_call(&mut rt, INITIAL_BALANCE / 2, true).is_ok());
    }

    #[test]
    fn auto_approve_above_locked_fail() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 1);

        let anne = Address::new_id(ANNE);

        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let darlene = Address::new_id(DARLENE);
        assert_eq!(
            ExitCode::ErrInsufficientFunds,
            darlene_propose(&mut rt, INITIAL_BALANCE)
                .unwrap_err()
                .exit_code()
        );

        rt.epoch = 1;
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), anne.clone());
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        rt.expect_send(
            darlene.clone(),
            METHOD_SEND,
            fake_params.clone(),
            TokenAmount::from(10u8),
            Serialized::default(),
            ExitCode::Ok,
        );

        assert!(darlene_propose(&mut rt, 10).is_ok());
    }

    #[test]
    fn more_than_locked() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 2);
        rt.received = TokenAmount::from(0u8);
        assert!(darlene_propose(&mut rt, INITIAL_BALANCE / 2).is_ok());

        rt.epoch = 1;

        assert_eq!(
            ExitCode::ErrInsufficientFunds,
            approve_call(&mut rt, INITIAL_BALANCE / 2, false)
                .unwrap_err()
                .exit_code()
        );
    }
}

mod test_propose {
    use super::*;
    const SEND_VALUE: u64 = 10;
    const NO_LOCK_DUR: i64 = 0;
    const CHUCK: u64 = 103;
    fn construct_runtime<'a, BS: BlockStore>(
        bs: &'a BS,
        num_approvals: i64,
    ) -> MockRuntime<'a, BS> {
        let receiver = Address::new_id(RECEIVER);
        let message = UnsignedMessage::builder()
            .to(receiver.clone())
            .from(SYSTEM_ACTOR_ADDR.clone())
            .build()
            .unwrap();
        let mut rt = MockRuntime::new(bs, message);
        rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());
        let signers = vec![Address::new_id(ANNE), Address::new_id(BOB)];
        construct_and_verify(&mut rt, signers, num_approvals, NO_LOCK_DUR);
        return rt;
    }

    fn propose_call<BS: BlockStore>(
        rt: &mut MockRuntime<BS>,
        caller_id: u64,
        expected: Vec<Transaction>,
    ) -> Result<Serialized, ActorError> {
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(caller_id));
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let v = propose(
            rt,
            Address::new_id(CHUCK),
            TokenAmount::from(SEND_VALUE),
            METHOD_SEND,
            fake_params.clone(),
        );
        assert_transactions(rt, expected);
        rt.verify();
        v
    }

    #[test]
    fn simple() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 2);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        assert!(propose_call(
            &mut rt,
            ANNE,
            vec![Transaction {
                to: Address::new_id(CHUCK),
                value: TokenAmount::from(SEND_VALUE),
                method: METHOD_SEND,
                params: fake_params,
                approved: vec![Address::new_id(ANNE)],
            }]
        )
        .is_ok());
    }

    #[test]
    fn with_threshold_met() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 1);
        rt.balance = TokenAmount::from(20u8);
        rt.received = TokenAmount::from(0u8);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        rt.expect_send(
            Address::new_id(CHUCK),
            METHOD_SEND,
            fake_params.clone(),
            TokenAmount::from(SEND_VALUE),
            Serialized::default(),
            ExitCode::Ok,
        );
        assert!(propose_call(&mut rt, ANNE, vec![]).is_ok());
    }

    #[test]
    fn fail_insufficent_balance() {
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 1);
        rt.balance = TokenAmount::from(0u8);
        rt.received = TokenAmount::from(0u8);
        assert_eq!(
            ExitCode::ErrInsufficientFunds,
            propose_call(&mut rt, ANNE, vec![]).unwrap_err().exit_code()
        );
    }

    #[test]
    fn fail_non_signer() {
        let richard = 105;
        let bs = MemoryDB::default();
        let mut rt = construct_runtime(&bs, 2);
        assert_eq!(
            ExitCode::ErrForbidden,
            propose_call(&mut rt, richard, vec![])
                .unwrap_err()
                .exit_code()
        );
    }
}

mod test_approve {
    use super::*;
    const CHUCK: u64 = 103;
    const NO_UNLOCK_DURATION: i64 = 10;
    const NUM_APPROVALS: i64 = 2;
    const TXN_ID: i64 = 0;
    const FAKE_METHOD: u64 = 42;
    const SEND_VALUE: u64 = 10;

    fn construct_and_propose<'a, BS: BlockStore>(
        bs: &'a BS,
        method_num: u64,
    ) -> MockRuntime<'a, BS> {
        let receiver = Address::new_id(RECEIVER);
        let message = UnsignedMessage::builder()
            .to(receiver.clone())
            .from(SYSTEM_ACTOR_ADDR.clone())
            .build()
            .unwrap();
        let mut rt = MockRuntime::new(bs, message);
        rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());
        let signers = vec![Address::new_id(ANNE), Address::new_id(BOB)];
        construct_and_verify(&mut rt, signers, NUM_APPROVALS, NO_UNLOCK_DURATION);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(ANNE));
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        assert!(propose(
            &mut rt,
            Address::new_id(CHUCK),
            TokenAmount::from(SEND_VALUE),
            method_num,
            fake_params.clone()
        )
        .is_ok());
        rt.verify();
        rt
    }

    fn approve_call<BS: BlockStore>(
        rt: &mut MockRuntime<BS>,
        approved: u64,
        receiver: u64,
        method_num: u64,
        txn_id: i64,
    ) -> Result<Serialized, ActorError> {
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let proposal_hash_data = make_proposal_hash(
            vec![Address::new_id(approved)],
            Address::new_id(receiver),
            TokenAmount::from(SEND_VALUE),
            method_num,
            fake_params.bytes(),
        );
        approve(rt, txn_id, proposal_hash_data)
    }

    fn chuck_assert_transaction<BS: BlockStore>(rt: &mut MockRuntime<BS>, method_num: u64) {
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        assert_transactions(
            rt,
            vec![Transaction {
                to: Address::new_id(CHUCK),
                value: TokenAmount::from(SEND_VALUE),
                method: method_num,
                params: fake_params.clone(),
                approved: vec![Address::new_id(ANNE)],
            }],
        );
    }

    #[test]
    fn simple() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, FAKE_METHOD);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let chuck = Address::new_id(CHUCK);
        chuck_assert_transaction(&mut rt, FAKE_METHOD);
        rt.balance = TokenAmount::from(SEND_VALUE);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(BOB));
        rt.expect_send(
            chuck.clone(),
            FAKE_METHOD,
            fake_params.clone(),
            TokenAmount::from(SEND_VALUE),
            Serialized::default(),
            ExitCode::Ok,
        );
        assert!(approve_call(&mut rt, ANNE, CHUCK, FAKE_METHOD, 0).is_ok());
        rt.verify();
        assert_transactions(&mut rt, vec![]);
    }

    #[test]
    fn fail_with_bad_proposal() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, METHOD_SEND);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let chuck = Address::new_id(CHUCK);
        chuck_assert_transaction(&mut rt, METHOD_SEND);
        rt.balance = TokenAmount::from(SEND_VALUE);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(BOB));
        rt.expect_send(
            chuck.clone(),
            FAKE_METHOD,
            fake_params.clone(),
            TokenAmount::from(SEND_VALUE),
            Serialized::default(),
            ExitCode::Ok,
        );
        assert_eq!(
            ExitCode::ErrIllegalState,
            approve_call(&mut rt, ANNE, CHUCK, FAKE_METHOD, TXN_ID)
                .unwrap_err()
                .exit_code()
        );
    }

    #[test]
    fn fail_transaction_more_than_once() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, METHOD_SEND);
        assert_eq!(
            ExitCode::ErrIllegalState,
            approve_call(&mut rt, ANNE, CHUCK, METHOD_SEND, TXN_ID)
                .unwrap_err()
                .exit_code()
        );
    }

    #[test]
    fn approve_transaction_that_doesnt_exist() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, METHOD_SEND);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(BOB));
        assert_eq!(
            ExitCode::ErrNotFound,
            approve_call(&mut rt, BOB, CHUCK, METHOD_SEND, 1)
                .unwrap_err()
                .exit_code()
        );
        rt.verify();
        chuck_assert_transaction(&mut rt, METHOD_SEND);
    }

    #[test]
    fn fail_non_signer() {
        let richard = 105;
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, METHOD_SEND);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(richard));
        assert_eq!(
            ExitCode::ErrForbidden,
            approve_call(&mut rt, richard, CHUCK, METHOD_SEND, TXN_ID)
                .unwrap_err()
                .exit_code()
        );
        rt.verify();
        chuck_assert_transaction(&mut rt, METHOD_SEND);
    }
}

mod test_cancel {
    use super::*;
    const CHUCK: u64 = 103;
    const RICHARD: u64 = 104;
    const NO_UNLOCK_DURATION: i64 = 0;
    const NUM_APPROVALS: i64 = 2;
    const TXN_ID: i64 = 0;
    const FAKE_METHOD: u64 = 42;
    const SEND_VALUE: u64 = 10;

    fn construct_and_propose<'a, BS: BlockStore>(
        bs: &'a BS,
        method_num: u64,
    ) -> MockRuntime<'a, BS> {
        let receiver = Address::new_id(RECEIVER);
        let message = UnsignedMessage::builder()
            .to(receiver.clone())
            .from(SYSTEM_ACTOR_ADDR.clone())
            .build()
            .unwrap();
        let mut rt = MockRuntime::new(bs, message);
        rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());
        let signers = vec![Address::new_id(ANNE), Address::new_id(BOB)];
        construct_and_verify(&mut rt, signers, NUM_APPROVALS, NO_UNLOCK_DURATION);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(ANNE));
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        assert!(propose(
            &mut rt,
            Address::new_id(CHUCK),
            TokenAmount::from(SEND_VALUE),
            method_num,
            fake_params.clone()
        )
        .is_ok());
        rt.verify();
        rt.expect_validate_caller_type(&[
            ACCOUNT_ACTOR_CODE_ID.clone(),
            MULTISIG_ACTOR_CODE_ID.clone(),
        ]);
        return rt;
    }

    fn cancel_and_assert<BS: BlockStore>(
        rt: &mut MockRuntime<BS>,
        txn_id: i64,
    ) -> Result<Serialized, ActorError> {
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();
        let proposal_hash_data = make_proposal_hash(
            vec![Address::new_id(ANNE)],
            Address::new_id(CHUCK),
            TokenAmount::from(SEND_VALUE),
            FAKE_METHOD,
            fake_params.bytes(),
        );
        let v = cancel(rt, txn_id, proposal_hash_data);
        rt.verify();
        assert_transactions(
            rt,
            vec![Transaction {
                to: Address::new_id(CHUCK),
                value: TokenAmount::from(SEND_VALUE),
                method: FAKE_METHOD,
                params: fake_params,
                approved: vec![Address::new_id(ANNE)],
            }],
        );
        v
    }

    #[test]
    fn simple() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, METHOD_SEND);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();

        rt.balance = TokenAmount::from(SEND_VALUE);

        let proposal_hash_data = make_proposal_hash(
            vec![Address::new_id(ANNE)],
            Address::new_id(CHUCK),
            TokenAmount::from(SEND_VALUE),
            METHOD_SEND,
            fake_params.bytes(),
        );

        assert!(cancel(&mut rt, TXN_ID, proposal_hash_data).is_ok());
        rt.verify();
        assert_transactions(&mut rt, vec![]);
    }

    #[test]
    fn cancel_with_bad_proposal() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, FAKE_METHOD);
        let fake_params = Serialized::serialize([1, 2, 3, 4]).unwrap();

        rt.balance = TokenAmount::from(SEND_VALUE);

        let proposal_hash_data = make_proposal_hash(
            vec![Address::new_id(CHUCK)],
            Address::new_id(BOB),
            TokenAmount::from(SEND_VALUE),
            FAKE_METHOD,
            fake_params.bytes(),
        );
        assert_eq!(
            ExitCode::ErrIllegalState,
            cancel(&mut rt, TXN_ID, proposal_hash_data)
                .unwrap_err()
                .exit_code()
        );
    }

    #[test]
    fn fail_to_cancel_transaction() {
        let bs = MemoryDB::default();

        let mut rt = construct_and_propose(&bs, FAKE_METHOD);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), Address::new_id(BOB));

        assert_eq!(
            ExitCode::ErrForbidden,
            cancel_and_assert(&mut rt, TXN_ID).unwrap_err().exit_code()
        );
    }

    #[test]
    fn fail_when_not_signer() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, FAKE_METHOD);

        let richard = Address::new_id(RICHARD);
        rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), richard.clone());

        assert_eq!(
            ExitCode::ErrForbidden,
            cancel_and_assert(&mut rt, TXN_ID).unwrap_err().exit_code()
        );
    }

    #[test]
    fn cancel_transition_doesnt_exist() {
        let bs = MemoryDB::default();
        let mut rt = construct_and_propose(&bs, FAKE_METHOD);

        assert_eq!(
            ExitCode::ErrNotFound,
            cancel_and_assert(&mut rt, 1).unwrap_err().exit_code()
        );
    }
}

mod test_add_signer {
    use super::*;
    struct SignerTestCase {
        desc: String,
        initial_signers: Vec<Address>,
        initial_approvals: i64,
        add_signer: Address,
        increase: bool,
        expect_signers: Vec<Address>,
        expect_approvals: i64,
        code: ExitCode,
    }
    const CHUCK: u64 = 103;
    const MULTISIG_WALLET_ADD: u64 = 100;
    const NO_LOCK_DURATION: i64 = 0;

    #[test]
    fn test() {
        let test_cases = vec![
            SignerTestCase {
                desc: "happy path add signer".to_string(),
                initial_signers: vec![Address::new_id(ANNE), Address::new_id(BOB)],
                initial_approvals: 2,
                add_signer: Address::new_id(CHUCK),
                increase: false,
                expect_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                expect_approvals: 2,
                code: ExitCode::Ok,
            },
            SignerTestCase {
                desc: "add signer and increase threshold".to_string(),
                initial_signers: vec![Address::new_id(ANNE), Address::new_id(BOB)],
                initial_approvals: 2,
                add_signer: Address::new_id(CHUCK),
                increase: true,
                expect_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                expect_approvals: 3,
                code: ExitCode::Ok,
            },
            SignerTestCase {
                desc: "fail to add signer than already exists".to_string(),
                initial_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                initial_approvals: 3,
                add_signer: Address::new_id(CHUCK),
                increase: false,
                expect_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                expect_approvals: 3,
                code: ExitCode::ErrIllegalArgument,
            },
        ];

        for test_case in test_cases {
            println!("Test case executing is {}", test_case.desc);
            let receiver = Address::new_id(MULTISIG_WALLET_ADD);
            let message = UnsignedMessage::builder()
                .to(receiver.clone())
                .from(SYSTEM_ACTOR_ADDR.clone())
                .build()
                .unwrap();
            let bs = MemoryDB::default();
            let mut rt = MockRuntime::new(&bs, message);
            rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());

            construct_and_verify(
                &mut rt,
                test_case.initial_signers,
                test_case.initial_approvals,
                NO_LOCK_DURATION,
            );
            rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), receiver.clone());
            rt.expect_validate_caller_addr(&[receiver.clone()]);
            if test_case.code == ExitCode::Ok {
                assert!(add_signer(&mut rt, test_case.add_signer, test_case.increase).is_ok());
                let state: State = rt.get_state().unwrap();
                assert_eq!(test_case.expect_signers, state.signers);
                assert_eq!(test_case.expect_approvals, state.num_approvals_threshold);
            } else {
                assert_eq!(
                    test_case.code,
                    add_signer(&mut rt, test_case.add_signer, test_case.increase)
                        .unwrap_err()
                        .exit_code()
                );
            }
            rt.verify();
        }
    }
}

mod test_remove_signer {
    use super::*;
    struct SignerTestCase {
        desc: String,
        initial_signers: Vec<Address>,
        initial_approvals: i64,
        remove_signer: Address,
        decrease: bool,
        expect_signers: Vec<Address>,
        expect_approvals: i64,
        code: ExitCode,
    }
    const CHUCK: u64 = 103;
    const RICHARD: u64 = 104;
    const MULTISIG_WALLET_ADD: u64 = 100;
    const NO_LOCK_DURATION: i64 = 0;

    #[test]
    fn test() {
        let test_cases = vec![
            SignerTestCase {
                desc: "happy path remove signer".to_string(),
                initial_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                initial_approvals: 2,
                remove_signer: Address::new_id(CHUCK),
                decrease: false,
                expect_signers: vec![Address::new_id(ANNE), Address::new_id(BOB)],
                expect_approvals: 2,
                code: ExitCode::Ok,
            },
            SignerTestCase {
                desc: "Remove signer and decrease threshold".to_string(),
                initial_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                initial_approvals: 2,
                remove_signer: Address::new_id(CHUCK),
                decrease: true,
                expect_signers: vec![Address::new_id(ANNE), Address::new_id(BOB)],
                expect_approvals: 1,
                code: ExitCode::Ok,
            },
            SignerTestCase {
                desc: "fail remove signer if decrease set to false and number of signers below threshold".to_string(),
                initial_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                initial_approvals: 3,
                remove_signer: Address::new_id(CHUCK),
                decrease: false,
                expect_signers: vec![Address::new_id(ANNE), Address::new_id(BOB)],
                expect_approvals: 2,
                code: ExitCode::ErrIllegalArgument,
            },
            SignerTestCase {
                desc: "Remove signer from single signer list".to_string(),
                initial_signers: vec![Address::new_id(ANNE)],
                initial_approvals: 2,
                remove_signer: Address::new_id(ANNE),
                decrease: false,
                expect_signers: vec![],
                expect_approvals: 2,
                code: ExitCode::ErrForbidden,
            },
            SignerTestCase {
                desc: "Fail to remove non signer".to_string(),
                initial_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                initial_approvals: 2,
                remove_signer: Address::new_id(RICHARD),
                decrease: false,
                expect_signers: vec![
                    Address::new_id(ANNE),
                    Address::new_id(BOB),
                    Address::new_id(CHUCK),
                ],
                expect_approvals: 2,
                code: ExitCode::ErrNotFound,
            },
        ];
        for test_case in test_cases {
            println!("Test case executing is {}", test_case.desc);
            let receiver = Address::new_id(MULTISIG_WALLET_ADD);
            let message = UnsignedMessage::builder()
                .to(receiver.clone())
                .from(SYSTEM_ACTOR_ADDR.clone())
                .build()
                .unwrap();
            let bs = MemoryDB::default();
            let mut rt = MockRuntime::new(&bs, message);
            rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());

            construct_and_verify(
                &mut rt,
                test_case.initial_signers,
                test_case.initial_approvals,
                NO_LOCK_DURATION,
            );
            rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), receiver.clone());
            rt.expect_validate_caller_addr(&[receiver.clone()]);
            if test_case.code == ExitCode::Ok {
                assert!(
                    remove_signer(&mut rt, test_case.remove_signer, test_case.decrease).is_ok()
                );
                let state: State = rt.get_state().unwrap();
                assert_eq!(test_case.expect_signers, state.signers);
            //assert_eq!(test_case.expect_approvals, state.num_approvals_threshold);
            } else {
                assert_eq!(
                    test_case.code,
                    remove_signer(&mut rt, test_case.remove_signer, test_case.decrease)
                        .unwrap_err()
                        .exit_code()
                );
            }
            rt.verify();
        }
    }
}

mod test_swap_signers {
    use super::*;
    struct SwapTestCase {
        desc: String,
        to: Address,
        from: Address,
        expect: Vec<Address>,
        code: ExitCode,
    }
    const CHUCK: u64 = 103;
    const DARLENE: u64 = 104;
    const MULTISIG_WALLET_ADD: u64 = 100;
    const NO_LOCK_DURATION: i64 = 0;
    const NUM_APPROVALS: i64 = 1;

    #[test]
    fn test() {
        let test_cases = vec![
            SwapTestCase {
                desc: "happy path signer swap".to_string(),
                to: Address::new_id(CHUCK),
                from: Address::new_id(BOB),
                expect: vec![Address::new_id(ANNE), Address::new_id(CHUCK)],
                code: ExitCode::Ok,
            },
            SwapTestCase {
                desc: "fail to swap when from signer not found".to_string(),
                to: Address::new_id(CHUCK),
                from: Address::new_id(DARLENE),
                expect: vec![Address::new_id(ANNE), Address::new_id(CHUCK)],
                code: ExitCode::ErrNotFound,
            },
            SwapTestCase {
                desc: "fail to swap when to signer already present".to_string(),
                to: Address::new_id(BOB),
                from: Address::new_id(ANNE),
                expect: vec![Address::new_id(ANNE), Address::new_id(CHUCK)],
                code: ExitCode::ErrIllegalArgument,
            },
        ];
        let initial_signer = vec![Address::new_id(ANNE), Address::new_id(BOB)];
        for test_case in test_cases {
            println!("Test case executing is {}", test_case.desc);
            let receiver = Address::new_id(MULTISIG_WALLET_ADD);
            let message = UnsignedMessage::builder()
                .to(receiver.clone())
                .from(SYSTEM_ACTOR_ADDR.clone())
                .build()
                .unwrap();
            let bs = MemoryDB::default();
            let mut rt = MockRuntime::new(&bs, message);
            rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());

            construct_and_verify(
                &mut rt,
                initial_signer.clone(),
                NUM_APPROVALS,
                NO_LOCK_DURATION,
            );
            rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), receiver.clone());

            rt.expect_validate_caller_addr(&[receiver.clone()]);
            if test_case.code == ExitCode::Ok {
                assert!(swap_signers(&mut rt, test_case.from, test_case.to).is_ok());
                let state: State = rt.get_state().unwrap();
                assert_eq!(test_case.expect, state.signers);
            } else {
                assert_eq!(
                    test_case.code,
                    swap_signers(&mut rt, test_case.from, test_case.to)
                        .unwrap_err()
                        .exit_code()
                );
            }
            rt.verify();
        }
    }
}

mod test_change_treshold {
    use super::*;
    const CHUCK: u64 = 103;
    const MULTISIG_WALLET_ADD: u64 = 100;
    const NO_LOCK_DURATION: i64 = 0;
    struct Threshold {
        desc: String,
        initial_threshold: i64,
        set_threshold: i64,
        code: ExitCode,
    }

    #[test]
    fn test() {
        let initial_signer = vec![
            Address::new_id(ANNE),
            Address::new_id(BOB),
            Address::new_id(CHUCK),
        ];
        let test_cases = vec![
            Threshold {
                desc: "happy path decrease threshold".to_string(),
                initial_threshold: 2,
                set_threshold: 1,
                code: ExitCode::Ok,
            },
            Threshold {
                desc: "happy path simple increase threshold".to_string(),
                initial_threshold: 2,
                set_threshold: 3,
                code: ExitCode::Ok,
            },
            Threshold {
                desc: "fail to set threshold to zero".to_string(),
                initial_threshold: 2,
                set_threshold: 0,
                code: ExitCode::ErrIllegalArgument,
            },
            Threshold {
                desc: "fail to set threshold less than zero".to_string(),
                initial_threshold: 2,
                set_threshold: -1,
                code: ExitCode::ErrIllegalArgument,
            },
            Threshold {
                desc: "fail to set threshold above number of signers".to_string(),
                initial_threshold: 2,
                set_threshold: initial_signer.len() as i64 + 1,
                code: ExitCode::ErrIllegalArgument,
            },
        ];
        for test_case in test_cases {
            println!("Test case executing is {}", test_case.desc);
            let receiver = Address::new_id(MULTISIG_WALLET_ADD);
            let message = UnsignedMessage::builder()
                .to(receiver.clone())
                .from(SYSTEM_ACTOR_ADDR.clone())
                .build()
                .unwrap();
            let bs = MemoryDB::default();
            let mut rt = MockRuntime::new(&bs, message);
            rt.set_caller(INIT_ACTOR_CODE_ID.clone(), INIT_ACTOR_ADDR.clone());
            construct_and_verify(
                &mut rt,
                initial_signer.clone(),
                test_case.initial_threshold,
                NO_LOCK_DURATION,
            );
            rt.set_caller(ACCOUNT_ACTOR_CODE_ID.clone(), receiver.clone());
            rt.expect_validate_caller_addr(&[receiver.clone()]);
            if test_case.code == ExitCode::Ok {
                assert!(change_num_approvals_threshold(&mut rt, test_case.set_threshold).is_ok());
                let state: State = rt.get_state().unwrap();
                assert_eq!(test_case.set_threshold, state.num_approvals_threshold);
            } else {
                assert_eq!(
                    test_case.code,
                    change_num_approvals_threshold(&mut rt, test_case.set_threshold)
                        .unwrap_err()
                        .exit_code()
                );
            }
            rt.verify();
        }
    }
}