//! NIST SP 800-56C Rev.2 one-step KDF (Option 1: hash-based), SHA-256.
//!
//! Confirmed for EAP-NOOB key derivation: SHA-256, 32-bit big-endian counter
//! starting at 1, `algorithm-id = "EAP-NOOB"`, 320-byte output split into
//! MSK/EMSK/AMSK/MethodId/Kms/Kmp/Kz.
//!
//! One block: `H( counter_be_u32 || Z || FixedInfo )`, concatenated until L bytes.

use sha2::{Digest, Sha256};

/// EAP-NOOB derived-key material total length (bytes).
pub const EAPNOOB_OUTPUT_LEN: usize = 320;

/// Algorithm-id literal that leads FixedInfo (byte-verified).
pub const ALGORITHM_ID: &[u8] = b"EAP-NOOB";

/// SP 800-56C one-step KDF with SHA-256.
pub fn one_step_kdf_sha256(z: &[u8], fixed_info: &[u8], out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let mut counter: u32 = 1;
    while out.len() < out_len {
        let mut h = Sha256::new();
        h.update(counter.to_be_bytes());
        h.update(z);
        h.update(fixed_info);
        out.extend_from_slice(&h.finalize());
        counter += 1;
    }
    out.truncate(out_len);
    out
}

/// Build the Completion-Exchange FixedInfo (RFC 9140 §3.5, Table 4):
/// `AlgorithmId("EAP-NOOB") || PartyUInfo(Np) || PartyVInfo(Ns) ||
/// SuppPrivInfo(Datalen=len(Noob) as one byte, Noob)`.
///
/// Confirmed against the upstream implementation (Apache-2.0): PartyUInfo is the
/// peer nonce Np, PartyVInfo is the server nonce Ns, and SuppPrivInfo carries a
/// one-byte length prefix before Noob.
pub fn eapnoob_fixed_info(np: &[u8], ns: &[u8], noob: &[u8]) -> Vec<u8> {
    let mut fi = Vec::with_capacity(ALGORITHM_ID.len() + np.len() + ns.len() + 1 + noob.len());
    fi.extend_from_slice(ALGORITHM_ID);
    fi.extend_from_slice(np);
    fi.extend_from_slice(ns);
    fi.push(noob.len() as u8); // SuppPrivInfo one-byte Datalen counter
    fi.extend_from_slice(noob);
    fi
}

/// The 320-byte EAP-NOOB output, split into named keys.
///
/// Offsets confirmed (RFC 9140 Table 5 + upstream): MSK(64) EMSK(64) AMSK(64)
/// MethodId(32) Kms(32) Kmp(32) Kz(32) = 320.
#[derive(Debug, Clone)]
pub struct DerivedKeys {
    pub msk: [u8; 64],
    pub emsk: [u8; 64],
    pub amsk: [u8; 64],
    pub method_id: [u8; 32],
    pub kms: [u8; 32],
    pub kmp: [u8; 32],
    pub kz: [u8; 32],
}

impl DerivedKeys {
    pub fn from_output(out: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(out.len() >= EAPNOOB_OUTPUT_LEN, "kdf output too short");
        let mut o = 0;
        let mut take = |n: usize| {
            let s = &out[o..o + n];
            o += n;
            s.to_vec()
        };
        Ok(Self {
            msk: take(64).try_into().unwrap(),
            emsk: take(64).try_into().unwrap(),
            amsk: take(64).try_into().unwrap(),
            method_id: take(32).try_into().unwrap(),
            kms: take(32).try_into().unwrap(),
            kmp: take(32).try_into().unwrap(),
            kz: take(32).try_into().unwrap(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdf_length_and_determinism() {
        let z = b"shared-secret";
        let fi = eapnoob_fixed_info(b"np", b"ns", b"noob");
        let a = one_step_kdf_sha256(z, &fi, EAPNOOB_OUTPUT_LEN);
        let b = one_step_kdf_sha256(z, &fi, EAPNOOB_OUTPUT_LEN);
        assert_eq!(a.len(), 320);
        assert_eq!(a, b);
        // first block must equal H(1||z||fi)
        let mut h = sha2::Sha256::new();
        h.update(1u32.to_be_bytes());
        h.update(z);
        h.update(&fi);
        assert_eq!(&a[..32], &h.finalize()[..]);
    }

    #[test]
    fn split_320() {
        let out = vec![7u8; 320];
        let k = DerivedKeys::from_output(&out).unwrap();
        assert_eq!(k.msk.len(), 64);
        assert_eq!(k.kz.len(), 32);
    }
}
