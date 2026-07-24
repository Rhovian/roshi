use solana_program_error::ProgramError;
use wincode::{SchemaRead, SchemaWrite};

/// Requested signer and writable privileges for one relayed CPI account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountFlags {
    pub is_signer: bool,
    pub is_writable: bool,
}

/// Packed requested privileges for a relayed CPI account slice.
///
/// Account `i` occupies bits `((i % 4) * 2)..((i % 4) * 2 + 2)` in
/// `bits[i / 4]`: bit 0 requests signer and bit 1 requests writable access.
/// The byte vector is canonical: it holds exactly enough bytes for
/// `accounts_len`, and every unused high bit in its last byte is zero.
#[derive(Clone, Debug, Eq, PartialEq, codama_macros::CodamaType, SchemaWrite, SchemaRead)]
pub struct PackedAccountFlags {
    pub accounts_len: u8,
    pub bits: Vec<u8>,
}

impl PackedAccountFlags {
    /// Pack runtime flags into their canonical wire representation.
    pub fn from_flags(flags: &[AccountFlags]) -> Self {
        assert!(
            flags.len() <= usize::from(u8::MAX),
            "a relayed CPI supports at most 255 account flags"
        );

        let mut bits = vec![0; flags.len().div_ceil(4)];
        for (index, flags) in flags.iter().enumerate() {
            let value = u8::from(flags.is_signer) | (u8::from(flags.is_writable) << 1);
            bits[index / 4] |= value << ((index % 4) * 2);
        }

        Self {
            accounts_len: flags.len() as u8,
            bits,
        }
    }

    /// Decode canonical packed flags.
    pub fn iter(&self) -> Result<impl Iterator<Item = AccountFlags> + '_, ProgramError> {
        let expected_len = usize::from(self.accounts_len).div_ceil(4);
        if self.bits.len() != expected_len {
            return Err(ProgramError::InvalidInstructionData);
        }

        let used_bits = (self.accounts_len % 4) * 2;
        if used_bits != 0 && self.bits[expected_len - 1] >> used_bits != 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok((0..usize::from(self.accounts_len)).map(|index| {
            let value = self.bits[index / 4] >> ((index % 4) * 2);
            AccountFlags {
                is_signer: value & 1 != 0,
                is_writable: value & 2 != 0,
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags() -> [AccountFlags; 3] {
        [
            AccountFlags {
                is_signer: true,
                is_writable: false,
            },
            AccountFlags {
                is_signer: false,
                is_writable: true,
            },
            AccountFlags {
                is_signer: true,
                is_writable: true,
            },
        ]
    }

    #[test]
    fn round_trips_canonical_flags() {
        let flags = flags();
        let packed = PackedAccountFlags::from_flags(&flags);
        let encoded = wincode::serialize(&packed).unwrap();
        let decoded: PackedAccountFlags = wincode::deserialize(&encoded).unwrap();

        assert_eq!(packed.bits, vec![0b00_11_10_01]);
        assert_eq!(decoded, packed);
        assert_eq!(decoded.iter().unwrap().collect::<Vec<_>>(), flags);
    }

    #[test]
    fn round_trips_flags_across_bytes() {
        let flags = [
            AccountFlags {
                is_signer: false,
                is_writable: false,
            },
            AccountFlags {
                is_signer: true,
                is_writable: false,
            },
            AccountFlags {
                is_signer: false,
                is_writable: true,
            },
            AccountFlags {
                is_signer: true,
                is_writable: true,
            },
            AccountFlags {
                is_signer: true,
                is_writable: true,
            },
            AccountFlags {
                is_signer: false,
                is_writable: true,
            },
        ];

        let packed = PackedAccountFlags::from_flags(&flags);

        assert_eq!(packed.bits, vec![0b11_10_01_00, 0b0000_1011]);
        assert_eq!(packed.iter().unwrap().collect::<Vec<_>>(), flags);
    }

    #[test]
    fn empty_flags_are_canonical() {
        let packed = PackedAccountFlags::from_flags(&[]);

        assert_eq!(packed.accounts_len, 0);
        assert!(packed.bits.is_empty());
        assert!(packed.iter().unwrap().next().is_none());
    }

    #[test]
    fn rejects_short_bits() {
        let packed = PackedAccountFlags {
            accounts_len: 1,
            bits: vec![],
        };

        assert!(matches!(
            packed.iter(),
            Err(ProgramError::InvalidInstructionData)
        ));
    }

    #[test]
    fn rejects_long_bits() {
        let packed = PackedAccountFlags {
            accounts_len: 1,
            bits: vec![0, 0],
        };

        assert!(matches!(
            packed.iter(),
            Err(ProgramError::InvalidInstructionData)
        ));
    }

    #[test]
    fn rejects_nonzero_pad_bits() {
        let packed = PackedAccountFlags {
            accounts_len: 3,
            bits: vec![0b0100_0000],
        };

        assert!(matches!(
            packed.iter(),
            Err(ProgramError::InvalidInstructionData)
        ));
    }

    #[test]
    fn rejects_nonzero_pad_bits_with_one_account() {
        let packed = PackedAccountFlags {
            accounts_len: 1,
            bits: vec![0b0000_0100],
        };

        assert!(matches!(
            packed.iter(),
            Err(ProgramError::InvalidInstructionData)
        ));
    }
}
