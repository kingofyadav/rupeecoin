/// Wallet scanner: trial-decrypt all shielded outputs and collect owned notes.
use rupeecoin_privacy::{Note, NoteCommitment, Nullifier, ViewSecretKey};

use crate::node_rpc::{NodeRpc, RpcClientError, ScannableOutput};

/// An owned (decrypted) note with its position in the commitment tree.
pub struct OwnedNote {
    pub index: u64,
    pub note: Note,
    pub commitment: NoteCommitment,
    pub nullifier: Nullifier,
}

/// Scan the node's shielded pool and return all notes owned by this view key.
///
/// `nk` is needed to compute nullifiers so we can detect spent notes.
pub fn scan(
    rpc: &NodeRpc,
    vsk: &ViewSecretKey,
    nk: &rupeecoin_privacy::NullifierKey,
) -> Result<Vec<OwnedNote>, RpcClientError> {
    // Fetch commitment count so we know when to stop paging.
    let (_roots, total) = rpc.get_recent_roots()?;

    let mut owned = Vec::new();
    let page_size = 200u64;
    let mut start = 0u64;

    while start < total {
        let outputs = rpc.get_shielded_outputs(start, page_size)?;
        if outputs.is_empty() {
            break;
        }
        for ScannableOutput {
            index,
            commitment,
            encrypted_note,
        } in outputs
        {
            if let Some(note) = encrypted_note.decrypt(vsk) {
                // Verify commitment matches (guards against a malicious node).
                if note.commitment() == commitment {
                    let nullifier = note.nullifier(nk);
                    // Skip notes already spent on-chain; skip with warning on check failure.
                    match rpc.is_nullifier_spent(&nullifier) {
                        Ok(true) => continue,
                        Ok(false) => {}
                        Err(e) => {
                            eprintln!("warning: nullifier check failed for note {index}: {e}; skipping");
                            continue;
                        }
                    }
                    owned.push(OwnedNote {
                        index,
                        note,
                        commitment,
                        nullifier,
                    });
                }
            }
        }
        start += page_size;
    }

    Ok(owned)
}

/// From a list of owned notes, select enough to cover `target_micro_rpc`.
///
/// Notes are selected ascending by value (smallest first) to minimise note
/// count.  Returns the selected notes and the change amount (in micro-RPC).
pub fn select_notes(
    notes: &[OwnedNote],
    target_micro_rpc: u64,
    fee_micro_rpc: u64,
) -> Option<(&[OwnedNote], u64)> {
    let need = target_micro_rpc.checked_add(fee_micro_rpc)?;
    let mut total = 0u64;
    let mut end = 0;
    // notes must already be sorted ascending; caller is responsible.
    for note in notes {
        if total >= need {
            break;
        }
        total = total.checked_add(note.note.value)?;
        end += 1;
    }
    if total >= need {
        let change = total - need;
        Some((&notes[..end], change))
    } else {
        None
    }
}
