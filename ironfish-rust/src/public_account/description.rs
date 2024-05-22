/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::io;

use crate::{
    errors::{IronfishError, IronfishErrorKind},
    PublicAddress,
};

use super::transfer::Transfer;

#[derive(Clone)]
pub struct PublicAccountDescription {
    /// TODO(jwp): do we need a separate account address for later migration
    /// pub(crate) account_address: EthereumAddress
    version: u8,
    // Minimum number of signers required to sign a message and be valid
    min_signers: u16,
    // Signers of the public account
    signers: Vec<VerifyingKey>,
    // Signatures of the signers for a given message
    signatures: Vec<Signature>,
    // asset transfers
    transfers: Vec<Transfer>,
    // address of the account
    address: PublicAddress,
}

impl PublicAccountDescription {
    pub fn new(
        version: u8,
        min_signers: u16,
        signers: Vec<VerifyingKey>,
        signatures: Vec<Signature>,
        transfers: Vec<Transfer>,
        address: PublicAddress,
    ) -> Result<PublicAccountDescription, IronfishError> {
        let description = Self {
            version,
            min_signers,
            signers,
            signatures,
            address,
            transfers,
        };

        description.valid()?;

        Ok(description)
    }
    pub fn read<R: io::Read>(mut reader: R) -> Result<Self, IronfishError> {
        let mut version_buf = [0; 1];
        reader.read_exact(&mut version_buf)?;
        let version = version_buf[0];

        let mut min_signers_buf = [0; 2];
        reader.read_exact(&mut min_signers_buf)?;
        let min_signers = u16::from_le_bytes(min_signers_buf);

        let mut signers_len_buf = [0; 2];
        reader.read_exact(&mut signers_len_buf)?;
        let signers_len = u16::from_le_bytes(signers_len_buf) as usize;

        let mut signers = Vec::with_capacity(signers_len);
        for _ in 0..signers_len {
            let mut signer_buf = [0; 32];
            reader.read_exact(&mut signer_buf)?;
            let signer = VerifyingKey::from_bytes(&signer_buf)
                .map_err(|_| IronfishError::new(IronfishErrorKind::InvalidData))?;
            signers.push(signer);
        }

        let mut signatures_len_buf = [0; 2];
        reader.read_exact(&mut signatures_len_buf)?;
        let signatures_len = u16::from_le_bytes(signatures_len_buf) as usize;

        let mut signatures = Vec::with_capacity(signatures_len);
        for _ in 0..signatures_len {
            let mut signature_buf = [0; 64];
            reader.read_exact(&mut signature_buf)?;
            let signature = Signature::from_bytes(&signature_buf);
            signatures.push(signature);
        }

        let mut transfers_len_buf = [0; 2];
        reader.read_exact(&mut transfers_len_buf)?;
        let transfers_len = u16::from_le_bytes(transfers_len_buf) as usize;

        let mut transfers = Vec::with_capacity(transfers_len);
        for _ in 0..transfers_len {
            let transfer = Transfer::read(&mut reader)?;
            transfers.push(transfer);
        }
        let address = PublicAddress::read(&mut reader)?;

        let description = Self {
            version,
            min_signers,
            signers,
            signatures,
            address,
            transfers,
        };
        description.valid()?;
        Ok(description)
    }

    pub fn write<W: io::Write>(&self, mut writer: W) -> Result<(), IronfishError> {
        // TODO: think about ideal ordering here
        writer.write_all(&self.version.to_le_bytes())?;

        writer.write_all(&self.min_signers.to_le_bytes())?;

        let signers_len: u16 = self.signers.len().try_into()?;
        writer.write_all(&signers_len.to_le_bytes())?;

        for signer in &self.signers {
            writer.write_all(signer.as_bytes())?;
        }

        let signatures_len: u16 = self.signatures.len().try_into()?;
        writer.write_all(&signatures_len.to_le_bytes())?;

        for signature in &self.signatures {
            writer.write_all(&signature.to_bytes())?;
        }

        let transfers_len: u16 = self.transfers.len().try_into()?;
        writer.write_all(&transfers_len.to_le_bytes())?;

        for transfer in &self.transfers {
            transfer.write(&mut writer)?;
        }

        self.address.write(&mut writer)?;

        Ok(())
    }

