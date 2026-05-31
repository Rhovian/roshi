//! Vault access-list helpers.

use solana_pubkey::Pubkey;
use solana_sha256_hasher::hashv;

pub const ACCESS_LEAF_DOMAIN: &[u8] = b"roshi:vault-access:leaf:v1";
pub const ACCESS_NODE_DOMAIN: &[u8] = b"roshi:vault-access:node:v1";
pub const EMPTY_ACCESS_MERKLE_ROOT: [u8; 32] = [0; 32];
pub const MAX_ACCESS_PROOF_LEN: usize = 32;

pub fn access_merkle_leaf(owner: &Pubkey) -> [u8; 32] {
    hashv(&[ACCESS_LEAF_DOMAIN, owner.as_ref()]).to_bytes()
}

pub fn access_merkle_node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let (first, second) = if left <= right {
        (left.as_slice(), right.as_slice())
    } else {
        (right.as_slice(), left.as_slice())
    };

    hashv(&[ACCESS_NODE_DOMAIN, first, second]).to_bytes()
}

pub fn verify_access_merkle_proof(
    owner: &Pubkey,
    merkle_root: &[u8; 32],
    proof: &[[u8; 32]],
) -> bool {
    if proof.len() > MAX_ACCESS_PROOF_LEN {
        return false;
    }

    let mut node = access_merkle_leaf(owner);

    for sibling in proof {
        node = access_merkle_node(&node, sibling);
    }

    &node == merkle_root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_single_leaf_root() {
        let owner = Pubkey::new_unique();
        let root = access_merkle_leaf(&owner);

        assert!(verify_access_merkle_proof(&owner, &root, &[]));
    }

    #[test]
    fn verifies_directionless_two_leaf_tree() {
        let left = Pubkey::new_unique();
        let right = Pubkey::new_unique();
        let left_leaf = access_merkle_leaf(&left);
        let right_leaf = access_merkle_leaf(&right);
        let root = access_merkle_node(&left_leaf, &right_leaf);

        assert!(verify_access_merkle_proof(&left, &root, &[right_leaf]));
        assert!(verify_access_merkle_proof(&right, &root, &[left_leaf]));
    }

    #[test]
    fn verifies_four_leaf_tree() {
        let owners = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let leaves = owners.map(|owner| access_merkle_leaf(&owner));
        let left_node = access_merkle_node(&leaves[0], &leaves[1]);
        let right_node = access_merkle_node(&leaves[2], &leaves[3]);
        let root = access_merkle_node(&left_node, &right_node);

        assert!(verify_access_merkle_proof(
            &owners[2],
            &root,
            &[leaves[3], left_node],
        ));
        assert!(verify_access_merkle_proof(
            &owners[0],
            &root,
            &[leaves[1], right_node],
        ));
    }

    #[test]
    fn rejects_wrong_owner_or_proof() {
        let owner = Pubkey::new_unique();
        let other = Pubkey::new_unique();
        let sibling = access_merkle_leaf(&Pubkey::new_unique());
        let root = access_merkle_node(&access_merkle_leaf(&owner), &sibling);

        assert!(!verify_access_merkle_proof(&other, &root, &[sibling]));
        assert!(!verify_access_merkle_proof(&owner, &root, &[]));
    }

    #[test]
    fn rejects_oversized_proof() {
        let owner = Pubkey::new_unique();
        let root = access_merkle_leaf(&owner);
        let oversized_proof = vec![[0; 32]; MAX_ACCESS_PROOF_LEN + 1];

        assert!(!verify_access_merkle_proof(&owner, &root, &oversized_proof));
    }
}
