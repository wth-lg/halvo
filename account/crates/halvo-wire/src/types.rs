//! The types that typify 0.8.0 generates from `account/schema/schema.v1.json`.
//! `cargo run -p halvo-wire --example write-types` writes this file, and the test
//! `committed_types_equal_a_fresh_generation` fails when it differs.

///`Cap`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Cap {
    pub amount: ::std::num::NonZeroU64,
    pub period: Period,
}
///`CapBody`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CapBody {
    pub cap: Cap,
}
///`CapGetAnswer`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CapGetAnswer {
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub cap: ::std::option::Option<Cap>,
    pub result: CapGetAnswerResult,
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub spend: ::std::option::Option<Spend>,
}
///`CapGetAnswerResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum CapGetAnswerResult {
    #[serde(rename = "cap")]
    Cap,
    #[serde(rename = "no-cap")]
    NoCap,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "no-such-account")]
    NoSuchAccount,
    #[serde(rename = "credential-mismatch")]
    CredentialMismatch,
}
impl ::std::fmt::Display for CapGetAnswerResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Cap => f.write_str("cap"),
            Self::NoCap => f.write_str("no-cap"),
            Self::Pending => f.write_str("pending"),
            Self::NoSuchAccount => f.write_str("no-such-account"),
            Self::CredentialMismatch => f.write_str("credential-mismatch"),
        }
    }
}
impl ::std::str::FromStr for CapGetAnswerResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "cap" => Ok(Self::Cap),
            "no-cap" => Ok(Self::NoCap),
            "pending" => Ok(Self::Pending),
            "no-such-account" => Ok(Self::NoSuchAccount),
            "credential-mismatch" => Ok(Self::CredentialMismatch),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CapGetAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CapGetAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`CapSetAnswer`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CapSetAnswer {
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub cap: ::std::option::Option<Cap>,
    pub result: CapSetAnswerResult,
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub spend: ::std::option::Option<Spend>,
}
///`CapSetAnswerResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum CapSetAnswerResult {
    #[serde(rename = "cap")]
    Cap,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "no-such-account")]
    NoSuchAccount,
    #[serde(rename = "credential-mismatch")]
    CredentialMismatch,
}
impl ::std::fmt::Display for CapSetAnswerResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Cap => f.write_str("cap"),
            Self::Pending => f.write_str("pending"),
            Self::NoSuchAccount => f.write_str("no-such-account"),
            Self::CredentialMismatch => f.write_str("credential-mismatch"),
        }
    }
}
impl ::std::str::FromStr for CapSetAnswerResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "cap" => Ok(Self::Cap),
            "pending" => Ok(Self::Pending),
            "no-such-account" => Ok(Self::NoSuchAccount),
            "credential-mismatch" => Ok(Self::CredentialMismatch),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CapSetAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CapSetAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`CheckoutAnswer`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct CheckoutAnswer {
    pub result: CheckoutAnswerResult,
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub url: ::std::option::Option<Url>,
}
///`CheckoutAnswerResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum CheckoutAnswerResult {
    #[serde(rename = "session")]
    Session,
    #[serde(rename = "account-exists")]
    AccountExists,
    #[serde(rename = "credential-mismatch")]
    CredentialMismatch,
    #[serde(rename = "unavailable")]
    Unavailable,
}
impl ::std::fmt::Display for CheckoutAnswerResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Session => f.write_str("session"),
            Self::AccountExists => f.write_str("account-exists"),
            Self::CredentialMismatch => f.write_str("credential-mismatch"),
            Self::Unavailable => f.write_str("unavailable"),
        }
    }
}
impl ::std::str::FromStr for CheckoutAnswerResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "session" => Ok(Self::Session),
            "account-exists" => Ok(Self::AccountExists),
            "credential-mismatch" => Ok(Self::CredentialMismatch),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for CheckoutAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for CheckoutAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`DeleteAnswer`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DeleteAnswer {
    pub result: DeleteAnswerResult,
}
///`DeleteAnswerResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum DeleteAnswerResult {
    #[serde(rename = "deleted")]
    Deleted,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "not-finished")]
    NotFinished,
    #[serde(rename = "no-such-account")]
    NoSuchAccount,
    #[serde(rename = "credential-mismatch")]
    CredentialMismatch,
}
impl ::std::fmt::Display for DeleteAnswerResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Deleted => f.write_str("deleted"),
            Self::Pending => f.write_str("pending"),
            Self::NotFinished => f.write_str("not-finished"),
            Self::NoSuchAccount => f.write_str("no-such-account"),
            Self::CredentialMismatch => f.write_str("credential-mismatch"),
        }
    }
}
impl ::std::str::FromStr for DeleteAnswerResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "deleted" => Ok(Self::Deleted),
            "pending" => Ok(Self::Pending),
            "not-finished" => Ok(Self::NotFinished),
            "no-such-account" => Ok(Self::NoSuchAccount),
            "credential-mismatch" => Ok(Self::CredentialMismatch),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for DeleteAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for DeleteAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`ManageAnswer`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct ManageAnswer {
    pub result: ManageAnswerResult,
    #[serde(skip_serializing_if = "::std::option::Option::is_none")]
    pub url: ::std::option::Option<Url>,
}
///`ManageAnswerResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum ManageAnswerResult {
    #[serde(rename = "page")]
    Page,
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "no-such-account")]
    NoSuchAccount,
    #[serde(rename = "credential-mismatch")]
    CredentialMismatch,
    #[serde(rename = "unavailable")]
    Unavailable,
}
impl ::std::fmt::Display for ManageAnswerResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Page => f.write_str("page"),
            Self::Pending => f.write_str("pending"),
            Self::NoSuchAccount => f.write_str("no-such-account"),
            Self::CredentialMismatch => f.write_str("credential-mismatch"),
            Self::Unavailable => f.write_str("unavailable"),
        }
    }
}
impl ::std::str::FromStr for ManageAnswerResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "page" => Ok(Self::Page),
            "pending" => Ok(Self::Pending),
            "no-such-account" => Ok(Self::NoSuchAccount),
            "credential-mismatch" => Ok(Self::CredentialMismatch),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for ManageAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for ManageAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`Period`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum Period {
    #[serde(rename = "week")]
    Week,
    #[serde(rename = "month")]
    Month,
}
impl ::std::fmt::Display for Period {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Week => f.write_str("week"),
            Self::Month => f.write_str("month"),
        }
    }
}
impl ::std::str::FromStr for Period {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "week" => Ok(Self::Week),
            "month" => Ok(Self::Month),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Period {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Period {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`Rejected`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Rejected {
    pub result: RejectedResult,
}
///`RejectedResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum RejectedResult {
    #[serde(rename = "rejected")]
    Rejected,
}
impl ::std::fmt::Display for RejectedResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Rejected => f.write_str("rejected"),
        }
    }
}
impl ::std::str::FromStr for RejectedResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "rejected" => Ok(Self::Rejected),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for RejectedResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for RejectedResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`Spend`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Spend {
    pub held: i64,
    pub period_end: i64,
    pub spent: i64,
}
///`StatusAnswer`
#[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct StatusAnswer {
    pub result: StatusAnswerResult,
}
///`StatusAnswerResult`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum StatusAnswerResult {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "payment-failed")]
    PaymentFailed,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "no-such-account")]
    NoSuchAccount,
    #[serde(rename = "credential-mismatch")]
    CredentialMismatch,
}
impl ::std::fmt::Display for StatusAnswerResult {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::Pending => f.write_str("pending"),
            Self::Active => f.write_str("active"),
            Self::PaymentFailed => f.write_str("payment-failed"),
            Self::Cancelled => f.write_str("cancelled"),
            Self::NoSuchAccount => f.write_str("no-such-account"),
            Self::CredentialMismatch => f.write_str("credential-mismatch"),
        }
    }
}
impl ::std::str::FromStr for StatusAnswerResult {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "pending" => Ok(Self::Pending),
            "active" => Ok(Self::Active),
            "payment-failed" => Ok(Self::PaymentFailed),
            "cancelled" => Ok(Self::Cancelled),
            "no-such-account" => Ok(Self::NoSuchAccount),
            "credential-mismatch" => Ok(Self::CredentialMismatch),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for StatusAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for StatusAnswerResult {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`Tier`
#[derive(
    ::serde::Deserialize,
    ::serde::Serialize,
    Clone,
    Copy,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd
)]
pub enum Tier {
    #[serde(rename = "low_cost_hosted")]
    LowCostHosted,
    #[serde(rename = "frontier")]
    Frontier,
}
impl ::std::fmt::Display for Tier {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match *self {
            Self::LowCostHosted => f.write_str("low_cost_hosted"),
            Self::Frontier => f.write_str("frontier"),
        }
    }
}
impl ::std::str::FromStr for Tier {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        match value {
            "low_cost_hosted" => Ok(Self::LowCostHosted),
            "frontier" => Ok(Self::Frontier),
            _ => Err("invalid value".into()),
        }
    }
}
impl ::std::convert::TryFrom<&str> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Tier {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
///`Url`
#[derive(::serde::Serialize, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct Url(::std::string::String);
impl ::std::ops::Deref for Url {
    type Target = ::std::string::String;
    fn deref(&self) -> &::std::string::String {
        &self.0
    }
}
impl ::std::convert::From<Url> for ::std::string::String {
    fn from(value: Url) -> Self {
        value.0
    }
}
impl ::std::str::FromStr for Url {
    type Err = self::error::ConversionError;
    fn from_str(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        if value.chars().count() > 2048usize {
            return Err("longer than 2048 characters".into());
        }
        Ok(Self(value.to_string()))
    }
}
impl ::std::convert::TryFrom<&str> for Url {
    type Error = self::error::ConversionError;
    fn try_from(
        value: &str,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl ::std::convert::TryFrom<::std::string::String> for Url {
    type Error = self::error::ConversionError;
    fn try_from(
        value: ::std::string::String,
    ) -> ::std::result::Result<Self, self::error::ConversionError> {
        value.parse()
    }
}
impl<'de> ::serde::Deserialize<'de> for Url {
    fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        ::std::string::String::deserialize(deserializer)?
            .parse()
            .map_err(|e: self::error::ConversionError| {
                <D::Error as ::serde::de::Error>::custom(e.to_string())
            })
    }
}
/// Error types.
pub mod error {
    /// Error from a `TryFrom` or `FromStr` implementation.
    pub struct ConversionError(::std::borrow::Cow<'static, str>);
    impl ::std::error::Error for ConversionError {}
    impl ::std::fmt::Display for ConversionError {
        fn fmt(
            &self,
            f: &mut ::std::fmt::Formatter<'_>,
        ) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Display::fmt(&self.0, f)
        }
    }
    impl ::std::fmt::Debug for ConversionError {
        fn fmt(
            &self,
            f: &mut ::std::fmt::Formatter<'_>,
        ) -> Result<(), ::std::fmt::Error> {
            ::std::fmt::Debug::fmt(&self.0, f)
        }
    }
    impl From<&'static str> for ConversionError {
        fn from(value: &'static str) -> Self {
            Self(value.into())
        }
    }
    impl From<String> for ConversionError {
        fn from(value: String) -> Self {
            Self(value.into())
        }
    }
}
