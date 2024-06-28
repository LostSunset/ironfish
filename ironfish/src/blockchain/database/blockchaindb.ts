/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
import { DefaultStateManager } from '@ethereumjs/statemanager'
import { Trie } from '@ethereumjs/trie'
import { ValueEncoding } from '@ethereumjs/util'
import { Assert } from '../../assert'
import { EvmStateDB } from '../../evm/database'
import { FileSystem } from '../../fileSystems'
import { BlockHeader } from '../../primitives'
import { BlockHash } from '../../primitives/blockheader'
import { TransactionHash } from '../../primitives/transaction'
import {
  BUFFER_ENCODING,
  DatabaseSchema,
  IDatabase,
  IDatabaseStore,
  IDatabaseTransaction,
  SchemaKey,
  SchemaValue,
  StringEncoding,
  U32_ENCODING,
} from '../../storage'
import { createDB } from '../../storage/utils'
import {
  AssetSchema,
  HashToNextSchema,
  HeadersSchema,
  MetaSchema,
  SequenceToHashesSchema,
  SequenceToHashSchema,
  TransactionHashToBlockHashSchema,
  TransactionsSchema,
} from '../schema'
import { AssetValue, AssetValueEncoding } from './assetValue'
import { HeaderEncoding, HeaderValue } from './headers'
import { SequenceToHashesValueEncoding } from './sequenceToHashes'
import { TransactionsValue, TransactionsValueEncoding } from './transactions'

export const VERSION_DATABASE_CHAIN = 28

export class BlockchainDB {
  db: IDatabase
  location: string
  files: FileSystem

  // BlockHash -> BlockHeader
  headers: IDatabaseStore<HeadersSchema>
  // Contains flat fields
  meta: IDatabaseStore<MetaSchema>
  // BlockHash -> BlockHeader
  transactions: IDatabaseStore<TransactionsSchema>
  // Sequence -> BlockHash[]
  sequenceToHashes: IDatabaseStore<SequenceToHashesSchema>
  // Sequence -> BlockHash
  sequenceToHash: IDatabaseStore<SequenceToHashSchema>
  // BlockHash -> BlockHash
  hashToNextHash: IDatabaseStore<HashToNextSchema>
  // Asset Identifier -> Asset
  assets: IDatabaseStore<AssetSchema>
  // TransactionHash -> BlockHash
  transactionHashToBlockHash: IDatabaseStore<TransactionHashToBlockHashSchema>

  stateManager: DefaultStateManager

  constructor(options: { location: string; files: FileSystem }) {
    this.location = options.location
    this.files = options.files
    this.db = createDB({ location: options.location })

    // BlockHash -> BlockHeader
    this.headers = this.db.addStore({
      name: 'bh',
      keyEncoding: BUFFER_ENCODING,
      valueEncoding: new HeaderEncoding(),
    })

    // Flat Fields
    this.meta = this.db.addStore({
      name: 'bm',
      keyEncoding: new StringEncoding<'head' | 'latest'>(),
      valueEncoding: BUFFER_ENCODING,
    })

    // BlockHash -> Transaction[]
    this.transactions = this.db.addStore({
      name: 'bt',
      keyEncoding: BUFFER_ENCODING,
      valueEncoding: new TransactionsValueEncoding(),
    })

    // number -> BlockHash[]
    this.sequenceToHashes = this.db.addStore({
      name: 'bs',
      keyEncoding: U32_ENCODING,
      valueEncoding: new SequenceToHashesValueEncoding(),
    })

    // number -> BlockHash
    this.sequenceToHash = this.db.addStore({
      name: 'bS',
      keyEncoding: U32_ENCODING,
      valueEncoding: BUFFER_ENCODING,
    })

    this.hashToNextHash = this.db.addStore({
      name: 'bH',
      keyEncoding: BUFFER_ENCODING,
      valueEncoding: BUFFER_ENCODING,
    })

    this.assets = this.db.addStore({
      name: 'bA',
      keyEncoding: BUFFER_ENCODING,
      valueEncoding: new AssetValueEncoding(),
    })

    this.transactionHashToBlockHash = this.db.addStore({
      name: 'tb',
      keyEncoding: BUFFER_ENCODING,
      valueEncoding: BUFFER_ENCODING,
    })

    this.stateManager = new DefaultStateManager({
      trie: new Trie({
        db: new EvmStateDB(this.db),
        valueEncoding: ValueEncoding.Bytes,
        useRootPersistence: true,
      }),
    })
  }

  async open(): Promise<void> {
    await this.files.mkdir(this.location, { recursive: true })
    await this.db.open()
    await this.db.upgrade(VERSION_DATABASE_CHAIN)
  }

