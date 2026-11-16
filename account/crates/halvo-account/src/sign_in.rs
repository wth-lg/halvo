use halvo_secrets::Secret;
use halvo_wire::request::{AccountId, Credential};

const ID_LEN: usize = 36;
const CREDENTIAL_LEN: usize = 64;

/// The account id and the credential that the helper made, kept in the store as one
/// secret: the 36 characters of the id, one space and the 64 characters of the credential.
pub struct SignIn {
    account: AccountId,
    credential: Credential,
}

impl SignIn {
    /// The length of the secret, 101 bytes.
    pub const SECRET_LEN: usize = ID_LEN + 1 + CREDENTIAL_LEN;

    /// Makes a new sign-in of 48 random bytes: the first 16 make the id and the other 32
    /// the credential.
    pub fn from_random(bytes: &[u8; 48]) -> SignIn {
        let mut id = [0; 16];
        let mut credential = [0; 32];
        id.copy_from_slice(&bytes[..16]);
        credential.copy_from_slice(&bytes[16..]);
        SignIn {
            account: AccountId::from_random(id),
            credential: Credential::from_random(credential),
        }
    }

    /// Reads a stored secret, or answers `None` when it is not of the form above.
    pub fn from_secret(secret: &Secret) -> Option<SignIn> {
        let bytes = secret.expose();
        if bytes.len() != Self::SECRET_LEN || bytes[ID_LEN] != b' ' {
            return None;
        }
        let text = std::str::from_utf8(bytes).ok()?;
        Some(SignIn {
            account: AccountId::parse(&text[..ID_LEN])?,
            credential: Credential::parse(&text[ID_LEN + 1..])?,
        })
    }

    /// The secret that the store keeps for this sign-in.
    pub fn to_secret(&self) -> Secret {
        let mut bytes = Vec::with_capacity(Self::SECRET_LEN);
        bytes.extend_from_slice(self.account.as_str().as_bytes());
        bytes.push(b' ');
        bytes.extend_from_slice(self.credential.as_str().as_bytes());
        Secret::new(bytes).expect("101 bytes are a secret")
    }

    /// The account id, the value of the header `halvo-account`.
    pub fn account(&self) -> &AccountId {
        &self.account
    }

    /// The credential, which `authorization` carries after `Bearer `.
    pub fn credential(&self) -> &Credential {
        &self.credential
    }
}
