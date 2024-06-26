/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
use super::internal_batch_verify_transactions;
use super::{ProposedTransaction, Transaction};
use crate::serializing::fr::FrSerializable;
use crate::serializing::{bytes_to_hex, hex_to_bytes, hex_to_vec_bytes};
use crate::test_util::create_multisig_identities;
use crate::transaction::tests::split_spender_key::split_spender_key;
use crate::transaction::unsigned::UnsignedTransaction;
use crate::transaction::TRANSACTION_PUBLIC_KEY_SIZE;
use crate::{
    assets::{asset::Asset, asset_identifier::NATIVE_ASSET},
    errors::{IronfishError, IronfishErrorKind},
    frost_utils::split_spender_key,
    keys::SaplingKey,
    merkle_note::NOTE_ENCRYPTION_MINER_KEYS,
    note::Note,
    sapling_bls12::SAPLING,
    test_util::make_fake_witness,
    transaction::{
        batch_verify_transactions, verify_transaction, TransactionVersion,
        TRANSACTION_EXPIRATION_SIZE, TRANSACTION_FEE_SIZE, TRANSACTION_SIGNATURE_SIZE,
    },
};

use ff::Field;
use ironfish_frost::{
    frost::{round2, round2::SignatureShare, Identifier, Randomizer},
    nonces::deterministic_signing_nonces,
};
use ironfish_zkp::{
    constants::{ASSET_ID_LENGTH, SPENDING_KEY_GENERATOR, TREE_DEPTH},
    proofs::{MintAsset, Output, Spend},
    redjubjub::{self, Signature},
};
use rand::rngs::ThreadRng;
use rand::{thread_rng, CryptoRng, Rng, RngCore};

#[test]
fn test_transaction() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let sender_key = SaplingKey::generate_key();

    // Native asset
    let in_note = Note::new(
        spender_key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        sender_key.public_address(),
    );
    let out_note = Note::new(
        receiver_key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        spender_key.public_address(),
    );

    let witness = make_fake_witness(&in_note);

    // Custom asset
    let mint_value = 5;
    let burn_value = 2;

    let asset = Asset::new(
        spender_key.public_address(),
        "Testcoin",
        "A really cool coin",
    )
    .expect("should be able to create an asset");
    let mint_out_note = Note::new(
        receiver_key.public_address(),
        2,
        "",
        *asset.id(),
        spender_key.public_address(),
    );

    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());

    // Spend
    transaction.add_spend(in_note, &witness).unwrap();
    assert_eq!(transaction.spends.len(), 1);

    // Output
    transaction.add_output(out_note).unwrap();
    assert_eq!(transaction.outputs.len(), 1);

    // Mint 5 tokens
    transaction.add_mint(asset, mint_value).unwrap();
    assert_eq!(transaction.mints.len(), 1);

    // Output 2 minted tokens to receiver
    transaction.add_output(mint_out_note).unwrap();
    assert_eq!(transaction.outputs.len(), 2);

    // Burn 2 tokens, leaving 1 token left to be put into a change note
    transaction.add_burn(asset.id, burn_value).unwrap();
    assert_eq!(transaction.burns.len(), 1);

    let public_transaction = transaction
        .post(&spender_key, None, 1)
        .expect("should be able to post transaction");
    verify_transaction(&public_transaction).expect("Should be able to verify transaction");
    assert_eq!(public_transaction.fee(), 1);

    // 4 outputs:
    // - 1 change note for the native asset
    // - 1 change note for the custom asset
    // - 1 provided output to receiver for native asset
    // - 1 provided output to receiver for minted asset
    assert_eq!(public_transaction.outputs.len(), 4);

    // test serialization
    let mut serialized_transaction = vec![];
    public_transaction
        .write(&mut serialized_transaction)
        .expect("should be able to serialize transaction");

    let read_back_transaction: Transaction =
        Transaction::read(&mut serialized_transaction[..].as_ref())
            .expect("should be able to deserialize valid transaction");

    assert_eq!(
        read_back_transaction.expiration.to_le_bytes().len(),
        TRANSACTION_EXPIRATION_SIZE
    );
    assert_eq!(
        read_back_transaction.fee.to_le_bytes().len(),
        TRANSACTION_FEE_SIZE
    );
    assert_eq!(public_transaction.fee, read_back_transaction.fee);
    assert_eq!(
        public_transaction.spends.len(),
        read_back_transaction.spends.len()
    );
    assert_eq!(
        public_transaction.outputs.len(),
        read_back_transaction.outputs.len()
    );
    assert_eq!(
        public_transaction.mints.len(),
        read_back_transaction.mints.len()
    );
    assert_eq!(
        public_transaction.burns.len(),
        read_back_transaction.burns.len()
    );
    assert_eq!(
        public_transaction.expiration,
        read_back_transaction.expiration
    );
    let mut serialized_again = vec![];
    read_back_transaction
        .write(&mut serialized_again)
        .expect("should be able to serialize transaction again");
    assert_eq!(serialized_transaction, serialized_again);
}

