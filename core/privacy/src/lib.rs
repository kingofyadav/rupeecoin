pub mod commitment_tree;
pub mod keys;
pub mod note;
pub mod proof;
pub mod transaction;

pub use commitment_tree::{CommitmentTree, MerklePath, TREE_DEPTH};
pub use keys::{
    NullifierKey, PaymentAddress, PrivacyKeyBundle, SpendKey, ViewCiphertext, ViewKey,
    ViewPublicKey, ViewSecretKey,
};
pub use note::{EncryptedNote, Note, NoteCommitment, Nullifier};
pub use proof::{
    BalanceProof, BatchedStarkSpendProof, RangeProof, SpendEvidence, SpendProof, StarkSpendProof,
};
pub use transaction::{
    PrivacyBlockApply, PrivacyTransaction, PrivacyTxError, ShieldedInput, ShieldedOutput,
    ShieldedTxBuilder, MAX_NOTE_VALUE,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_commit_nullifier_roundtrip() {
        let bundle = PrivacyKeyBundle::generate();
        let note = Note::new(1_000_000, bundle.payment_address.clone());
        let cm = note.commitment();
        let nf = note.nullifier(&bundle.nullifier_key);

        // Same note, same commitment determinism
        assert_eq!(note.commitment(), cm);
        // Different nullifier key → different nullifier
        let bundle2 = PrivacyKeyBundle::generate();
        assert_ne!(note.nullifier(&bundle2.nullifier_key), nf);
    }

    #[test]
    fn note_encrypt_decrypt_roundtrip() {
        let bundle = PrivacyKeyBundle::generate();
        let note = Note::new(42_000_000, bundle.payment_address.clone());
        let enc = EncryptedNote::encrypt(&note, &bundle.view_key.public);
        let dec = enc
            .decrypt(&bundle.view_key.secret)
            .expect("decryption should succeed");
        assert_eq!(dec.value, note.value);
        assert_eq!(dec.recipient, note.recipient);
        assert_eq!(dec.rcm, note.rcm);
    }

    #[test]
    fn note_decrypt_wrong_key_fails() {
        let alice = PrivacyKeyBundle::generate();
        let bob = PrivacyKeyBundle::generate();
        let note = Note::new(1_000, alice.payment_address.clone());
        let enc = EncryptedNote::encrypt(&note, &alice.view_key.public);
        // Bob's secret key cannot decrypt Alice's note
        assert!(enc.decrypt(&bob.view_key.secret).is_none());
    }

    #[test]
    fn commitment_tree_and_merkle_path() {
        let bundle = PrivacyKeyBundle::generate();
        let mut tree = CommitmentTree::new();

        // Insert 5 notes
        let mut notes = Vec::new();
        for value in [100u64, 200, 300, 400, 500] {
            let note = Note::new(value, bundle.payment_address.clone());
            let cm = note.commitment();
            tree.append(cm);
            notes.push((note, cm));
        }

        let root = tree.root();

        // Verify auth path for each leaf
        for (idx, (_note, cm)) in notes.iter().enumerate() {
            let path = tree.auth_path(idx).expect("path should exist");
            assert_eq!(
                path.compute_root(cm.0),
                root,
                "path for leaf {idx} should recompute root"
            );
        }
    }

    #[test]
    fn spend_proof_verify() {
        let bundle = PrivacyKeyBundle::generate();
        let mut tree = CommitmentTree::new();
        let note = Note::new(5_000_000, bundle.payment_address.clone());
        let cm = note.commitment();
        let idx = tree.append(cm);
        let anchor = tree.root();

        let path = tree.auth_path(idx).unwrap();
        let spend_proof = SpendProof {
            note_commitment: cm,
            merkle_path: path,
            anchor,
        };
        assert!(spend_proof.verify(), "spend proof should verify");
    }

    #[test]
    fn range_proof_roundtrip() {
        let note = Note::new(123_456_789, PrivacyKeyBundle::generate().payment_address);
        let rp = RangeProof::prove(note.value, &note.rcm);
        assert!(rp.verify(note.value, &note.rcm));
        // Wrong value fails
        assert!(!rp.verify(note.value + 1, &note.rcm));
    }

    // ── Phase 11.5 output well-formedness tests ───────────────────────────────

    fn make_valid_output(value: u64, recipient: &PrivacyKeyBundle) -> ShieldedOutput {
        let note = Note::new(value, recipient.payment_address.clone());
        let cm = note.commitment();
        let enc = EncryptedNote::encrypt(&note, &recipient.view_key.public);
        ShieldedOutput::new(cm, enc, value)
    }

    #[test]
    fn output_zero_value_rejected() {
        let bob = PrivacyKeyBundle::generate();
        // Manually construct a zero-value output (bypassing builder)
        let note = Note::new(1, bob.payment_address.clone());
        let cm = note.commitment();
        let enc = EncryptedNote::encrypt(&note, &bob.view_key.public);
        let mut out = ShieldedOutput::new(cm, enc, 1);
        out.value = 0; // Force zero value
        assert!(matches!(out.validate(), Err(PrivacyTxError::InvalidOutputValue(_))));
    }

    #[test]
    fn output_value_exceeds_cap_rejected() {
        let bob = PrivacyKeyBundle::generate();
        let note = Note::new(1, bob.payment_address.clone());
        let cm = note.commitment();
        let enc = EncryptedNote::encrypt(&note, &bob.view_key.public);
        let mut out = ShieldedOutput::new(cm, enc, 1);
        out.value = MAX_NOTE_VALUE + 1;
        assert!(matches!(out.validate(), Err(PrivacyTxError::InvalidOutputValue(_))));
    }

    #[test]
    fn output_short_ciphertext_rejected() {
        let bob = PrivacyKeyBundle::generate();
        let note = Note::new(1_000, bob.payment_address.clone());
        let cm = note.commitment();
        let enc = EncryptedNote::encrypt(&note, &bob.view_key.public);
        let mut out = ShieldedOutput::new(cm, enc, 1_000);
        out.encrypted_note.ciphertext.truncate(10);
        assert!(matches!(out.validate(), Err(PrivacyTxError::MalformedEncryptedNote)));
    }

    #[test]
    fn output_valid_passes_validate() {
        let bob = PrivacyKeyBundle::generate();
        let out = make_valid_output(1_000_000, &bob);
        out.validate().expect("valid output should pass");
    }

    #[test]
    fn duplicate_commitment_in_tx_rejected() {
        let alice = PrivacyKeyBundle::generate();
        let bob = PrivacyKeyBundle::generate();

        let mut tree = CommitmentTree::new();
        let input_note = Note::new(20_000_000, alice.payment_address.clone());
        let cm = input_note.commitment();
        let idx = tree.append(cm);
        let anchor = tree.root();
        let path = tree.auth_path(idx).unwrap();

        let nullifier = input_note.nullifier(&alice.nullifier_key);
        let spend_proof = SpendProof { note_commitment: cm, merkle_path: path, anchor };

        // Create two outputs with identical commitment (same note, same rcm)
        let output_note = Note { value: 9_900_000, recipient: bob.payment_address.clone(), rcm: [7u8; 32] };
        let out_cm = output_note.commitment();
        let enc1 = EncryptedNote::encrypt(&output_note, &bob.view_key.public);
        let enc2 = EncryptedNote::encrypt(&output_note, &bob.view_key.public);

        let mut builder = ShieldedTxBuilder::new(anchor, 200_000);
        builder.add_input(ShieldedInput::new(nullifier, spend_proof, input_note.value));
        builder.add_output(ShieldedOutput::new(out_cm, enc1, output_note.value));
        builder.add_output(ShieldedOutput::new(out_cm, enc2, output_note.value));

        // build() only checks balance; validate_stateless() catches duplicates
        let tx = builder.build().unwrap();
        assert!(matches!(
            tx.validate_stateless(),
            Err(PrivacyTxError::DuplicateCommitment)
        ));
    }

    #[test]
    fn shielded_tx_builder_balance() {
        let alice = PrivacyKeyBundle::generate();
        let bob = PrivacyKeyBundle::generate();

        let mut tree = CommitmentTree::new();
        let input_note = Note::new(10_000_000, alice.payment_address.clone());
        let cm = input_note.commitment();
        let idx = tree.append(cm);
        let anchor = tree.root();
        let path = tree.auth_path(idx).unwrap();

        let nullifier = input_note.nullifier(&alice.nullifier_key);
        let spend_proof = SpendProof {
            note_commitment: cm,
            merkle_path: path,
            anchor,
        };

        let output_note = Note::new(9_900_000, bob.payment_address.clone());
        let output_cm = output_note.commitment();
        let enc = EncryptedNote::encrypt(&output_note, &bob.view_key.public);

        let mut builder = ShieldedTxBuilder::new(anchor, 100_000 /* fee */);
        builder.add_input(ShieldedInput::new(nullifier, spend_proof, input_note.value));
        builder.add_output(ShieldedOutput::new(output_cm, enc, output_note.value));

        let tx = builder.build().expect("balanced tx should build");
        tx.validate_stateless()
            .expect("stateless validation should pass");

        // Stateful: anchor must be in known roots
        let known = vec![anchor];
        let spent = std::collections::HashSet::new();
        tx.validate_stateful(&known, &spent)
            .expect("stateful validation should pass");

        // Re-spending same nullifier should fail
        let mut spent_set = std::collections::HashSet::new();
        spent_set.insert(nullifier);
        assert!(tx.validate_stateful(&known, &spent_set).is_err());
    }
}