  async close(): Promise<void> {
    await this.db.close()
  }

  async getBlockHeader(
    blockHash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<BlockHeader | undefined> {
    return (await this.headers.get(blockHash, tx))?.header
  }

  async deleteHeader(hash: Buffer, tx?: IDatabaseTransaction): Promise<void> {
    return this.headers.del(hash, tx)
  }

  async putBlockHeader(
    hash: Buffer,
    header: HeaderValue,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.headers.put(hash, header, tx)
  }

  async getMetaHash(
    key: 'head' | 'latest',
    tx?: IDatabaseTransaction,
  ): Promise<Buffer | undefined> {
    return this.meta.get(key, tx)
  }

  async putMetaHash(
    key: 'head' | 'latest',
    value: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.meta.put(key, value, tx)
  }

  async getTransactions(
    blockHash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<TransactionsValue | undefined> {
    return this.transactions.get(blockHash, tx)
  }

  async addTransaction(
    hash: Buffer,
    value: TransactionsValue,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.transactions.add(hash, value, tx)
  }

  async putTransaction(
    hash: Buffer,
    value: TransactionsValue,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.transactions.put(hash, value, tx)
  }

  async deleteTransaction(hash: Buffer, tx?: IDatabaseTransaction): Promise<void> {
    return this.transactions.del(hash, tx)
  }

  async getBlockHashesAtSequence(
    sequence: number,
    tx?: IDatabaseTransaction,
  ): Promise<BlockHash[]> {
    const hashes = await this.sequenceToHashes.get(sequence, tx)
    if (!hashes) {
      return []
    }

    return hashes.hashes
  }

  async getBlockHeadersAtSequence(
    sequence: number,
    tx?: IDatabaseTransaction,
  ): Promise<BlockHeader[]> {
    const hashes = await this.sequenceToHashes.get(sequence, tx)

    if (!hashes) {
      return []
    }

    const headers = await Promise.all(
      hashes.hashes.map(async (h) => {
        const header = await this.getBlockHeader(h, tx)
        Assert.isNotUndefined(header)
        return header
      }),
    )

    return headers
  }

  async addHashAtSequence(
    sequence: number,
    hash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    const hashes = await this.getBlockHashesAtSequence(sequence, tx)
    return this.sequenceToHashes.put(sequence, { hashes: [...hashes, hash] }, tx)
  }

  async removeHashAtSequence(
    sequence: number,
    hash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    const result = await this.getBlockHashesAtSequence(sequence, tx)
    const hashes = result.filter((h) => !h.equals(hash))
    if (hashes.length === 0) {
      await this.sequenceToHashes.del(sequence, tx)
    } else {
      return this.sequenceToHashes.put(sequence, { hashes }, tx)
    }
  }

  async getBlockHashAtSequence(
    sequence: number,
    tx?: IDatabaseTransaction,
  ): Promise<BlockHash | undefined> {
    return this.sequenceToHash.get(sequence, tx)
  }

  async getBlockHeaderAtSequence(sequence: number): Promise<BlockHeader | undefined> {
    const hash = await this.sequenceToHash.get(sequence)
    if (!hash) {
      return undefined
    }

    return this.getBlockHeader(hash)
  }

  async putSequenceToHash(
    sequence: number,
    hash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.sequenceToHash.put(sequence, hash, tx)
  }

  async deleteSequenceToHash(sequence: number, tx?: IDatabaseTransaction): Promise<void> {
    return this.sequenceToHash.del(sequence, tx)
  }

  async clearSequenceToHash(tx?: IDatabaseTransaction): Promise<void> {
    return this.sequenceToHash.clear(tx)
  }

  async getNextHash(
    hash: BlockHash,
    tx?: IDatabaseTransaction,
  ): Promise<BlockHash | undefined> {
    return this.hashToNextHash.get(hash, tx)
  }

  async putNextHash(hash: Buffer, nextHash: Buffer, tx?: IDatabaseTransaction): Promise<void> {
    return this.hashToNextHash.put(hash, nextHash, tx)
  }

  async deleteNextHash(hash: Buffer, tx?: IDatabaseTransaction): Promise<void> {
    return this.hashToNextHash.del(hash, tx)
  }

  async clearHashToNextHash(tx?: IDatabaseTransaction): Promise<void> {
    return this.hashToNextHash.clear(tx)
  }

  async getAsset(assetId: Buffer, tx?: IDatabaseTransaction): Promise<AssetValue | undefined> {
    return this.assets.get(assetId, tx)
  }

  async putAsset(
    assetId: Buffer,
    assetValue: AssetValue,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.assets.put(assetId, assetValue, tx)
  }

  async deleteAsset(assetId: Buffer, tx?: IDatabaseTransaction): Promise<void> {
    return this.assets.del(assetId, tx)
  }

  async getBlockHashByTransactionHash(
    transactionHash: TransactionHash,
    tx?: IDatabaseTransaction,
  ): Promise<BlockHash | undefined> {
    return this.transactionHashToBlockHash.get(transactionHash, tx)
  }

  async transactionHashHasBlock(
    transactionHash: TransactionHash,
    tx?: IDatabaseTransaction,
  ): Promise<boolean> {
    return this.transactionHashToBlockHash.has(transactionHash, tx)
  }

  async putTransactionHashToBlockHash(
    transactionHash: Buffer,
    blockHash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.transactionHashToBlockHash.put(transactionHash, blockHash, tx)
  }

  async deleteTransactionHashToBlockHash(
    transactionHash: Buffer,
    tx?: IDatabaseTransaction,
  ): Promise<void> {
    return this.transactionHashToBlockHash.del(transactionHash, tx)
  }

  async compact(): Promise<void> {
    return this.db.compact()
  }

  async getVersion(): Promise<number> {
    return this.db.getVersion()
  }

  transaction<TResult>(
    handler: (transaction: BlockchainDBTransaction) => Promise<TResult>,
  ): Promise<TResult>
  transaction(): BlockchainDBTransaction
  transaction(
    handler?: (transaction: BlockchainDBTransaction) => Promise<unknown>,
  ): IDatabaseTransaction | Promise<unknown> {
    if (handler === undefined) {
      return new BlockchainDBTransaction(this.db, this.stateManager)
    }

    return this.withTransaction(null, handler)
  }

  // TODO(hughy): this is copied from Database. can/should we reuse the underlying implementation?
  async withTransaction<TResult>(
    transaction: BlockchainDBTransaction | undefined | null,
    handler: (transaction: BlockchainDBTransaction) => Promise<TResult>,
  ): Promise<TResult> {
    const created = !transaction
    transaction = transaction || this.transaction()

    try {
      await transaction.acquireLock()
      const result = await handler(transaction)
      if (created) {
        await transaction.commit()
      }
      return result
    } catch (error: unknown) {
      if (created) {
        await transaction.abort()
      }
      throw error
    }
  }

  async size(): Promise<number> {
    return this.db.size()
  }
}

export class BlockchainDBTransaction implements IDatabaseTransaction {
  tx: IDatabaseTransaction
  stateManager: DefaultStateManager
  checkpoint = false
  cache: Map<Buffer, unknown>

  constructor(db: IDatabase, stateManager: DefaultStateManager) {
    this.tx = db.transaction()
    this.stateManager = stateManager
    this.cache = this.tx.cache
  }

  async acquireLock(): Promise<void> {
    if (!this.checkpoint) {
      await this.stateManager.checkpoint()
      this.checkpoint = true
    }
    await this.tx.acquireLock()
  }

  async update(): Promise<void> {
    if (this.checkpoint) {
      await this.stateManager.commit()
      this.checkpoint = false
    }
    await this.tx.update()
  }

  async commit(): Promise<void> {
    if (this.checkpoint) {
      await this.stateManager.commit()
      this.checkpoint = false
    }
    await this.tx.commit()
  }

  async abort(): Promise<void> {
    if (this.checkpoint) {
      await this.stateManager.revert()
      this.checkpoint = false
    }
    await this.tx.abort()
  }

  get size(): number {
    return this.tx.size
  }

  has<Schema extends DatabaseSchema>(
    store: IDatabaseStore<Schema>,
    key: SchemaKey<Schema>,
  ): Promise<boolean> {
    return this.tx.has(store, key)
  }

  get<Schema extends DatabaseSchema>(
    store: IDatabaseStore<Schema>,
    key: SchemaKey<Schema>,
  ): Promise<SchemaValue<Schema> | undefined> {
    return this.tx.get(store, key)
  }

  put<Schema extends DatabaseSchema>(
    store: IDatabaseStore<Schema>,
    key: SchemaKey<Schema>,
    value: SchemaValue<Schema>,
  ): Promise<void> {
    return this.tx.put(store, key, value)
  }

  add<Schema extends DatabaseSchema>(
    store: IDatabaseStore<Schema>,
    key: SchemaKey<Schema>,
    value: SchemaValue<Schema>,
  ): Promise<void> {
    return this.tx.add(store, key, value)
  }

  del<Schema extends DatabaseSchema>(
    store: IDatabaseStore<Schema>,
    key: SchemaKey<Schema>,
  ): Promise<void> {
    return this.tx.del(store, key)
  }
}
