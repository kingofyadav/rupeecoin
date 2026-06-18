/// Lightweight assembler for RVM bytecode.
///
/// Labels are strings; forward references are patched at `build()` time.
/// Jump targets are encoded as PUSH8 <8-byte address> so the code is always
/// position-independent after patching.
use std::collections::HashMap;

use crate::opcode::Op;

pub struct Assembler {
    code: Vec<u8>,
    /// label name → byte offset in `code` where the JUMPDEST lives
    labels: HashMap<String, u64>,
    /// (patch_offset, label) — the 8 bytes starting at `patch_offset` hold
    /// a placeholder that must be replaced with the resolved label address
    patches: Vec<(usize, String)>,
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Assembler {
    pub fn new() -> Self {
        Assembler {
            code: Vec::new(),
            labels: HashMap::new(),
            patches: Vec::new(),
        }
    }

    /// Emit a raw opcode byte.
    pub fn op(&mut self, op: Op) -> &mut Self {
        self.code.push(op as u8);
        self
    }

    /// Emit PUSH8 followed by an 8-byte big-endian value.
    pub fn push8(&mut self, val: u64) -> &mut Self {
        self.code.push(Op::Push8 as u8);
        self.code.extend_from_slice(&val.to_be_bytes());
        self
    }

    /// Emit PUSH8 <placeholder> and register a forward-reference patch.
    /// The 8-byte placeholder is filled with the label's resolved address at build time.
    pub fn push_label(&mut self, label: &str) -> &mut Self {
        self.code.push(Op::Push8 as u8);
        let patch_pos = self.code.len();
        self.code.extend_from_slice(&[0u8; 8]);
        self.patches.push((patch_pos, label.to_string()));
        self
    }

    /// Define a label at the current position and emit JUMPDEST.
    pub fn label(&mut self, name: &str) -> &mut Self {
        // The JUMP in RVM jumps to the JUMPDEST offset; the VM skips the
        // JUMPDEST opcode automatically (pc = dest + 1).
        let pos = self.code.len() as u64;
        self.labels.insert(name.to_string(), pos);
        self.code.push(Op::JumpDest as u8);
        self
    }

    /// Finish assembly: patch all label references and return the bytecode.
    ///
    /// Returns `Err` with the name of the first undefined label encountered.
    pub fn build(&self) -> Result<Vec<u8>, String> {
        let mut code = self.code.clone();
        for (offset, label) in &self.patches {
            let addr = *self.labels.get(label.as_str())
                .ok_or_else(|| format!("undefined label: {label}"))?;
            code[*offset..*offset + 8].copy_from_slice(&addr.to_be_bytes());
        }
        Ok(code)
    }

    /// Current byte length of the assembled code.
    pub fn len(&self) -> usize {
        self.code.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// Emit bytes verbatim (e.g. for inline data).
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.code.extend_from_slice(bytes);
        self
    }
}