#[test]
fn test_transaction_simple() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let sender_key = SaplingKey::generate_key();
    let spender_key_clone = spender_key.clone();

    let in_note = Note::new(
        spender_key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        sender_key.public_address(),
    );
    let out_note = Note::new(
        receiver_key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        spender_key.public_address(),
    );
    let witness = make_fake_witness(&in_note);

    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());
    transaction.add_spend(in_note, &witness).unwrap();
    assert_eq!(transaction.spends.len(), 1);
    transaction.add_output(out_note).unwrap();
    assert_eq!(transaction.outputs.len(), 1);

    let public_transaction = transaction
        .post(&spender_key, None, 1)
        .expect("should be able to post transaction");
    verify_transaction(&public_transaction).expect("Should be able to verify transaction");
    assert_eq!(public_transaction.fee(), 1);

    // A change note was created
    assert_eq!(public_transaction.outputs.len(), 2);
    assert_eq!(public_transaction.spends.len(), 1);
    assert_eq!(public_transaction.mints.len(), 0);
    assert_eq!(public_transaction.burns.len(), 0);

    let received_note = public_transaction.outputs[1]
        .merkle_note()
        .decrypt_note_for_owner(&spender_key_clone.incoming_viewing_key)
        .unwrap();
    assert_eq!(received_note.sender, spender_key_clone.public_address());
}

#[test]
fn test_proposed_transaction_build() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let sender_key = SaplingKey::generate_key();
    let spender_key_clone = spender_key.clone();

    let in_note = Note::new(
        spender_key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        sender_key.public_address(),
    );
    let out_note = Note::new(
        receiver_key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        spender_key.public_address(),
    );
    let witness = make_fake_witness(&in_note);

    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());
    transaction.add_spend(in_note, &witness).unwrap();
    assert_eq!(transaction.spends.len(), 1);
    transaction.add_output(out_note).unwrap();
    assert_eq!(transaction.outputs.len(), 1);

    let public_address: crate::PublicAddress = spender_key.public_address();
    let intended_fee = 1;

    let unsigned_transaction = transaction
        .build(
            spender_key.proof_authorizing_key,
            spender_key.view_key().clone(),
            spender_key.outgoing_view_key().clone(),
            intended_fee,
            Some(public_address),
        )
        .expect("should be able to build unsigned transaction");

    // A change note was created
    assert_eq!(unsigned_transaction.outputs.len(), 2);
    assert_eq!(unsigned_transaction.spends.len(), 1);
    assert_eq!(unsigned_transaction.mints.len(), 0);
    assert_eq!(unsigned_transaction.burns.len(), 0);

    let received_note = unsigned_transaction.outputs[1]
        .merkle_note()
        .decrypt_note_for_owner(&spender_key_clone.incoming_viewing_key)
        .unwrap();
    assert_eq!(received_note.sender, spender_key_clone.public_address());
}

#[test]
fn test_miners_fee() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let out_note = Note::new(
        receiver_key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        spender_key.public_address(),
    );
    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());
    transaction.add_output(out_note).unwrap();
    let posted_transaction = transaction
        .post_miners_fee(&spender_key)
        .expect("it is a valid miner's fee");
    assert_eq!(posted_transaction.fee, -42);
    assert_eq!(
        &posted_transaction
            .iter_outputs()
            .next()
            .unwrap()
            .merkle_note
            .note_encryption_keys,
        NOTE_ENCRYPTION_MINER_KEYS
    );
}

#[test]
fn test_transaction_signature() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let spender_address = spender_key.public_address();
    let receiver_address = receiver_key.public_address();
    let sender_key = SaplingKey::generate_key();

    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());
    let in_note = Note::new(
        spender_address,
        42,
        "",
        NATIVE_ASSET,
        sender_key.public_address(),
    );
    let out_note = Note::new(receiver_address, 41, "", NATIVE_ASSET, spender_address);
    let witness = make_fake_witness(&in_note);

    transaction.add_spend(in_note, &witness).unwrap();

    transaction.add_output(out_note).unwrap();

    transaction.set_expiration(1337);

    let public_transaction = transaction
        .post(&spender_key, None, 0)
        .expect("should be able to post transaction");

    let mut serialized_signature = vec![];
    public_transaction
        .binding_signature()
        .write(&mut serialized_signature)
        .unwrap();
    assert_eq!(serialized_signature.len(), TRANSACTION_SIGNATURE_SIZE);
    Signature::read(&mut serialized_signature[..].as_ref())
        .expect("Can deserialize back into a valid Signature");
}

#[test]
fn test_transaction_created_with_version_1() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let sender_key = SaplingKey::generate_key();

    let in_note = Note::new(
        spender_key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        sender_key.public_address(),
    );

    let out_note = Note::new(
        receiver_key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        spender_key.public_address(),
    );
    let witness = make_fake_witness(&in_note);

    let mut transaction = ProposedTransaction::new(TransactionVersion::V1);
    transaction.add_spend(in_note, &witness).unwrap();
    transaction.add_output(out_note).unwrap();

    assert_eq!(transaction.version, TransactionVersion::V1);

    let public_transaction = transaction
        .post(&spender_key, None, 1)
        .expect("should be able to post transaction");

    assert_eq!(public_transaction.version, TransactionVersion::V1);

    verify_transaction(&public_transaction).expect("version 1 transactions should be valid");
}

