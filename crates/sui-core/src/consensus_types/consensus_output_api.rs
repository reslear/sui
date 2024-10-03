// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0
use std::{cmp::Ordering, fmt::Display};

use consensus_core::{BlockAPI, CommitDigest, TransactionIndex, VerifiedBlock};
use sui_protocol_config::ProtocolConfig;
use sui_types::{
    digests::ConsensusCommitDigest,
    messages_consensus::{AuthorityIndex, ConsensusTransaction, Round},
};

pub(crate) struct ParsedTransaction {
    // Transaction from consensus output.
    pub(crate) transaction: ConsensusTransaction,
    // Whether the transaction was rejected in voting.
    pub(crate) rejected: bool,
    // Bytes length of the serialized transaction
    pub(crate) serialized_len: usize,
    // Consensus round of the block containing the transaction.
    #[allow(unused)]
    pub(crate) round: Round,
    // Authority index of the block containing the transaction.
    #[allow(unused)]
    pub(crate) authority: AuthorityIndex,
    // Transaction index in the block.
    #[allow(unused)]
    pub(crate) transaction_index: TransactionIndex,
}

pub(crate) trait ConsensusCommitAPI: Display {
    fn reputation_score_sorted_desc(&self) -> Option<Vec<(AuthorityIndex, u64)>>;
    fn leader_round(&self) -> u64;
    fn leader_author_index(&self) -> AuthorityIndex;

    /// Returns epoch UNIX timestamp in milliseconds
    fn commit_timestamp_ms(&self) -> u64;

    /// Returns a unique global index for each committed sub-dag.
    fn commit_sub_dag_index(&self) -> u64;

    /// Returns all accepted and rejected transactions per block in the commit in deterministic order.
    fn transactions(&self) -> Vec<(AuthorityIndex, Vec<ParsedTransaction>)>;

    /// Returns the digest of consensus output.
    fn consensus_digest(&self, protocol_config: &ProtocolConfig) -> ConsensusCommitDigest;
}

impl ConsensusCommitAPI for consensus_core::CommittedSubDag {
    fn reputation_score_sorted_desc(&self) -> Option<Vec<(AuthorityIndex, u64)>> {
        if !self.reputation_scores_desc.is_empty() {
            Some(
                self.reputation_scores_desc
                    .iter()
                    .map(|(id, score)| (id.value() as AuthorityIndex, *score))
                    .collect(),
            )
        } else {
            None
        }
    }

    fn leader_round(&self) -> u64 {
        self.leader.round as u64
    }

    fn leader_author_index(&self) -> AuthorityIndex {
        self.leader.author.value() as AuthorityIndex
    }

    fn commit_timestamp_ms(&self) -> u64 {
        // TODO: Enforce ordered timestamp in Mysticeti.
        self.timestamp_ms
    }

    fn commit_sub_dag_index(&self) -> u64 {
        self.commit_ref.index.into()
    }

    fn transactions(&self) -> Vec<(AuthorityIndex, Vec<ParsedTransaction>)> {
        self.blocks
            .iter()
            .zip(self.rejected_transactions.iter())
            .map(|(b, r)| {
                (
                    b.author().value() as AuthorityIndex,
                    parse_block_transactions(b, r),
                )
            })
            .collect()
    }

    fn consensus_digest(&self, protocol_config: &ProtocolConfig) -> ConsensusCommitDigest {
        if protocol_config.mysticeti_use_committed_subdag_digest() {
            // We port CommitDigest, a consensus space object, into ConsensusCommitDigest, a sui-core space object.
            // We assume they always have the same format.
            static_assertions::assert_eq_size!(ConsensusCommitDigest, CommitDigest);
            ConsensusCommitDigest::new(self.commit_ref.digest.into_inner())
        } else {
            ConsensusCommitDigest::default()
        }
    }
}

pub(crate) fn parse_block_transactions(
    block: &VerifiedBlock,
    rejected_transactions: &[TransactionIndex],
) -> Vec<ParsedTransaction> {
    let round = block.round();
    let authority = block.author().value() as AuthorityIndex;

    let mut rejected_i = 0;
    block
        .transactions()
        .iter().enumerate()
        .map(|(index, tx)| {
            let transaction = match bcs::from_bytes::<ConsensusTransaction>(tx.data()) {
                Ok(transaction) => transaction,
                Err(err) => {
                    panic!("Failed to deserialize sequenced consensus transaction(this should not happen) {} from {authority} at {round}", err);
                },
            };
            let rejected = if rejected_i < rejected_transactions.len() {
                match rejected_transactions[rejected_i].cmp(&(index as TransactionIndex)) {
                    Ordering::Less => {
                        panic!("Rejected transaction indices are not in order. Block {:?}, rejected transactions: {:?}", block, rejected_transactions);
                    },
                    Ordering::Equal => {
                        rejected_i += 1;
                        true
                    },
                    Ordering::Greater => {
                        false
                    },
                }
            } else {
                false
            };
            ParsedTransaction {
                transaction,
                rejected,
                serialized_len: tx.data().len(),
                round,
                authority,
                transaction_index: index as TransactionIndex,
            }
        })
        .collect()
}
