// Copyright 2020 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT

use actor::{
    self, ACCOUNT_ACTOR_CODE_ID, CRON_ACTOR_CODE_ID, INIT_ACTOR_CODE_ID, MARKET_ACTOR_CODE_ID,
    MINER_ACTOR_CODE_ID, MULTISIG_ACTOR_CODE_ID, PAYCH_ACTOR_CODE_ID, POWER_ACTOR_CODE_ID,
    REWARD_ACTOR_CODE_ID, SYSTEM_ACTOR_CODE_ID, VERIFIED_ACTOR_CODE_ID,
};
use address::Address;
use cid::{multihash::Blake2b256, Cid};
use clock::ChainEpoch;
use crypto::{DomainSeparationTag, Signature};
use encoding::blake2b_256;
use encoding::{de::DeserializeOwned, Cbor};
use fil_types::{PieceInfo, RegisteredProof, SealVerifyInfo, WindowPoStVerifyInfo};
use ipld_blockstore::BlockStore;
use message::{Message, UnsignedMessage};
use runtime::{ActorCode, Runtime, Syscalls};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use vm::{ActorError, ExitCode, MethodNum, Randomness, Serialized, TokenAmount};

use runtime::ConsensusFault;
use std::error::Error as StdError;

pub struct MockRuntime<'a, BS: BlockStore> {
    pub epoch: ChainEpoch,
    pub caller_type: Cid,
    pub miner: Address,
    //pub value_received: TokenAmount,
    pub id_addresses: HashMap<Address, Address>,
    pub actor_code_cids: HashMap<Address, Cid>,
    pub new_actor_addr: Option<Address>,
    pub message: UnsignedMessage,

    // TODO: syscalls: syscaller

    // Actor State
    pub state: Option<Cid>,
    pub balance: TokenAmount,
    pub received: TokenAmount,

    // VM Impl
    pub in_call: bool,
    pub store: &'a BS,
    pub in_transaction: bool,

    // Expectations
    pub expect_validate_caller_any: Cell<bool>,
    pub expect_validate_caller_addr: RefCell<Option<Vec<Address>>>,
    pub expect_validate_caller_type: RefCell<Option<Vec<Cid>>>,
    pub expect_sends: VecDeque<ExpectedMessage>,
    pub expect_create_actor: Option<ExpectCreateActor>,
    pub expect_verify_sig: RefCell<Option<ExpectedVerifySig>>,
    pub expect_verify_seal: RefCell<Option<ExpectedVerifySeal>>,
    pub expect_verify_post: RefCell<Option<ExpectedVerifyPost>>,
}

#[derive(Clone, Debug)]
pub struct ExpectCreateActor {
    pub code_id: Cid,
    pub address: Address,
}
#[derive(Clone, Debug)]
pub struct ExpectedMessage {
    pub to: Address,
    pub method: MethodNum,
    pub params: Serialized,
    pub value: TokenAmount,

    // returns from applying expectedMessage
    pub send_return: Serialized,
    pub exit_code: ExitCode,
}

#[derive(Clone, Debug)]
pub struct ExpectedVerifySig {
    sig: Signature,
    signer: Address,
    plaintext: Vec<u8>,
    result: ExitCode,
}

#[derive(Clone, Debug)]
pub struct ExpectedVerifySeal {
    seal: SealVerifyInfo,
    result: ExitCode,
}
#[derive(Clone, Debug)]
pub struct ExpectedVerifyPost {
    post: WindowPoStVerifyInfo,
    result: ExitCode,
}