#[test]
fn test_transaction_version_is_checked() {
    fn assert_invalid_version(result: Result<Transaction, IronfishError>) {
        match result {
            Ok(_) => panic!("expected an error"),
            Err(IronfishError { kind, .. }) => match kind {
                IronfishErrorKind::InvalidTransactionVersion => {}
                _ => {
                    panic!("expected InvalidTransactionVersion, got {:?} instead", kind);
                }
            },
        }
    }

    let mut transaction = [0u8; 256];

    let valid_versions = [1u8, 2u8];
    let invalid_versions = (u8::MIN..=u8::MAX)
        .filter(|v| !valid_versions.contains(v))
        .collect::<Vec<u8>>();
    assert_eq!(invalid_versions.len(), 254);

    // Verify that valid versions are correctly deserialized
    for version in valid_versions {
        transaction[0] = version;
        assert!(Transaction::read(&transaction[..]).is_ok());
    }

    // Verify that invalid versions result in InvalidTransactionVersion upon deserialization
    for version in invalid_versions {
        transaction[0] = version;
        assert_invalid_version(Transaction::read(&transaction[..]));
    }
}

#[test]
fn test_transaction_value_overflows() {
    let key = SaplingKey::generate_key();

    let overflow_value = (i64::MAX as u64) + 1;

    let asset = Asset::new(key.public_address(), "testcoin", "").unwrap();

    let note = Note::new(
        key.public_address(),
        overflow_value,
        "",
        NATIVE_ASSET,
        key.public_address(),
    );
    let witness = make_fake_witness(&note);

    let mut tx = ProposedTransaction::new(TransactionVersion::latest());

    // spend
    assert!(tx.add_spend(note.clone(), &witness).is_err());

    // output
    assert!(tx.add_output(note).is_err());

    // mint
    assert!(tx.add_mint(asset, overflow_value).is_err());

    // burn
    assert!(tx.add_burn(asset.id, overflow_value).is_err());
}

#[test]
fn test_batch_verify_wrong_params() {
    let rng = &mut thread_rng();

    let wrong_spend_params =
        bellperson::groth16::generate_random_parameters::<blstrs::Bls12, _, _>(
            Spend {
                value_commitment: None,
                proof_generation_key: None,
                payment_address: None,
                commitment_randomness: None,
                ar: None,
                auth_path: vec![None; TREE_DEPTH],
                anchor: None,
                sender_address: None,
            },
            rng,
        )
        .unwrap();

    let wrong_output_params =
        bellperson::groth16::generate_random_parameters::<blstrs::Bls12, _, _>(
            Output {
                value_commitment: None,
                payment_address: None,
                commitment_randomness: None,
                esk: None,
                asset_id: [0; ASSET_ID_LENGTH],
                ar: None,
                proof_generation_key: None,
            },
            rng,
        )
        .unwrap();

    let wrong_mint_params = bellperson::groth16::generate_random_parameters::<blstrs::Bls12, _, _>(
        MintAsset {
            proof_generation_key: None,
            public_key_randomness: None,
        },
        rng,
    )
    .unwrap();

    let wrong_spend_vk = bellperson::groth16::prepare_verifying_key(&wrong_spend_params.vk);
    let wrong_output_vk = bellperson::groth16::prepare_verifying_key(&wrong_output_params.vk);
    let wrong_mint_vk = bellperson::groth16::prepare_verifying_key(&wrong_mint_params.vk);

    //
    // TRANSACTION GENERATION
    //
    let key = SaplingKey::generate_key();
    let other_key = SaplingKey::generate_key();

    // Native asset
    let in_note = Note::new(
        key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        key.public_address(),
    );
    let out_note = Note::new(
        key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        key.public_address(),
    );

    let witness = make_fake_witness(&in_note);

    // Custom asset
    let mint_value = 5;
    let burn_value = 2;

    let asset1 = Asset::new(key.public_address(), "Testcoin", "A really cool coin")
        .expect("should be able to create an asset");

    let asset2 = Asset::new(other_key.public_address(), "Othercoin", "").unwrap();

    let mint_out_note = Note::new(
        key.public_address(),
        2,
        "",
        *asset1.id(),
        key.public_address(),
    );

    let mut proposed_transaction1 = ProposedTransaction::new(TransactionVersion::latest());

    proposed_transaction1.add_spend(in_note, &witness).unwrap();
    proposed_transaction1.add_output(out_note).unwrap();

    proposed_transaction1.add_mint(asset1, mint_value).unwrap();
    proposed_transaction1.add_output(mint_out_note).unwrap();

    proposed_transaction1
        .add_burn(asset1.id, burn_value)
        .unwrap();

    let transaction1 = proposed_transaction1
        .post(&key, None, 1)
        .expect("should be able to post transaction");

    let mut proposed_transaction2 = ProposedTransaction::new(TransactionVersion::latest());
    proposed_transaction2.add_mint(asset2, 5).unwrap();

    let transaction2 = proposed_transaction2.post(&other_key, None, 0).unwrap();
    //
    // END TRANSACTION CREATION
    //

    batch_verify_transactions([&transaction1, &transaction2])
        .expect("Should verify using Sapling params");
    internal_batch_verify_transactions(
        [&transaction1, &transaction2],
        &wrong_spend_vk,
        &SAPLING.output_verifying_key,
        &SAPLING.mint_verifying_key,
    )
    .expect_err("Should not verify if spend verifying key is wrong");
    internal_batch_verify_transactions(
        [&transaction1, &transaction2],
        &SAPLING.spend_verifying_key,
        &wrong_output_vk,
        &SAPLING.mint_verifying_key,
    )
    .expect_err("Should not verify if output verifying key is wrong");
    internal_batch_verify_transactions(
        [&transaction1, &transaction2],
        &SAPLING.spend_verifying_key,
        &SAPLING.output_verifying_key,
        &wrong_mint_vk,
    )
    .expect_err("Should not verify if mint verifying key is wrong");
}

