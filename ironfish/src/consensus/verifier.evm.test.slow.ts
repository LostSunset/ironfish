/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

jest.mock('ws')

import '../testUtilities/matchers/blockchain'
import { LegacyTransaction } from '@ethereumjs/tx'
import { Account as EthAccount, Address } from '@ethereumjs/util'
import ContractArtifact from '@ironfish/ironfish-contracts'
import { Asset } from '@ironfish/rust-nodejs'
import { ethers } from 'ethers'
import { Assert } from '../assert'
import { EvmShield } from '../evm'
import { FullNode } from '../node'
import { Transaction } from '../primitives'
import { EvmDescription, legacyTransactionToEvmDescription } from '../primitives/evmDescription'
import { TransactionVersion } from '../primitives/transaction'
import { createNodeTest, useAccountFixture, useMinerBlockFixture } from '../testUtilities'
import { SpendingAccount } from '../wallet'
import { Consensus } from './consensus'
import { VerificationResultReason } from './verifier'

describe('Verifier', () => {
  describe('EVM Transaction', () => {
    const nodeTest = createNodeTest()
    let node: FullNode
    let senderAccountIf: SpendingAccount
    let description: EvmDescription
    let evmSenderAddress: Address
    let evmRecipientAddress: Address
    let evmPrivateKey: Uint8Array

    beforeEach(async () => {
      jest
        .spyOn(Consensus.prototype, 'getActiveTransactionVersion')
        .mockImplementation(() => TransactionVersion.V3)

      const { node: n } = await nodeTest.createSetup()
      node = n

      senderAccountIf = await useAccountFixture(node.wallet, 'sender')
      const recipientAccountIf = await useAccountFixture(node.wallet, 'recipient')

      evmPrivateKey = Uint8Array.from(Buffer.from(senderAccountIf.spendingKey || '', 'hex'))
      const recipientPrivateKey = Uint8Array.from(
        Buffer.from(recipientAccountIf.spendingKey, 'hex'),
      )

      evmSenderAddress = Address.fromPrivateKey(evmPrivateKey)
      evmRecipientAddress = Address.fromPrivateKey(recipientPrivateKey)

      const senderAccount = new EthAccount(BigInt(0), 500_000_000n)

      await node.chain.blockchainDb.stateManager.checkpoint()
      await node.chain.blockchainDb.stateManager.putAccount(evmSenderAddress, senderAccount)
      await node.chain.blockchainDb.stateManager.commit()

      const tx = new LegacyTransaction({
        nonce: 0n,
        to: evmRecipientAddress,
        value: 200000n,
        gasLimit: 21000n,
        gasPrice: 7n,
      })
      const signed = tx.sign(evmPrivateKey)

      description = legacyTransactionToEvmDescription(signed)
    })

    it('verify transaction returns true on valid evm transaction', async () => {
      const raw = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: description,
      })
      const transaction = raw.post(senderAccountIf.spendingKey || '')
      const deserialized = new Transaction(transaction.serialize())
      const result = await node.chain.verifier.verifyNewTransaction(deserialized)

      expect(result).toEqual({ valid: true })
    })

    it('verify transaction returns true on valid contract deployment transaction', async () => {
      // Deploy the global contract
      const tx = new LegacyTransaction({
        gasLimit: 1_000_000n,
        gasPrice: 7n,
        data: ContractArtifact.bytecode,
      })

      const signed = tx.sign(evmPrivateKey)

      description = legacyTransactionToEvmDescription(signed)

      const raw = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: description,
      })

      const transaction = raw.post(senderAccountIf.spendingKey || '')
      const deserialized = new Transaction(transaction.serialize())
      const result = await node.chain.verifier.verifyNewTransaction(deserialized)

      expect(result).toEqual({ valid: true })
    })

    it('verify transaction returns true on valid shield transaction', async () => {
      let tx: LegacyTransaction

      tx = new LegacyTransaction({
        gasLimit: 1_000_000n,
        gasPrice: 7n,
        data: ContractArtifact.bytecode,
        nonce: 0n,
      })

      Assert.isNotUndefined(node.chain.evm)

      const result = await node.chain.evm.runTx({ tx: tx.sign(evmPrivateKey) })
      const globalContractAddress = result?.createdAddress

      Assert.isNotUndefined(globalContractAddress)

      const contract = await node.chain.blockchainDb.stateManager.getAccount(
        globalContractAddress,
      )

      expect(contract).toBeDefined()

      const globalContract = new ethers.Interface(ContractArtifact.abi)

      const encodedFunctionData = globalContract.encodeFunctionData('shield', [
        Buffer.from(senderAccountIf.publicAddress, 'hex'),
        Asset.nativeId(),
        100n,
      ])

      tx = new LegacyTransaction({
        nonce: 1n,
        gasLimit: 100_000n,
        to: globalContractAddress,
        gasPrice: 7n,
        data: encodedFunctionData,
      })

      let signed = tx.sign(evmPrivateKey)

      description = legacyTransactionToEvmDescription(signed)

      const raw = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: description,
      })

      const transaction = raw.post(senderAccountIf.spendingKey || '')
      const deserialized = new Transaction(transaction.serialize())
      const verificationResult = await node.chain.verifier.verifyNewTransaction(deserialized)

      expect(verificationResult).toEqual({ valid: true })

      tx = new LegacyTransaction({
        nonce: 2n,
        gasLimit: 100_000n,
        to: globalContractAddress,
        gasPrice: 7n,
        data: encodedFunctionData,
      })

      signed = tx.sign(evmPrivateKey)

      const evmResult = await node.chain.evm.verifyTx({ tx: signed })

      const shieldEvents = evmResult.events.filter(
        (event) => event.name === 'shield',
      ) as EvmShield[]

      expect(shieldEvents).toHaveLength(1)
      expect(shieldEvents[0].amount).toEqual(100n)
      expect(shieldEvents[0].ironfishAddress.toString('hex')).toEqual(
        senderAccountIf.publicAddress,
      )
      expect(shieldEvents[0].caller).toEqual(evmSenderAddress)
      expect(shieldEvents[0].assetId).toEqual(Asset.nativeId())
    })

    it('verify transaction returns false on global account mint non-evm', async () => {
      // TODO(jwp): use proof generation key of IronfishEvm to create mint, but senderAccount for transaction
      const mint = {
        creator: senderAccountIf.publicAddress,
        name: '0x00000000000000000000000000000000002cd37f',
        metadata: '',
        value: 1n,
      }

      const raw = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        mints: [mint],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: description,
      })
      const transaction = raw.post(senderAccountIf.spendingKey || '')
      // first mint can occur in any transaction
      const block = await useMinerBlockFixture(
        node.chain,
        undefined,
        senderAccountIf,
        node.wallet,
        [transaction],
      )
      await expect(node.chain).toAddBlock(block)

      const mint2 = {
        creator: senderAccountIf.publicAddress,
        name: '0x00000000000000000000000000000000002cd37f',
        metadata: '',
        value: 2n,
      }

      const raw2 = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        mints: [mint2],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
      })
      const transaction2 = raw2.post(senderAccountIf.spendingKey || '')

      const deserialized = new Transaction(transaction2.serialize())
      // Try to mint again, this time should fail
      const result = await node.chain.verifier.verifyNewTransaction(deserialized)

      expect(result).toEqual({
        valid: false,
        reason: VerificationResultReason.EVM_MINT_NON_EVM,
      })
    })

    // TODO(jwp): test mint/burn mismatch balance/length

    it('verify transaction returns false when evm transaction has invalid signature', async () => {
      // Change the signature to be invalid
      const busted = { ...description, s: Buffer.alloc(32) }

      const raw = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: busted,
      })
      const transaction = raw.post(senderAccountIf.spendingKey || '')
      const deserialized = new Transaction(transaction.serialize())
      const result = await node.chain.verifier.verifyNewTransaction(deserialized)

      expect(result).toEqual({
        valid: false,
        reason: VerificationResultReason.EVM_TRANSACTION_INVALID_SIGNATURE,
      })
    })
    it('fails validation when consecutive transaction uses too much balance in a block', async () => {
      const evmAccount = await node.chain.blockchainDb.stateManager.getAccount(evmSenderAddress)
      Assert.isNotUndefined(evmAccount)
      const tx = new LegacyTransaction({
        nonce: 0n,
        to: evmRecipientAddress,
        value: evmAccount.balance / 2n,
        gasLimit: 21000n,
        gasPrice: 7n,
      })
      const signed = tx.sign(evmPrivateKey)
      const evmDescription = legacyTransactionToEvmDescription(signed)
      const raw = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: evmDescription,
      })
      const transaction = raw.post(senderAccountIf.spendingKey || '')

      const tx2 = new LegacyTransaction({
        nonce: 0n,
        to: evmRecipientAddress,
        value: evmAccount.balance / 2n,
        gasLimit: 21000n,
        gasPrice: 7n,
      })
      const signed2 = tx2.sign(evmPrivateKey)
      const evmDescription2 = legacyTransactionToEvmDescription(signed2)
      const raw2 = await node.wallet.createTransaction({
        account: senderAccountIf,
        outputs: [],
        fee: 0n,
        expiration: 0,
        expirationDelta: 0,
        evm: evmDescription2,
      })
      const transaction2 = raw2.post(senderAccountIf.spendingKey || '')

      const invalidBlock = useMinerBlockFixture(
        node.chain,
        undefined,
        senderAccountIf,
        node.wallet,
        [transaction, transaction2],
      )
      await expect(invalidBlock).rejects.toThrow(
        VerificationResultReason.EVM_TRANSACTION_INSUFFICIENT_BALANCE,
      )
    })
  })
})
