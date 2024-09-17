/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
import { Address, bytesToHex, hexToBytes } from '@ethereumjs/util'
import * as yup from 'yup'
import { Assert } from '../../../assert'
import { FullNode } from '../../../node'
import { registerEthRoute } from '../eth/ethRouter'
import { ApiNamespace } from '../namespaces'
import { ethBlockRefToBlock } from './util'

export type GetCodeRequest = [string, string]

export const GetCodeRequestSchema: yup.MixedSchema<GetCodeRequest> = yup
  .mixed<[string, string]>()
  .defined()

export type GetCodeResponse = string

export const GetCodeResponseSchema: yup.StringSchema<GetCodeResponse> = yup.string().defined()

registerEthRoute<typeof GetCodeRequestSchema, GetCodeResponse>(
  `eth_getCode`,
  `${ApiNamespace.eth}/getCode`,
  GetCodeRequestSchema,
  async (request, node): Promise<void> => {
    Assert.isInstanceOf(node, FullNode)

    const [addressHex, blockRef] = request.data

    // TODO handle pending transactions
    const block = await ethBlockRefToBlock(blockRef, node.chain)

    const stateManager = await node.chain.blockchainDb.stateManager.withStateRoot(
      block?.header.stateCommitment,
    )
    const contractAddress = new Address(hexToBytes(addressHex))
    const code = bytesToHex(await stateManager.getContractCode(contractAddress))
    request.end(code)
  },
)