#[test]
fn test_batch_verify() {
    let key = SaplingKey::generate_key();
    let other_key = SaplingKey::generate_key();

    let public_key_randomness = jubjub::Fr::random(thread_rng());
    let other_randomized_public_key =
        redjubjub::PublicKey(other_key.view_key.authorizing_key.into())
            .randomize(public_key_randomness, *SPENDING_KEY_GENERATOR);

    // Native asset
    let in_note = Note::new(
        key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        key.public_address(),
    );
    let out_note = Note::new(
        key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        key.public_address(),
    );

    let witness = make_fake_witness(&in_note);

    // Custom asset
    let mint_value = 5;
    let burn_value = 2;

    let asset1 = Asset::new(key.public_address(), "Testcoin", "A really cool coin")
        .expect("should be able to create an asset");

    let asset2 = Asset::new(other_key.public_address(), "Othercoin", "").unwrap();

    let mint_out_note = Note::new(
        key.public_address(),
        2,
        "",
        *asset1.id(),
        key.public_address(),
    );

    let mut proposed_transaction1 = ProposedTransaction::new(TransactionVersion::latest());

    proposed_transaction1.add_spend(in_note, &witness).unwrap();
    proposed_transaction1.add_output(out_note).unwrap();

    proposed_transaction1.add_mint(asset1, mint_value).unwrap();
    proposed_transaction1.add_output(mint_out_note).unwrap();

    proposed_transaction1
        .add_burn(asset1.id, burn_value)
        .unwrap();

    let mut transaction1 = proposed_transaction1
        .post(&key, None, 1)
        .expect("should be able to post transaction");

    let mut proposed_transaction2 = ProposedTransaction::new(TransactionVersion::latest());
    proposed_transaction2.add_mint(asset2, 5).unwrap();

    let transaction2 = proposed_transaction2.post(&other_key, None, 0).unwrap();

    batch_verify_transactions([&transaction1, &transaction2])
        .expect("should be able to verify transaction");

    transaction1.randomized_public_key = other_randomized_public_key;

    assert!(matches!(
        batch_verify_transactions([&transaction1, &transaction2]),
        Err(e) if matches!(e.kind, IronfishErrorKind::InvalidSpendSignature)
    ));
}

#[test]
fn test_sign_simple() {
    let spender_key = SaplingKey::generate_key();
    let receiver_key = SaplingKey::generate_key();
    let sender_key = SaplingKey::generate_key();

    let in_note = Note::new(
        spender_key.public_address(),
        42,
        "",
        NATIVE_ASSET,
        sender_key.public_address(),
    );
    let out_note = Note::new(
        receiver_key.public_address(),
        40,
        "",
        NATIVE_ASSET,
        spender_key.public_address(),
    );
    let witness = make_fake_witness(&in_note);

    // create transaction, add spend and output
    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());
    transaction
        .add_spend(in_note, &witness)
        .expect("should be able to add a spend");
    transaction
        .add_output(out_note)
        .expect("should be able to add an output");

    // build transaction, generate proofs
    let unsigned_transaction = transaction
        .build(
            spender_key.proof_authorizing_key,
            spender_key.view_key().clone(),
            spender_key.outgoing_view_key().clone(),
            1,
            Some(spender_key.public_address()),
        )
        .expect("should be able to build unsigned transaction");

    // sign transaction
    let signed_transaction = unsigned_transaction
        .sign(&spender_key)
        .expect("should be able to sign transaction");

    // verify transaction
    verify_transaction(&signed_transaction).expect("should be able to verify transaction");
}

