use super::Cause;

fn lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

macro_rules! id {
    ($(#[$m:meta])* $T:ident) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $T(String);

        impl $T {
            /// The one constructor: 32 lower-case hex digits, else `malformed` (I0.5).
            pub fn parse(text: &str) -> Result<$T, Cause> {
                if lower_hex(text, 32) {
                    Ok($T(text.to_owned()))
                } else {
                    Err(Cause::Malformed)
                }
            }

            /// The id's 32 hex digits.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl ::std::fmt::Display for $T {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl ::serde::Serialize for $T {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }
    };
}

id! {
    /// The id of an agent (A-A0.6).
    AgentId
}
id! {
    /// The id of a workflow (W-C2.1).
    WorkflowId
}
id! {
    /// The id of a run (A-A0.6).
    RunId
}
id! {
    /// The id of an effect (A-A0.6).
    EffectId
}
id! {
    /// The id of an approval in a decision (A-A0.6, A-A6.3).
    ApprovalId
}
id! {
    /// The id of an item, made by the coordinator from 16 bytes of its `Rng` port (I0.5).
    ItemId
}

/// The 32 lower-case hex digits of 16 bytes, the text that an id is parsed from (I0.5).
pub fn hex(bytes: [u8; 16]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(32);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 15)] as char);
    }
    s
}

/// A digest of a bound object: 64 lower-case hex digits (A-A6.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Digest(String);

impl Digest {
    /// Reads 64 lower-case hex digits, else `malformed`.
    pub fn parse(text: &str) -> Result<Digest, Cause> {
        if lower_hex(text, 64) {
            Ok(Digest(text.to_owned()))
        } else {
            Err(Cause::Malformed)
        }
    }

    /// The digest's 64 hex digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A name of I0.6's form that no closed list of this crate holds: a slot name, a field name or
/// the error name of an owner's reply. 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Name(String);

impl Name {
    /// Reads a name of I0.6's form, else `malformed`.
    pub fn parse(text: &str) -> Result<Name, Cause> {
        let ok = (1..=32).contains(&text.len())
            && text
                .bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'));
        if ok {
            Ok(Name(text.to_owned()))
        } else {
            Err(Cause::Malformed)
        }
    }

    /// The name's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An origin `<scheme>://<host>:<port>` in lower case with the port always written (A-A7.4), in
/// the form that the agent-setup run's schema takes and in that form alone (I0.5, ruling IG-21).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin(String);

impl Origin {
    /// Reads an origin as `WebOrigin::parse` of the agent-setup run's schema reads it at b8e367e:
    /// the scheme `http` or `https`; a host of 1 to 253 bytes of labels split by `.`, each of 1 to
    /// 63 bytes of `a` to `z`, `0` to `9` and `-` that neither begins nor ends with `-`; and a port
    /// from 1 to 65,535 with no leading zero. Anything else is `malformed`.
    pub fn parse(text: &str) -> Result<Origin, Cause> {
        let (scheme, rest) = text.split_once("://").ok_or(Cause::Malformed)?;
        let (host, port) = rest.rsplit_once(':').ok_or(Cause::Malformed)?;
        let label = |l: &str| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        };
        let host_ok = (1..=253).contains(&host.len()) && host.split('.').all(label);
        let port_ok = !port.is_empty()
            && !port.starts_with('0')
            && port.len() <= 5
            && port.bytes().all(|b| b.is_ascii_digit())
            && port.parse::<u32>().is_ok_and(|p| (1..=65_535).contains(&p));
        if matches!(scheme, "http" | "https") && host_ok && port_ok {
            Ok(Origin(text.to_owned()))
        } else {
            Err(Cause::Malformed)
        }
    }

    fn parts(&self) -> (&str, &str, &str) {
        let (scheme, rest) = self.0.split_once("://").unwrap_or(("", &self.0));
        let (host, port) = rest.rsplit_once(':').unwrap_or((rest, ""));
        (scheme, host, port)
    }

    /// The scheme: `http` or `https`.
    pub fn scheme(&self) -> &str {
        self.parts().0
    }

    /// The host.
    pub fn host(&self) -> &str {
        self.parts().1
    }

    /// The port, from 1 to 65,535.
    pub fn port(&self) -> u16 {
        self.parts().2.parse().unwrap_or(0)
    }

    /// The origin's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! text_serialize {
    ($($T:ident),+) => {
        $(
            impl ::serde::Serialize for $T {
                fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    s.serialize_str(&self.0)
                }
            }
        )+
    };
}

text_serialize!(Digest, Name, Origin);
