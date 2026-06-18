/// Phase 12.3 — Smart contract transaction types.
///
/// These extend the base `Transaction` with two new coinbase-style
/// transaction variants: contract deployment and contract invocation.
/// The `coinbase_data` prefix distinguishes them from regular txs.
use serde::{Deserialize, Serialize};

// 9-byte prefixes ensure no collision with the 8-byte LE u64 block height in regular coinbase_data.
pub const CONTRACT_DEPLOY_PREFIX: &[u8] = b"CD\x01\x00\x00\x00\x00\x00\x00";
pub const CONTRACT_CALL_PREFIX:   &[u8] = b"CC\x01\x00\x00\x00\x00\x00\x00";

/// A contract deployment payload embedded in `Transaction::coinbase_data`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractDeploy {
    /// RVM bytecode of the contract runtime (not the constructor).
    pub bytecode: Vec<u8>,
    /// ABI-encoded constructor arguments placed at memory[0] before the
    /// runtime code begins.  May be empty for argument-less constructors.
    pub constructor_args: Vec<u8>,
    /// Initial RPC value sent to the contract account (may be 0).
    pub value: u64,
    /// Maximum gas the deployer is willing to spend on construction.
    pub gas_limit: u64,
}

/// A contract invocation payload embedded in `Transaction::coinbase_data`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractCall {
    /// Address of the contract to call (8-byte RVM address).
    pub contract_addr: u64,
    /// Call data (selector + ABI-encoded arguments).
    pub calldata: Vec<u8>,
    /// RPC value transferred with the call.
    pub value: u64,
    /// Gas limit for this call.
    pub gas_limit: u64,
}

impl ContractDeploy {
    /// Serialise to the wire format used in `Transaction::coinbase_data`.
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        let mut out = CONTRACT_DEPLOY_PREFIX.to_vec();
        out.extend_from_slice(&bincode::serialize(self)?);
        Ok(out)
    }

    /// Deserialise from `Transaction::coinbase_data`.
    pub fn decode(data: &[u8]) -> Option<Self> {
        if !data.starts_with(CONTRACT_DEPLOY_PREFIX) {
            return None;
        }
        bincode::deserialize(&data[CONTRACT_DEPLOY_PREFIX.len()..]).ok()
    }
}

impl ContractCall {
    /// Serialise to the wire format used in `Transaction::coinbase_data`.
    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        let mut out = CONTRACT_CALL_PREFIX.to_vec();
        out.extend_from_slice(&bincode::serialize(self)?);
        Ok(out)
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if !data.starts_with(CONTRACT_CALL_PREFIX) {
            return None;
        }
        bincode::deserialize(&data[CONTRACT_CALL_PREFIX.len()..]).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_deploy_roundtrip() {
        let cd = ContractDeploy {
            bytecode: vec![0x60, 0x01, 0x60, 0x00, 0x52],
            constructor_args: vec![],
            value: 0,
            gas_limit: 100_000,
        };
        let encoded = cd.encode().unwrap();
        let decoded = ContractDeploy::decode(&encoded).unwrap();
        assert_eq!(cd, decoded);
    }

    #[test]
    fn contract_call_roundtrip() {
        let cc = ContractCall {
            contract_addr: 0xDEAD_BEEF_1234_5678,
            calldata: vec![0xAA, 0xBB, 0xCC, 0xDD, 0x00, 0x00, 0x00, 0x01],
            value: 1_000,
            gas_limit: 50_000,
        };
        let encoded = cc.encode().unwrap();
        let decoded = ContractCall::decode(&encoded).unwrap();
        assert_eq!(cc, decoded);
    }

    #[test]
    fn decode_wrong_prefix_returns_none() {
        let bad = b"NOTCD\x01\x00\x00".to_vec();
        assert!(ContractDeploy::decode(&bad).is_none());
        assert!(ContractCall::decode(&bad).is_none());
    }
}