#[test]
fn test_aggregate_signature_shares() {
    let spender_key = SaplingKey::generate_key();

    let identities = create_multisig_identities(10);

    // key package generation by trusted dealer
    let key_packages = split_spender_key(&spender_key, 2, &identities)
        .expect("should be able to split spender key");

    // create raw/proposed transaction
    let in_note = Note::new(
        key_packages.public_address,
        42,
        "",
        NATIVE_ASSET,
        key_packages.public_address,
    );
    let out_note = Note::new(
        key_packages.public_address,
        40,
        "",
        NATIVE_ASSET,
        key_packages.public_address,
    );
    let asset = Asset::new(
        key_packages.public_address,
        "Testcoin",
        "A really cool coin",
    )
    .expect("should be able to create an asset");
    let value = 5;
    let mint_out_note = Note::new(
        key_packages.public_address,
        value,
        "",
        *asset.id(),
        key_packages.public_address,
    );
    let witness = make_fake_witness(&in_note);

    let mut transaction = ProposedTransaction::new(TransactionVersion::latest());
    transaction
        .add_spend(in_note, &witness)
        .expect("add spend to transaction");
    assert_eq!(transaction.spends.len(), 1);
    transaction
        .add_output(out_note)
        .expect("add output to transaction");
    assert_eq!(transaction.outputs.len(), 1);
    transaction
        .add_mint(asset, value)
        .expect("add mint to transaction");
    transaction
        .add_output(mint_out_note)
        .expect("add mint output to transaction");

    let intended_fee = 1;
    transaction
        .add_change_notes(
            Some(key_packages.public_address),
            key_packages.public_address,
            intended_fee,
        )
        .expect("should be able to add change notes");

    // build UnsignedTransaction without signing
    let mut unsigned_transaction = transaction
        .build(
            key_packages.proof_authorizing_key,
            key_packages.view_key,
            key_packages.outgoing_view_key,
            intended_fee,
            Some(key_packages.public_address),
        )
        .expect("should be able to build unsigned transaction");

    let transaction_hash = unsigned_transaction
        .transaction_signature_hash()
        .expect("should be able to compute transaction hash");

    let mut commitments = HashMap::new();

    // simulate round 1
    for (identity, key_package) in key_packages.key_packages.iter() {
        let nonces = deterministic_signing_nonces(
            key_package.signing_share(),
            &transaction_hash,
            &identities,
        );
        commitments.insert(identity.clone(), (&nonces).into());
    }

    // coordinator creates signing package
    let signing_package = unsigned_transaction
        .signing_package(commitments)
        .expect("should be able to create signing package");

    // simulate round 2
    let mut signature_shares: BTreeMap<Identifier, SignatureShare> = BTreeMap::new();
    let randomizer =
        Randomizer::deserialize(&unsigned_transaction.public_key_randomness.to_bytes())
            .expect("should be able to deserialize randomizer");

    for (identity, key_package) in key_packages.key_packages.iter() {
        let nonces = deterministic_signing_nonces(
            key_package.signing_share(),
            &transaction_hash,
            &identities,
        );
        let signature_share = round2::sign(
            &signing_package.frost_signing_package,
            &nonces,
            key_package,
            randomizer,
        )
        .expect("should be able to create signature share");
        signature_shares.insert(identity.to_frost_identifier(), signature_share);
    }

    // coordinator creates signed transaction
    let signed_transaction = unsigned_transaction
        .aggregate_signature_shares(
            &key_packages.public_key_package,
            &signing_package.frost_signing_package,
            signature_shares,
        )
        .expect("should be able to sign transaction");

    assert_eq!(signed_transaction.spends.len(), 1);
    assert_eq!(signed_transaction.outputs.len(), 3);
    assert_eq!(signed_transaction.mints.len(), 1);
    assert_eq!(signed_transaction.burns.len(), 0);

    // verify transaction
    verify_transaction(&signed_transaction).expect("should be able to verify transaction");
}

// struct StaticRng<'a>(&'a [u8]);

// impl<'a> RngCore for StaticRng<'a> {
//     fn next_u32(&mut self) -> u32 {
//         let (bytes, remaining) = self
//             .0
//             .split_first_chunk::<4>()
//             .expect("not enough random bytes");
//         self.0 = remaining;
//         u32::from_le_bytes(*bytes)
//     }

//     fn next_u64(&mut self) -> u64 {
//         let (bytes, remaining) = self
//             .0
//             .split_first_chunk::<8>()
//             .expect("not enough random bytes");
//         self.0 = remaining;
//         u64::from_le_bytes(*bytes)
//     }

//     fn fill_bytes(&mut self, dest: &mut [u8]) {
//         assert!(self.0.len() >= dest.len(), "not enough random bytes");
//         dest.copy_from_slice(&self.0[..dest.len()]);
//         self.0 = &self.0[dest.len()..];
//     }

//     fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
//         if self.0.len() >= dest.len() {
//             dest.copy_from_slice(&self.0[..dest.len()]);
//             self.0 = &self.0[dest.len()..];
//             Ok(())
//         } else {
//             Err(rand_core::Error::new("not enough random bytes"))
//         }
//     }
// }

