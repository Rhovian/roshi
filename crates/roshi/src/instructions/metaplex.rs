//! Hand-rolled Metaplex Token Metadata CPI data for the share-mint metadata
//! instruction. Display only — no economic invariant may depend on metadata —
//! so the dependency is two borsh-encoded instruction layouts rather than the
//! full `mpl-token-metadata` crate.

use solana_pubkey::{pubkey, Pubkey};

/// Metaplex Token Metadata program id (vetted constant, same pattern as the
/// Pyth receiver id).
pub(crate) const TOKEN_METADATA_PROGRAM_ID: Pubkey =
    pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");

const METADATA_SEED: &[u8] = b"metadata";

/// `CreateMetadataAccountV3` instruction discriminator.
const CREATE_METADATA_ACCOUNT_V3: u8 = 33;
/// `UpdateMetadataAccountV2` instruction discriminator.
const UPDATE_METADATA_ACCOUNT_V2: u8 = 15;

/// The canonical metadata PDA for `mint` under the Token Metadata program.
pub(crate) fn find_metadata_address(mint: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            METADATA_SEED,
            TOKEN_METADATA_PROGRAM_ID.as_ref(),
            mint.as_ref(),
        ],
        &TOKEN_METADATA_PROGRAM_ID,
    )
}

/// Borsh-encode a Metaplex `DataV2` carrying only display fields: zero seller
/// fee, no creators, no collection, no uses. Length limits (name <= 32,
/// symbol <= 10, uri <= 200) are enforced by the Metaplex program.
fn write_data_v2(data: &mut Vec<u8>, name: &str, symbol: &str, uri: &str) {
    for field in [name, symbol, uri] {
        data.extend_from_slice(&(field.len() as u32).to_le_bytes());
        data.extend_from_slice(field.as_bytes());
    }
    data.extend_from_slice(&0u16.to_le_bytes()); // seller_fee_basis_points
    data.push(0); // creators: None
    data.push(0); // collection: None
    data.push(0); // uses: None
}

/// `CreateMetadataAccountV3 { data, is_mutable: true, collection_details: None }`.
pub(crate) fn create_metadata_v3_data(name: &str, symbol: &str, uri: &str) -> Vec<u8> {
    let mut data = vec![CREATE_METADATA_ACCOUNT_V3];
    write_data_v2(&mut data, name, symbol, uri);
    data.push(1); // is_mutable: renames go through this same instruction
    data.push(0); // collection_details: None
    data
}

/// `UpdateMetadataAccountV2 { data: Some(..), update_authority: None,
/// primary_sale_happened: None, is_mutable: None }`.
pub(crate) fn update_metadata_v2_data(name: &str, symbol: &str, uri: &str) -> Vec<u8> {
    let mut data = vec![UPDATE_METADATA_ACCOUNT_V2];
    data.push(1); // data: Some(DataV2)
    write_data_v2(&mut data, name, symbol, uri);
    data.push(0); // new update_authority: None (stays the vault PDA)
    data.push(0); // primary_sale_happened: None
    data.push(0); // is_mutable: None
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_data_encodes_display_fields_only() {
        let data = create_metadata_v3_data("Roshi USDC", "rUSDC", "https://x/y.json");

        assert_eq!(data[0], CREATE_METADATA_ACCOUNT_V3);
        assert_eq!(&data[1..5], &10u32.to_le_bytes());
        assert_eq!(&data[5..15], b"Roshi USDC");
        // Tail: sfbp 0, creators None, collection None, uses None,
        // is_mutable true, collection_details None.
        assert_eq!(&data[data.len() - 7..], &[0, 0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn update_data_wraps_data_v2_in_some_and_changes_nothing_else() {
        let data = update_metadata_v2_data("Roshi USDC", "rUSDC", "https://x/y.json");

        assert_eq!(data[0], UPDATE_METADATA_ACCOUNT_V2);
        assert_eq!(data[1], 1); // Some(DataV2)
        assert_eq!(&data[2..6], &10u32.to_le_bytes());
        // Tail: update_authority None, primary_sale None, is_mutable None.
        assert_eq!(&data[data.len() - 3..], &[0, 0, 0]);
    }
}