impl<'a, BS> MockRuntime<'a, BS>
where
    BS: BlockStore,
{
    pub fn new(bs: &'a BS, message: UnsignedMessage) -> Self {
        MockRuntime {
            epoch: 0,
            caller_type: Cid::default(),

            miner: Address::new_id(0),

            id_addresses: HashMap::new(),
            actor_code_cids: HashMap::new(),
            new_actor_addr: None,

            message: message,
            state: None,
            balance: 0u8.into(),
            received: 0u8.into(),

            // VM Impl
            in_call: false,
            store: bs,
            in_transaction: false,

            // Expectations
            expect_validate_caller_any: Cell::new(false),
            expect_validate_caller_addr: RefCell::new(None),
            expect_validate_caller_type: RefCell::new(None),
            expect_sends: VecDeque::new(),
            expect_create_actor: None,
            expect_verify_sig: RefCell::new(None),
            expect_verify_seal: RefCell::new(None),
            expect_verify_post: RefCell::new(None),
        }
    }
    fn require_in_call(&self) {
        assert!(
            self.in_call,
            "invalid runtime invocation outside of method call",
        )
    }
    fn check_argument(&self, predicate: bool, msg: String) -> Result<(), ActorError> {
        if !predicate {
            return Err(ActorError::new(ExitCode::SysErrorIllegalArgument, msg));
        }
        Ok(())
    }
    fn put<C: Cbor>(&self, o: &C) -> Result<Cid, ActorError> {
        Ok(self.store.put(&o, Blake2b256).unwrap())
    }
    fn _get<T: DeserializeOwned>(&self, cid: Cid) -> Result<T, ActorError> {
        Ok(self.store.get(&cid).unwrap().unwrap())
    }

    #[allow(dead_code)]
    pub fn get_state<T: DeserializeOwned>(&self) -> Result<T, ActorError> {
        let data: T = self
            .store
            .get(&self.state.as_ref().unwrap())
            .unwrap()
            .unwrap();
        Ok(data)
    }
    pub fn expect_validate_caller_addr(&self, addr: &[Address]) {
        assert!(addr.len() > 0, "addrs must be non-empty");
        *self.expect_validate_caller_addr.borrow_mut() = Some(addr.to_vec());
    }

    #[allow(dead_code)]
    pub fn expect_validate_caller_type(&self, ids: &[Cid]) {
        assert!(ids.len() > 0, "addrs must be non-empty");
        *self.expect_validate_caller_type.borrow_mut() = Some(ids.to_vec());
    }

    #[allow(dead_code)]
    pub fn expect_verify_signature(
        &mut self,
        sig: Signature,
        signer: Address,
        plaintext: Vec<u8>,
        result: ExitCode,
    ) {
        self.expect_verify_sig = RefCell::new(Some(ExpectedVerifySig {
            sig: sig,
            signer: signer,
            plaintext: plaintext,
            result: result,
        }));
    }

    #[allow(dead_code)]
    pub fn expect_validate_caller_any(&self) {
        self.expect_validate_caller_any.set(true);
    }

    pub fn call(
        &mut self,
        to_code: &Cid,
        method_num: MethodNum,
        params: &Serialized,
    ) -> Result<Serialized, ActorError> {
        self.in_call = true;
        let prev_state = self.state.clone();

        let res = match to_code {
            x if x == &*SYSTEM_ACTOR_CODE_ID => {
                actor::system::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*INIT_ACTOR_CODE_ID => {
                actor::init::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*CRON_ACTOR_CODE_ID => {
                actor::cron::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*ACCOUNT_ACTOR_CODE_ID => {
                actor::account::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*POWER_ACTOR_CODE_ID => {
                actor::power::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*MINER_ACTOR_CODE_ID => {
                actor::miner::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*MARKET_ACTOR_CODE_ID => {
                actor::market::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*PAYCH_ACTOR_CODE_ID => {
                actor::paych::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*MULTISIG_ACTOR_CODE_ID => {
                actor::multisig::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*REWARD_ACTOR_CODE_ID => {
                actor::reward::Actor.invoke_method(self, method_num, params)
            }
            x if x == &*VERIFIED_ACTOR_CODE_ID => {
                actor::verifreg::Actor.invoke_method(self, method_num, params)
            }
            _ => Err(ActorError::new(
                ExitCode::SysErrForbidden,
                "invalid method id".to_owned(),
            )),
        };

        if res.is_err() {
            self.state = prev_state;
        }
        self.in_call = false;
        return res;
    }
    pub fn verify(&mut self) {
        assert!(
            !self.expect_validate_caller_any.get(),
            "expected ValidateCallerAny, not received"
        );
        assert!(
            self.expect_validate_caller_addr.borrow().as_ref().is_none(),
            "expected ValidateCallerAddr {:?}, not received",
            self.expect_validate_caller_addr.borrow().as_ref().unwrap()
        );
        assert!(
            self.expect_validate_caller_type.borrow().as_ref().is_none(),
            "expected ValidateCallerType {:?}, not received",
            self.expect_validate_caller_type.borrow().as_ref().unwrap()
        );
        assert!(
            self.expect_sends.is_empty(),
            "expected all message to be send, unsent messages {:?}",
            self.expect_sends
        );
        assert!(
            self.expect_create_actor.is_none(),
            "expected actor to be created, uncreated actor: {:?}",
            self.expect_create_actor
        );

        self.reset();
    }
    pub fn reset(&mut self) {
        self.expect_validate_caller_any.set(false);
        *self.expect_validate_caller_addr.borrow_mut() = None;
        *self.expect_validate_caller_type.borrow_mut() = None;
        self.expect_create_actor = None;
        *self.expect_verify_sig.borrow_mut() = None;
        self.expect_sends.clear();
    }

    #[allow(dead_code)]
    pub fn expect_send(
        &mut self,
        to: Address,
        method: MethodNum,
        params: Serialized,
        value: TokenAmount,
        send_return: Serialized,
        exit_code: ExitCode,
    ) {
        self.expect_sends.push_back(ExpectedMessage {
            to,
            method,
            params,
            value,
            send_return,
            exit_code,
        })
    }

    #[allow(dead_code)]
    pub fn expect_create_actor(&mut self, code_id: Cid, address: Address) {
        let a = ExpectCreateActor { code_id, address };
        self.expect_create_actor = Some(a);
    }

    #[allow(dead_code)]
    pub fn set_caller(&mut self, code_id: Cid, address: Address) {
        self.message = UnsignedMessage::builder()
            .to(self.message.to().clone())
            .from(address.clone())
            .value(self.message.value().clone())
            .build()
            .unwrap();
        self.caller_type = code_id.clone();
        self.actor_code_cids.insert(address, code_id);
    }

    #[allow(dead_code)]
    pub fn set_value(&mut self, value: TokenAmount) {
        self.message = UnsignedMessage::builder()
            .to(self.message.to().clone())
            .from(self.message.from().clone())
            .value(value)
            .build()
            .unwrap();
    }
}

impl<BS> Runtime<BS> for MockRuntime<'_, BS>
where
    BS: BlockStore,
{
    fn message(&self) -> &UnsignedMessage {
        self.require_in_call();
        &self.message
    }

    fn curr_epoch(&self) -> ChainEpoch {
        self.require_in_call();
        self.epoch
    }

    fn validate_immediate_caller_accept_any(&self) {
        self.require_in_call();
        assert!(
            self.expect_validate_caller_any.get(),
            "unexpected validate-caller-any"
        );
        self.expect_validate_caller_any.set(false);
    }

    fn validate_immediate_caller_is<'a, I>(&self, addresses: I) -> Result<(), ActorError>
    where
        I: IntoIterator<Item = &'a Address>,
    {
        self.require_in_call();

        let addrs: Vec<Address> = addresses.into_iter().cloned().collect();

        self.check_argument(addrs.len() > 0, "addrs must be non-empty".to_owned())?;

        assert!(
            self.expect_validate_caller_addr.borrow().is_some(),
            "unexpected validate caller addrs"
        );
        assert!(
            &addrs == self.expect_validate_caller_addr.borrow().as_ref().unwrap(),
            "unexpected validate caller addrs {:?}, expected {:?}",
            addrs,
            self.expect_validate_caller_addr.borrow().as_ref()
        );

        for expected in &addrs {
            if self.message().from() == expected {
                *self.expect_validate_caller_addr.borrow_mut() = None;
                return Ok(());
            }
        }
        *self.expect_validate_caller_addr.borrow_mut() = None;
        return Err(ActorError::new(
            ExitCode::ErrForbidden,
            format!(
                "caller address {:?} forbidden, allowed: {:?}",
                self.message().from(),
                &addrs
            ),
        ));
    }
    fn validate_immediate_caller_type<'a, I>(&self, types: I) -> Result<(), ActorError>
    where
        I: IntoIterator<Item = &'a Cid>,
    {
        self.require_in_call();
        let types: Vec<Cid> = types.into_iter().cloned().collect();

        self.check_argument(types.len() > 0, "types must be non-empty".to_owned())?;

        assert!(
            self.expect_validate_caller_type.borrow().is_some(),
            "unexpected validate caller code"
        );
        assert!(
            &types == self.expect_validate_caller_type.borrow().as_ref().unwrap(),
            "unexpected validate caller code {:?}, expected {:?}",
            types,
            self.expect_validate_caller_type
        );

        *self.expect_validate_caller_type.borrow_mut() = None;

        for expected in &types {
            if &self.caller_type == expected {
                *self.expect_validate_caller_type.borrow_mut() = None;
                return Ok(());
            }
        }

        Err(self.abort(
            ExitCode::ErrForbidden,
            format!(
                "caller type {:?} forbidden, allowed: {:?}",
                self.caller_type, types
            ),
        ))
    }

    fn current_balance(&self) -> Result<TokenAmount, ActorError> {
        self.require_in_call();
        Ok(self.balance.clone())
    }

    fn resolve_address(&self, address: &Address) -> Result<Address, ActorError> {
        self.require_in_call();
        if address.protocol() == address::Protocol::ID {
            return Ok(address.clone());
        }
        let resolved = self.id_addresses.get(&address).unwrap();
        return Ok(resolved.clone());
    }

    fn get_actor_code_cid(&self, addr: &Address) -> Result<Cid, ActorError> {
        self.require_in_call();
        if let Some(ret) = self.actor_code_cids.get(&addr) {
            return Ok(ret.clone());
        }
        Err(ActorError::new(
            ExitCode::ErrIllegalArgument,
            "Actor address is not found".to_string(),
        ))
    }

    fn get_randomness(
        &self,
        _personalization: DomainSeparationTag,
        _rand_epoch: ChainEpoch,
        _entropy: &[u8],
    ) -> Result<Randomness, ActorError> {
        unimplemented!()
    }

    fn create<C: Cbor>(&mut self, obj: &C) -> Result<(), ActorError> {
        if self.state.is_some() == true {
            return Err(self.abort(
                ExitCode::SysErrorIllegalActor,
                "state already constructed".to_owned(),
            ));
        }
        self.state = Some(self.store.put(obj, Blake2b256).unwrap());
        Ok(())
    }

    fn state<C: Cbor>(&self) -> Result<C, ActorError> {
        Ok(self
            .store
            .get(&self.state.as_ref().unwrap())
            .unwrap()
            .unwrap())
    }

    fn transaction<C: Cbor, R, F>(&mut self, f: F) -> Result<R, ActorError>
    where
        F: FnOnce(&mut C, &mut Self) -> R,
    {
        if self.in_transaction {
            return Err(self.abort(ExitCode::SysErrorIllegalActor, "nested transaction"));
        }
        let mut read_only = self.state()?;
        self.in_transaction = true;
        let ret = f(&mut read_only, self);
        self.state = Some(self.put(&read_only).unwrap());
        self.in_transaction = false;
        Ok(ret)
    }

    fn store(&self) -> &BS {
        self.store
    }

    fn send(
        &mut self,
        to: &Address,
        method: MethodNum,
        params: &Serialized,
        value: &TokenAmount,
    ) -> Result<Serialized, ActorError> {
        self.require_in_call();
        if self.in_transaction {
            return Err(self.abort(
                ExitCode::SysErrorIllegalActor,
                "side-effect within transaction",
            ));
        }
        assert!(
            !self.expect_sends.is_empty(),
            "unexpected expectedMessage to: {:?} method: {:?}, value: {:?}, params: {:?}",
            to,
            method,
            value,
            params
        );

        let expected_msg = self.expect_sends.pop_front().unwrap();

        assert!(&expected_msg.to == to && expected_msg.method == method && &expected_msg.params == params && &expected_msg.value == value, "expectedMessage being sent does not match expectation.\nMessage -\t to: {:?} method: {:?} value: {:?} params: {:?}\nExpected -\t {:?}", to, method, value, params, self.expect_sends[0]);

        if value > &self.balance {
            return Err(self.abort(
                ExitCode::SysErrSenderStateInvalid,
                format!(
                    "cannot send value: {:?} exceeds balance: {:?}",
                    value, self.balance
                ),
            ));
        }
        self.balance -= value;

        match expected_msg.exit_code {
            ExitCode::Ok => return Ok(expected_msg.send_return),
            x => {
                return Err(ActorError::new(x, "Expected message Fail".to_string()));
            }
        }
    }

    fn abort<S: AsRef<str>>(&self, exit_code: ExitCode, msg: S) -> ActorError {
        ActorError::new(exit_code, msg.as_ref().to_owned())
    }

    fn new_actor_address(&mut self) -> Result<Address, ActorError> {
        self.require_in_call();
        let ret = self
            .new_actor_addr
            .as_ref()
            .expect("unexpected call to new actor address")
            .clone();
        self.new_actor_addr = None;
        return Ok(ret);
    }

    fn create_actor(&mut self, code_id: &Cid, address: &Address) -> Result<(), ActorError> {
        self.require_in_call();
        if self.in_transaction {
            return Err(self.abort(
                ExitCode::SysErrorIllegalActor,
                "side-effect within transaction".to_owned(),
            ));
        }
        let expect_create_actor = self
            .expect_create_actor
            .take()
            .expect("unexpected call to create actor");

        assert!(&expect_create_actor.code_id == code_id && &expect_create_actor.address == address, "unexpected actor being created, expected code: {:?} address: {:?}, actual code: {:?} address: {:?}", expect_create_actor.code_id, expect_create_actor.address, code_id, address);
        Ok(())
    }

    fn delete_actor(&mut self, _beneficiary: &Address) -> Result<(), ActorError> {
        self.require_in_call();
        if self.in_transaction {
            return Err(self.abort(
                ExitCode::SysErrorIllegalActor,
                "side-effect within transaction".to_owned(),
            ));
        }
        todo!("implement me???")
    }

    fn total_fil_circ_supply(&self) -> Result<TokenAmount, ActorError> {
        unimplemented!();
    }

    fn syscalls(&self) -> &dyn Syscalls {
        self
    }
}

impl<BS> Syscalls for MockRuntime<'_, BS>
where
    BS: BlockStore,
{
    fn verify_signature(
        &self,
        _signature: &Signature,
        _signer: &Address,
        _plaintext: &[u8],
    ) -> Result<(), Box<dyn StdError>> {
        let op_exp = self.expect_verify_sig.replace(Option::None);

        if let Some(exp) = op_exp {
            if exp.sig == *_signature
                && exp.signer == *_signer
                && (exp.plaintext[..]) == *_plaintext
            {
                if exp.result == ExitCode::Ok {
                    return Ok(());
                } else {
                    return Err(Box::new(ActorError::new(
                        exp.result,
                        "Expected failure".to_string(),
                    )));
                }
            } else {
                return Err(Box::new(ActorError::new(
                    ExitCode::ErrIllegalState,
                    "Signatures did not match".to_string(),
                )));
            }
        } else {
            return Err(Box::new(ActorError::new(
                ExitCode::ErrPlaceholder,
                "Expected verify sig not there ".to_string(),
            )));
        }
    }

    fn hash_blake2b(&self, _data: &[u8]) -> Result<[u8; 32], Box<dyn StdError>> {
        Ok(blake2b_256(&_data))
    }
    fn compute_unsealed_sector_cid(
        &self,
        _reg: RegisteredProof,
        _pieces: &[PieceInfo],
    ) -> Result<Cid, Box<dyn StdError>> {
        unimplemented!();
    }
    fn verify_seal(&self, _vi: &SealVerifyInfo) -> Result<(), Box<dyn StdError>> {
        let op_exp = self.expect_verify_seal.replace(Option::None);
        if let Some(exp) = op_exp {
            if exp.seal == *_vi {
                if exp.result == ExitCode::Ok {
                    return Ok(());
                } else {
                    return Err(Box::new(ActorError::new(
                        exp.result,
                        "Expected failure".to_string(),
                    )));
                }
            } else {
                return Err(Box::new(ActorError::new(
                    ExitCode::ErrIllegalState,
                    "Seal did not match".to_string(),
                )));
            }
        } else {
            return Err(Box::new(ActorError::new(
                ExitCode::ErrPlaceholder,
                "Expected verify seal not there ".to_string(),
            )));
        }
    }
    fn batch_verify_seals(
        &self,
        vis: &[(Address, Vec<SealVerifyInfo>)],
    ) -> Result<HashMap<Address, Vec<bool>>, Box<dyn StdError>> {
        let mut out: HashMap<Address, Vec<bool>> = HashMap::new();
        for (k, v) in vis {
            out.insert(*k, vec![true; v.len()]);
        }
        Ok(out)
    }

    fn verify_post(&self, _vi: &WindowPoStVerifyInfo) -> Result<(), Box<dyn StdError>> {
        let op_exp = self.expect_verify_post.replace(Option::None);
        if let Some(exp) = op_exp {
            if exp.post == *_vi {
                if exp.result == ExitCode::Ok {
                    return Ok(());
                } else {
                    return Err(Box::new(ActorError::new(
                        exp.result,
                        "Expected failure".to_string(),
                    )));
                }
            } else {
                return Err(Box::new(ActorError::new(
                    ExitCode::ErrIllegalState,
                    "Post did not match".to_string(),
                )));
            }
        } else {
            return Err(Box::new(ActorError::new(
                ExitCode::ErrPlaceholder,
                "Expected verify post not there ".to_string(),
            )));
        }
    }
    fn verify_consensus_fault(
        &self,
        _h1: &[u8],
        _h2: &[u8],
        _extra: &[u8],
        _earliest: ChainEpoch,
    ) -> Result<Option<ConsensusFault>, Box<dyn StdError>> {
        unimplemented!();
    }
}