// #[cfg_attr(doc_cfg, doc(cfg(feature = "std_rng")))]
// #[derive(Clone, Debug, PartialEq, Eq)]
// impl ZRng {
//     pub fn from_bytes(seed_bytes: [u8; 80]) -> ZRng {
//         ZRng { seed_bytes }
//     }
// }

// impl RngCore for ZRng {
//     #[inline(always)]
//     fn next_u32(&mut self) -> u32 {
//         let mut bytes: [u8; 4] = [0u8; 4];
//         bytes.copy_from_slice(&self.seed_bytes[..4]);
//         u32::from_be_bytes(bytes)
//     }

//     #[inline(always)]
//     fn next_u64(&mut self) -> u64 {
//         let mut bytes: [u8; 8] = [0u8; 8];
//         bytes.copy_from_slice(&self.seed_bytes[..8]);
//         u64::from_be_bytes(bytes)
//     }

//     #[inline(always)]
//     fn fill_bytes(&mut self, dest: &mut [u8]) {
//         dest.copy_from_slice(&self.seed_bytes)
//     }

//     #[inline(always)]
//     fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
//         dest.copy_from_slice(&self.seed_bytes);
//         Ok(())
//     }
// }

// impl CryptoRng for ZRng {}

// #[test]
// fn test_sign_zondax() {
//     let spender_key =
//         SaplingKey::from_hex("eedb03842adc584156cc3bad24d9c576b24362e0fd859ae5373983321204ba05")
//             .unwrap();

//     let unsigned_transaction_bytes = hex_to_vec_bytes("02010000000000000002000000000000000000000000000000000000000000000001000000000000000000000092f503d1a8c9788f0e597c18f9517f8a5f6d9c94f3b5ddc81758413e49042ac76644780b9ac3e0eb11d006e85e9aa06bab650e1f87c0a7ecda7def2c6a1bf70b6644780b9ac3e0eb11d006e85e9aa06bab650e1f87c0a7ecda7def2c6a1bf70b83cf4d23a656c425fee6dc152fbda21b1d4647a231b362c589ed65d835ce4842e64a8acc0ee02597bd3a3901542bc86d997f4e7334667549d22c5b271e3a53daf98b530b93a421a60ddd7943a3e4a28bda2328d8a502e006e025b6c6794cea6901d65b0fa16a97928bfa923bb84aaa1f98d61fe927f569842acbcf92c435c7437d615044500e65f4ed38be725e0cef2381345079c4f39bd5841cfb92f1348b0f0317f2c34e169d3f68304de895ca8860b9b741eae3d0c797954da476edfbc5e692a239edfcd4270ea5cb953e696f37611170b9194217d80978803d75665653b60cbe3350134de28f30af1227201b16336b686d788773aa9fddb30a013fefba45780500007a6fa1535b6ce2daf53393af01483cae6a0dd4743fcbbe29a2a2b6da96ba2a0400000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a3be2bb53e7d58e62cfcac7e48b990e64eb97445ca7bffab23a2c8d07cf178107f9f2fc6370cfd676ea228a62c00ed21a3c811a0f419975948844f8103fcd636a86c904d5b6b457ed717ca12b6ea8bd88a495ca13a26ebef61e4fe5e794ff45002828acfbb8096271b533c6551daaa0d85b8d8c5372c7509e0035f1cff22a6fab269965e915a9da8e361c88701e66294b00acaeec890a1c2c96c9b1991b40056b70dfb5b1dfe0fec9f96c8338b09b7c643ea6242f23524f1abce04b0ae9d309e3561426df404ce7f159ffb5e2e595693bc929bf778a39144eea35cfb23d3d0cd8c963113512cffeb4aac3286998f0d19b111418dacdd7be0941176e468557d3f576a47a7b753acc13f6cc077b3ff308acf535302796176e0306c2376efb7265ecbc16e35dcbbb1834b701443ca7dd03b6c1942b8562b3789774ffdfc77d1f9ec7b895994caedf85ee36ce7e96dd62d0616af5b7e41a0bafd64342041d7695d6b0672ebbd6e44c4739e428d857dd5ba8b9ae3a7f05e49f1e335ac1e4fa85f2674aa7c196d23f8f4db753ebca05bb93ecf340e88dea1be88e4acdc30609a27d5215a6d288af174ace25a751ec5370282223fba5c05b8aa7e9f5cb2b4d729aba84f2edb88100a2f9bc3363b0c94bf4f5c6e07078304734daea4be441a5efe371c781475eed22d306011a049a2b5401714f91eb3db78f8f731a29a36366241ad6785cbc6d58d3075c4b9870a4f0a471f0cfc5603d7ee4065e1657ac4fdedddb4542063d19f7eda4c51b966d7fd3bc1f8ac2a9355ced60f92b432ad2ca773c6e7351a07fce2d54033436317e40f5895acbf4481ebbef4a9622b6e83209bb25a24fe60c9685370d280b8eb05d1ddfe2feeee9ff4e5b4d5ad51d4b334321a3b7c6bebb4ab66fe9c9001c800fe99928a7d23941bb0f442ea326e985dae5fe7489fee07a35c075e23a16a03886ab8a9de570bfbb6677cf0dd8480e347fa46eea9729bbdcb600aae7bdb879ef0b22f4487d3c2c16c845c82632c0b4b8bff34eebb9809eb256795a103c8ed5c6dd926d2f259f6280f36a9882f3207bae2c5ba45a2555b6fab54680d00a28b5402d889e30c1333a8501db1279e86433772238e4a84d57ce9c3832213c5eac607ab326b017f6427ec4988a16f34443eb0e19969fabffc1d39859e630a951aabcc869670ad305aab499024cec781149945fe80100e29aac9fb0b44c93b407af14fc0d25f9220d75073e137f4223cf65677868b7c9684df167e40c4c1f50b45ab88d560feba2c15e32b6b375c0f37856ce17fdcafbbe7ee4f9b5348641027351f50156de06546bfc052947e0f27e049e1eea169dff3ee5d35b8f848b4809cb96e876c061feae26260d5a3dc5c621efcb65c424d905c6ebf314f6b246d4d07abe72f47665fc7a4a0b4a80963bbc7ffa0b0a3be28a45cee1c5b9428425768c8dd3ad851d527bc8ad1ea1872d2aaa76a495444771e8b9e2f16f5068edaec5e3910e4872df5c34f68c942951a93f56cb5e5ee47d9223e03a6fcc9de52d28ff409ba255c6d0c67d28daa359c0a").expect("should read unsigned transaction bytes");