    fn valid(&self) -> Result<(), IronfishError> {
        if self.min_signers < 1 {
            return Err(IronfishError::new(IronfishErrorKind::InvalidThreshold));
        }

        if self.signers.len() < self.min_signers as usize {
            return Err(IronfishError::new(IronfishErrorKind::InvalidThreshold));
        }

        if self.signatures.len() < self.min_signers as usize {
            return Err(IronfishError::new(IronfishErrorKind::InvalidData));
        }

        let unique_signers: Vec<_> = self.signers.iter().collect();
        if unique_signers.len() != self.signers.len() {
            return Err(IronfishError::new(IronfishErrorKind::DuplicateSigner));
        }
    
        let unique_signatures: Vec<_> = self.signatures.iter().collect();
        if unique_signatures.len() != self.signatures.len() {
            return Err(IronfishError::new(IronfishErrorKind::DuplicateSignature));
        }

        Ok(())
    }

    pub fn verify(&self) -> Result<(), IronfishError> {
        self.valid()?;

        let hash = &PublicAccountDescription::hash(
            &self.version,
            &self.min_signers,
            &self.signers,
            &self.address,
            &self.transfers,
        )?;
        for signature in &self.signatures {
            let is_valid = self.signers.iter().any(|signer| signer.verify(hash, signature).is_ok());
    
            if !is_valid {
                return Err(IronfishError::new(IronfishErrorKind::InvalidSignature));
            }
        }
        Ok(())
    }

    pub fn hash(
        version: &u8,
        threshold: &u16,
        signers: &Vec<VerifyingKey>,
        address: &PublicAddress,
        transfers: &Vec<Transfer>,
    ) -> Result<[u8; 32], IronfishError> {
        // TODO(jwp): verify which hashers supported by axelar
        let mut hasher = blake3::Hasher::new();
        hasher.update(&version.to_le_bytes());
        hasher.update(&threshold.to_le_bytes());
        for signer in signers {
            hasher.update(signer.as_bytes());
        }
        for transfer in transfers {
            hasher.update(&transfer.as_bytes()?);
        }
        hasher.update(&address.public_address());
        Ok(hasher.finalize().into())
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn min_signers(&self) -> u16 {
        self.min_signers
    }

    pub fn signers(&self) -> &Vec<VerifyingKey> {
        &self.signers
    }

    pub fn signatures(&self) -> &Vec<Signature> {
        &self.signatures
    }

    pub fn transfers(&self) -> &Vec<Transfer> {
        &self.transfers
    }

    pub fn address(&self) -> &PublicAddress {
        &self.address
    }
}

#[cfg(test)]
mod tests {
    use crate::{assets::asset_identifier, public_account::transfer::PublicMemo, SaplingKey};

    use super::*;
    use ed25519_dalek::{ed25519::signature::SignerMut, SigningKey};
    use rand::rngs::OsRng;

    #[test]
    fn test_public_account_create_description() {
        let mut csprng = OsRng {};
        let key = SaplingKey::generate_key();
        let public_address = key.public_address();
        let mut signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let transfer = Transfer {
            asset_id: asset_identifier::NATIVE_ASSET,
            amount: 100,
            to: public_address,
            memo: PublicMemo([0; 256]),
        };
        let hash = PublicAccountDescription::hash(
            &1,
            &1,
            &vec![verifying_key],
            &public_address,
            &vec![transfer],
        )
        .expect("Should successfully hash");
        let signature = signing_key.sign(&hash);

        let original = PublicAccountDescription::new(
            1,
            1,
            vec![verifying_key],
            vec![signature],
            vec![transfer],
            public_address,
        )
        .expect("Should successfully create description");

        original
            .verify()
            .expect("Should be valid/verified creation");

        let mut buffer = Vec::new();
        original.write(&mut buffer).unwrap();

        let read = PublicAccountDescription::read(&buffer[..]).unwrap();

        assert_eq!(original.min_signers, read.min_signers);
        assert_eq!(original.signers, read.signers);
        assert_eq!(original.signatures, read.signatures);
    }
}
