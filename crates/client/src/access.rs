//! Client helpers for vault access allowlists.

use roshi_interface::access::{
    access_merkle_leaf, access_merkle_node, EMPTY_ACCESS_MERKLE_ROOT, MAX_ACCESS_PROOF_LEN,
};
use solana_pubkey::Pubkey;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessMerkleTree {
    entries: Vec<AccessMerkleEntry>,
    layers: Vec<Vec<[u8; 32]>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AccessMerkleEntry {
    owner: Pubkey,
    leaf: [u8; 32],
}

impl AccessMerkleTree {
    pub fn new(owners: impl IntoIterator<Item = Pubkey>) -> Self {
        let mut entries = owners
            .into_iter()
            .map(|owner| AccessMerkleEntry {
                owner,
                leaf: access_merkle_leaf(&owner),
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| {
            left.leaf
                .cmp(&right.leaf)
                .then_with(|| left.owner.to_bytes().cmp(&right.owner.to_bytes()))
        });
        entries.dedup_by(|left, right| left.owner == right.owner);

        let mut layers = Vec::new();
        if !entries.is_empty() {
            layers.push(entries.iter().map(|entry| entry.leaf).collect::<Vec<_>>());

            while layers.last().is_some_and(|layer| layer.len() > 1) {
                let current = layers.last().expect("layer exists");
                let mut next = Vec::with_capacity(current.len().div_ceil(2));

                for pair in current.chunks(2) {
                    if let [left, right] = pair {
                        next.push(access_merkle_node(left, right));
                    } else {
                        next.push(pair[0]);
                    }
                }

                layers.push(next);
            }
        }

        Self { entries, layers }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn root(&self) -> [u8; 32] {
        self.layers
            .last()
            .and_then(|layer| layer.first())
            .copied()
            .unwrap_or(EMPTY_ACCESS_MERKLE_ROOT)
    }

    pub fn proof(&self, owner: &Pubkey) -> Option<Vec<[u8; 32]>> {
        let mut index = self
            .entries
            .iter()
            .position(|entry| entry.owner == *owner)?;
        let mut proof = Vec::new();

        for layer in &self.layers {
            if layer.len() <= 1 {
                break;
            }

            let sibling_index = if index % 2 == 0 {
                index + 1
            } else {
                index.saturating_sub(1)
            };

            if let Some(sibling) = layer.get(sibling_index) {
                proof.push(*sibling);
            }

            index /= 2;
        }

        if proof.len() <= MAX_ACCESS_PROOF_LEN {
            Some(proof)
        } else {
            None
        }
    }

    pub fn contains(&self, owner: &Pubkey) -> bool {
        self.entries.iter().any(|entry| entry.owner == *owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roshi_interface::access::verify_access_merkle_proof;

    #[test]
    fn empty_tree_uses_closed_root_and_has_no_proofs() {
        let tree = AccessMerkleTree::new(Vec::<Pubkey>::new());

        assert!(tree.is_empty());
        assert_eq!(tree.root(), EMPTY_ACCESS_MERKLE_ROOT);
        assert_eq!(tree.proof(&Pubkey::new_unique()), None);
    }

    #[test]
    fn builds_verifiable_proofs() {
        let owners = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let tree = AccessMerkleTree::new(owners);
        let root = tree.root();

        for owner in owners {
            let proof = tree.proof(&owner).unwrap();
            assert!(verify_access_merkle_proof(&owner, &root, &proof));
        }
    }

    #[test]
    fn root_is_stable_across_input_order() {
        let owners = [
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        ];
        let mut reversed = owners;
        reversed.reverse();

        assert_eq!(
            AccessMerkleTree::new(owners).root(),
            AccessMerkleTree::new(reversed).root()
        );
    }

    #[test]
    fn duplicate_owners_do_not_change_root() {
        let owner = Pubkey::new_unique();
        let other = Pubkey::new_unique();

        assert_eq!(
            AccessMerkleTree::new([owner, other]).root(),
            AccessMerkleTree::new([owner, other, owner]).root()
        );
    }

    #[test]
    fn missing_owner_has_no_proof() {
        let tree = AccessMerkleTree::new([Pubkey::new_unique()]);

        assert_eq!(tree.proof(&Pubkey::new_unique()), None);
    }
}
