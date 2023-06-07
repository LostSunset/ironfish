/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/* eslint-disable no-console */
import { createNodeTest, useAccountFixture, useBlockWithTxs } from '../../testUtilities'
import { BenchUtils } from '../../utils'
import { WorkerPool } from '../pool'
import { DecryptNoteOptions } from './decryptNotes'

describe('DecryptNotes job', () => {
  const jestConsole = console

  beforeAll(() => {
    global.console = require('console')
  })

  afterAll(() => {
    global.console = jestConsole
  })
  const nodeTest = createNodeTest(true)

  const TRANSACTIONS = 50

  const NOTES = [20, 100]
  const CAN_DECRYPT_AS_OWNER = [true, false]
  const TRY_DECRYPT_AS_SPENDER = [true, false]

  it('decryptsNotes', async () => {
    const account = await useAccountFixture(nodeTest.wallet, 'account')
    const account2 = await useAccountFixture(nodeTest.wallet, 'account2')

    const { block, transactions } = await useBlockWithTxs(nodeTest.node, TRANSACTIONS, account)
    await expect(nodeTest.chain).toAddBlock(block)

    const payload1: DecryptNoteOptions = {
      incomingViewKey: account.incomingViewKey,
      outgoingViewKey: account.outgoingViewKey,
      viewKey: account.viewKey,
      decryptForSpender: true,
      notes: [],
    }
    const payload2: DecryptNoteOptions = {
      incomingViewKey: account2.incomingViewKey,
      outgoingViewKey: account2.outgoingViewKey,
      viewKey: account2.viewKey,
      decryptForSpender: true,
      notes: [],
    }
    const notesToDecrypt: DecryptNoteOptions['notes'] = []
    let i = 0
    for (const transaction of transactions) {
      for (const note of transaction.notes) {
        notesToDecrypt.push({
          serializedNote: note.serialize(),
          currentNoteIndex: i++,
        })
      }
    }

    // Generate test permutations
    const TESTS: {
      notes: number
      canDecryptAsOwner: boolean
      tryDecryptForSpender: boolean
    }[] = []
    for (const notes of NOTES) {
      for (const canDecryptAsOwner of CAN_DECRYPT_AS_OWNER) {
        for (const tryDecryptForSpender of TRY_DECRYPT_AS_SPENDER) {
          TESTS.push({
            notes,
            canDecryptAsOwner,
            tryDecryptForSpender,
          })
        }
      }
    }

    for (const test of TESTS) {
      const payload = test.canDecryptAsOwner ? payload1 : payload2
      payload.decryptForSpender = test.tryDecryptForSpender
      payload.notes = notesToDecrypt.slice(0, test.notes)

      const tsPool = new WorkerPool({ numWorkers: 1 })
      tsPool.start()

      const results = await BenchUtils.withSegmentIterations(10, 100, async () => {
        const _x = await tsPool.decryptNotes(payload)
      })

      const title = `[DecryptNotes: notes: ${
        test.notes
      }, canDecrypt: ${test.canDecryptAsOwner.toString()}, decryptForSpender: ${test.tryDecryptForSpender.toString()}]`
      console.log(BenchUtils.renderSegmentAggregate(results, title))

      await tsPool.stop()
    }
  })
})