//     let unsigned_transaction = UnsignedTransaction::read(&unsigned_transaction_bytes[..])
//         .expect("should deserialize unsigned transaction");

//     let transaction_hash = "c5031c241ee5e89776d1964bd2247eb50b04b096bc29d683f80be71636486b97";
//     let rng_string = "abddfeea2f28d1d6a39d87cc7e43370b052d2738eb9d4cb01b360dda07945e13a6494273ad6b7085805e8fc94c23276e46f5442144c3803f92bb1efb5059ee615861d7a387f14a1ae591834aa28d19a8";
//     let signature = "00115b9e1d1bf1b2f917478055884dc9a3457d427bba82db5c5207ec73ce452fd0029511f26281c174501f88ddce26df48458b34d9079027ab92116ede992b0d";
//     let mut rng = ZRng::from_bytes(rng_string);

//     let transaction_hash_bytes = unsigned_transaction.transaction_signature_hash().unwrap();
//     println!("{}", bytes_to_hex(&transaction_hash_bytes));
//     // assert transaction hash
//     assert_eq!(bytes_to_hex(&transaction_hash_bytes), transaction_hash);
//     println!("c5031c241ee5e89776d1964bd2247eb50b04b096bc29d683f80be71636486b97");

//     let private_key = redjubjub::PrivateKey(spender_key.spend_authorizing_key);
//     let randomized_private_key = private_key.randomize(unsigned_transaction.public_key_randomness);

//     let transaction_randomized_public_key =
//         redjubjub::PublicKey(spender_key.view_key.authorizing_key.into()).randomize(
//             unsigned_transaction.public_key_randomness,
//             *SPENDING_KEY_GENERATOR,
//         );

//     let mut data_to_be_signed = [0; 64];
//     let public_key_bytes = transaction_randomized_public_key.0.to_string().into_bytes();
//     data_to_be_signed[..TRANSACTION_PUBLIC_KEY_SIZE].copy_from_slice(&public_key_bytes[..]);
//     data_to_be_signed[32..].copy_from_slice(&transaction_hash_bytes[..]);

//     // let rng_bytes = hex_to_bytes(rng_string).expect("should read rng bytes");
//     // let mut rng = StaticRng(&rng_bytes);

//     // TODO: Get RngCore from rng

//     // create thread rng from rng
//     // RngCore::
//     let signature =
//         randomized_private_key.sign(&data_to_be_signed, &mut rng, *SPENDING_KEY_GENERATOR);

//     let mut serialized_signature = vec![];
//     signature
//         .write(&mut serialized_signature)
//         .expect("serializing signature should not fail");

//     println!("{}", bytes_to_hex(&serialized_signature));
//     println!("00115b9e1d1bf1b2f917478055884dc9a3457d427bba82db5c5207ec73ce452fd0029511f26281c174501f88ddce26df48458b34d9079027ab92116ede992b0d")
// }

