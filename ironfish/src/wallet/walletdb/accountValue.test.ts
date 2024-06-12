/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */
import { generateKey } from '@ironfish/rust-nodejs'
import { AccountValue, AccountValueEncoding } from './accountValue'

describe('AccountValueEncoding', () => {
  it('serializes the object into a buffer and deserializes to the original object', () => {
    const encoder = new AccountValueEncoding()

    const key = generateKey()
    const value: AccountValue = {
      id: 'id',
      name: 'foobar👁️🏃🐟',
      incomingViewKey: key.incomingViewKey,
      outgoingViewKey: key.outgoingViewKey,
      publicAddress: key.publicAddress,
      spendingKey: key.spendingKey,
      viewKey: key.viewKey,
      version: 1,
      createdAt: { sequence: 1 },
      scanningEnabled: true,
      proofAuthorizingKey: key.proofAuthorizingKey,
    }
    const buffer = encoder.serialize(value)
    const deserializedValue = encoder.deserialize(buffer)
    expect(deserializedValue).toEqual(value)
  })

  it('serializes an object with multisigKeys into a buffer and deserializes to the original object', () => {
    const encoder = new AccountValueEncoding()

    const key = generateKey()
    const value: AccountValue = {
      id: 'id',
      name: 'foobar👁️🏃🐟',
      incomingViewKey: key.incomingViewKey,
      outgoingViewKey: key.outgoingViewKey,
      publicAddress: key.publicAddress,
      // NOTE: accounts with multisigKeys should not have spendingKey
      spendingKey: null,
      viewKey: key.viewKey,
      version: 1,
      createdAt: null,
      scanningEnabled: true,
      multisigKeys: {
        publicKeyPackage: 'cccc',
        secret: 'deaf',
        keyPackage: 'beef',
      },
      proofAuthorizingKey: key.proofAuthorizingKey,
    }
    const buffer = encoder.serialize(value)
    const deserializedValue = encoder.deserialize(buffer)
    expect(deserializedValue).toEqual(value)
  })
})