// #[test]
// fn test_aggregate_signatures() {
//     let unsigned = "0101000000000000000200000000000000000000000000000000000000000000000100000000000000000000001d4a95c1b3efd2fe83e68691fdc6c92a86491aa59dd2b59c496359a9443724cc00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000a491a32a0cc9074486823f3606f1a01ad84ef3ad6fcbe03264a1cb945d542903f0c5ba7b1e896cf3d20a6336787e8078a2656552352096a947fae5b38688dd097ff5a5eb348bcf1b648c31b82cd98eb9c3e2227995de6946c21a86ed17ffcd7a10cd7e6fecc354513dcd263500d6c7695abe26865fb227fd2b1bb8af546b8799ebc6b02767e1ebaef8c3d3e610e12d9fb0c9d5327563c0686a260708467e81e2f5a314237fd52c71a0dfa09ec217ef20d839c66c476dfd29f6a539838ce510e3eee5fa9db8f72a312c6d52cb53a0708e8b327b96829a8cc8cad2e89116c0cfe5d607ffc6e591f48c725704be1c2817e0b58545ff9090e404e496b9904bcd6a28bb0b0900df54ce691413ddec47234a9ac23884f031b50156b45cc47e8ae017cbb749152b000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000008c28972516dada122772971aead5415bd91e35515b4e0fdc203b41808f93d17466d668cbf190d203602c52a4fb8c20e5aa25e4b9e2cedbd4ba36ae0399c50001c0380337cc30adf44e27e2810d15327227e67b606b545c1863ab7acd8e5fcb3b09f8b88c53d160ddb543f36e98d304d9cfbd09ec2715c48bb679c536961c02c42996c231e5ddb57d86a0cb9121e399a3abb6f82125838a0bb43c19b973e1b12e0b449d06c1219514d575894645acee952539f9a687c2567d900bdf440285e2c5ecad7da98b716324f51fbe97aab23b755e100c45159d72f085dc76b418aa63d346cc85b718bcbb81cf909843e88d416c463ba886d3ae0149f76f9b43c485f41d6f7de98d91471d07d52ac6608eca01341a26180aaf46377db354675dd4a2af511b38deab7c1481255ae3d89a030bc1f402b1b92c14e33539b7f478470cb0a4e911c2cf13eaf138322fbe33da0fd1bde6e73f085decda78e70b3136709cd9c1476c494ebbc1bc96693ce528770f1e02bad49b6e6e67cca5e346b20850db1aa29f1675b684bc66079ad86e5e215aeaa0dfaedda798f31b277373417609e1f84e97a7444c4305f183cdce93b6ee6f86c9a058441a9f32872f853e0f3ccd0b001a302afa3d4828e7af724ffb74bc8d211d698095b2e0ee48efa2b46b0da5f8765302af354f9dba638b653924bc16f36d8a46e4827d680297e2fc4e8dda2fec1ad1572c50c7d5d4bc95c6884a8f135db770a356623f31ce785587adea64b8eb3b09e060a8a0bda0d6f818ecec79897a496e7d7c09c73e56a229e08a2677cdf3b53944d04878f19d13fb1726831a7609627c709a78502fedbe9ab082afe0d2caf1bfdc18a72f15c5f456291560647807e08c4f240f8dc559b925e87b1a884a2151fdcbba8bb40a70a1e06a50c4f33ec3127c698d2b5fa80e47af52994aaff14efdca3c71c26cdffbec65ba75a3509289b1ef609a458c4fc9965a0a8826e153af375a4d27b48913243ef9eae535edb423a64ad302eb802c2831087e91b9ce7f2b9b21ed8f33eeb760194c36430e84297ec96600d170201267cce91438aecaaa405419614eddb0218db45f256a3ea3a72baf23b654b51f581b3f8289a35f8c91dbdc5fa4596bf21e3b534217057905511fe160ccf43a3d1c104af7971e5f3c4007819d333413716a653f2bd2fafd859094e6bcddae3ea42b4dcfae2aa91c070708f36267d4ad486859f384027b8f329a6a6f4724f21f618cf3d47bb3991c41b8fa490a960035e6a094290927c473b550bcee2e233548e402d00bc0b22e47e9e9c345cac67ab57293e1afac9f5596292e17e5ded7545e7f340a81c7b96e1b215aa32a8c68309a1438890603ebc1a8d1310ecb902a316270e1e9994e17a160c6bda195212241001ef46acfa57f62efdb82737839d5e65da6d6f9ca04f9f9f85f22155a5ea20eca3766d104153b91889e3db1eabda1f746dee486080ac49737591dd1a1c1591efa1350ce0edb2e173724962a0362f239d6618402dd00efe58c17ab3840179ad310b87977dae6f57acfb8f572877109";
//     let mut signatures = Vec::new();
//     signatures.push("84f12c978f1ff72adc5972bf11cdb5b4c9b219a3714ac01d502ef9523f6a00ab000dec9464574776c7863321b28df533184c3d20aed10f96ef9954306ae30604".to_string());
//     let bytes = hex_to_vec_bytes(&unsigned).expect("invalid hex");
//     let mut unsigned_transaction = UnsignedTransaction::read(&*bytes).expect("invalid bytes");
//     let signed_transaction = unsigned_transaction
//         .aggregate_signatures(signatures)
//         .expect("error aggregating signatures");

//     verify_transaction(&signed_transaction).expect("should be able to verify transaction");
// }
