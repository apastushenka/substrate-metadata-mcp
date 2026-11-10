//! What a failed lookup reports, and the words this server puts it in: each error's `Display`.
//! The words name the tools a caller should turn to next, which only this server knows.

use thiserror::Error;

use crate::metadata::item::PalletItemKind;

// A variant that wraps another error is `#[error(transparent)]`, never a bare `#[from]`:
// `#[from]` alone makes the wrapped error a `source`, and the `{:#}` the server renders tool
// errors with would then print its words twice.

/// Several names match ignoring case, and none matches exactly. `name` is as the caller wrote
/// it, `matches` as the metadata spells them.
#[derive(Debug, Error)]
#[error("`{name}` matches several names ignoring case: {}", .matches.join(", "))]
pub struct AmbiguousName {
    pub name: String,
    pub matches: Vec<String>,
}

/// A name written together with its pallet or runtime API, as in `Balances::transfer_all` or
/// `Core.version`: `qualifier` is the part before the separator, `name` the part after it.
#[derive(Debug, PartialEq, Eq)]
pub struct QualifiedName {
    pub qualifier: String,
    pub name: String,
}

impl QualifiedName {
    /// `None` unless both halves are there: a name that only trails a separator is a typo,
    /// not an address.
    pub(super) fn parse(name: &str) -> Option<Self> {
        let (qualifier, name) = name.rsplit_once("::").or_else(|| name.rsplit_once('.'))?;

        if qualifier.is_empty() || name.is_empty() {
            return None;
        }

        Some(Self {
            qualifier: String::from(qualifier),
            name: String::from(name),
        })
    }
}

/// Why `pallet` found nothing.
#[derive(Debug, Error)]
pub enum PalletLookupError {
    /// The name matches several pallets, or several runtime APIs, ignoring case.
    #[error(transparent)]
    AmbiguousName(#[from] AmbiguousName),
    /// No pallet has the name, but this runtime API does.
    #[error(
        "`{0}` is a runtime API, not a pallet; `list_runtime_api_methods` lists its methods and `get_runtime_api_method` reads one"
    )]
    IsRuntimeApi(String),
    /// Neither a pallet nor a runtime API has the name, given as the caller wrote it.
    #[error("no pallet named `{0}`; `list_pallets` lists the valid names")]
    UnknownPallet(String),
}

/// Why `pallet_item` found nothing.
#[derive(Debug, Error)]
pub enum PalletItemLookupError {
    /// The item's name came with its pallet.
    #[error(
        "`name` is the item's own name: pass `{}` as `pallet` and `{}` as `name`",
        .0.qualifier,
        .0.name
    )]
    QualifiedName(QualifiedName),
    /// The name matches several items of this kind in the pallet, ignoring case.
    #[error(transparent)]
    AmbiguousName(#[from] AmbiguousName),
    /// The pallet itself was not found.
    #[error(transparent)]
    Pallet(#[from] PalletLookupError),
    /// The pallet has no item of this kind by that name: `pallet` as the metadata spells it,
    /// `name` as the caller wrote it. `candidates` are the pallet's items of other kinds that
    /// do have the name: each its kind, and the name as the metadata spells it.
    #[error("{}", unknown_item_message(.pallet, .kind, .name, .candidates))]
    UnknownItem {
        pallet: String,
        kind: PalletItemKind,
        name: String,
        candidates: Vec<(PalletItemKind, String)>,
    },
    /// A view function was asked for, and metadata before v16 has none in any pallet:
    /// `pallet` as the metadata spells it, `name` as the caller wrote it, `version` the
    /// metadata's.
    #[error(
        "no `view_function` named `{name}` in pallet `{pallet}`; view functions need metadata v16; this metadata is v{version}, so no pallet has any"
    )]
    NoViewFunctions {
        pallet: String,
        name: String,
        version: u32,
    },
}

/// Why `runtime_api` found nothing.
#[derive(Debug, Error)]
pub enum RuntimeApiLookupError {
    /// The name matches several runtime APIs, or several pallets, ignoring case.
    #[error(transparent)]
    AmbiguousName(#[from] AmbiguousName),
    /// No runtime API has the name, but this pallet does.
    #[error(
        "`{0}` is a pallet, not a runtime API; `list_pallet_items` lists its items and `get_pallet_item` reads one"
    )]
    IsPallet(String),
    /// Neither a runtime API nor a pallet has the name, given as the caller wrote it.
    #[error("no runtime API named `{0}`; `list_runtime_apis` lists the valid names")]
    UnknownRuntimeApi(String),
    /// Metadata before v15 has no runtime APIs at all; `version` is the metadata's.
    #[error("runtime APIs need metadata v15; this metadata is v{version}")]
    NoRuntimeApis { version: u32 },
}

/// Why `runtime_api_method` found nothing.
#[derive(Debug, Error)]
pub enum RuntimeApiMethodLookupError {
    /// The method's name came with its runtime API.
    #[error(
        "`name` is the method's own name: pass `{}` as `runtime_api` and `{}` as `name`",
        .0.qualifier,
        .0.name
    )]
    QualifiedName(QualifiedName),
    /// The name matches several methods of the runtime API, ignoring case.
    #[error(transparent)]
    AmbiguousName(#[from] AmbiguousName),
    /// The runtime API itself was not found.
    #[error(transparent)]
    RuntimeApi(#[from] RuntimeApiLookupError),
    /// The runtime API has no method by that name: `runtime_api` as the metadata spells it,
    /// `name` as the caller wrote it.
    #[error(
        "no method named `{name}` in runtime API `{runtime_api}`; `list_runtime_api_methods` lists them"
    )]
    UnknownMethod { runtime_api: String, name: String },
}

fn unknown_item_message(
    pallet: &str,
    kind: &PalletItemKind,
    name: &str,
    candidates: &[(PalletItemKind, String)],
) -> String {
    let kind = kind.as_str();

    if candidates.is_empty() {
        return format!(
            "no `{kind}` named `{name}` in pallet `{pallet}`; `list_pallet_items` lists them"
        );
    }

    // The metadata's spelling, not the request's: this is the string the caller will copy
    // into the retry, and `name` above is whatever they typed.
    let candidates = candidates
        .iter()
        .map(|(kind, actual)| format!("`{}` has `{actual}`", kind.as_str()))
        .collect::<Vec<_>>()
        .join(", ");

    format!("no `{kind}` named `{name}` in pallet `{pallet}`, but {candidates}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qualified(qualifier: &str, name: &str) -> Option<QualifiedName> {
        Some(QualifiedName {
            qualifier: String::from(qualifier),
            name: String::from(name),
        })
    }

    #[test]
    fn qualified_name_parse_splits_on_colons() {
        assert_eq!(
            QualifiedName::parse("Balances::transfer_all"),
            qualified("Balances", "transfer_all")
        );
        assert_eq!(
            QualifiedName::parse("Core::version"),
            qualified("Core", "version")
        );
    }

    #[test]
    fn qualified_name_parse_splits_on_dot() {
        assert_eq!(
            QualifiedName::parse("Balances.transfer"),
            qualified("Balances", "transfer")
        );
        assert_eq!(
            QualifiedName::parse("Core.version"),
            qualified("Core", "version")
        );
    }

    #[test]
    fn qualified_name_parse_leaves_plain_name_alone() {
        assert_eq!(QualifiedName::parse("transfer_all"), None);
        assert_eq!(QualifiedName::parse("version"), None);
    }

    #[test]
    fn qualified_name_parse_needs_both_halves() {
        assert_eq!(QualifiedName::parse("transfer."), None);
        assert_eq!(QualifiedName::parse("::transfer_all"), None);
        assert_eq!(QualifiedName::parse("version."), None);
        assert_eq!(QualifiedName::parse("::version"), None);
    }

    // The server renders a tool error with `{:#}`, which follows the `source` chain: a wrapped
    // error must come out once, as its own words.
    #[test]
    fn wrapped_error_is_worded_once() {
        let error = PalletItemLookupError::Pallet(PalletLookupError::UnknownPallet(String::from(
            "Staking",
        )));

        assert_eq!(
            format!("{:#}", anyhow::Error::from(error)),
            "no pallet named `Staking`; `list_pallets` lists the valid names"
        );
    }

    #[test]
    fn ambiguous_name_lists_the_matches() {
        let ambiguous = AmbiguousName {
            name: String::from("BALANCES"),
            matches: vec![String::from("balances"), String::from("Balances")],
        };

        assert_eq!(
            ambiguous.to_string(),
            "`BALANCES` matches several names ignoring case: balances, Balances"
        );
    }

    #[test]
    fn unknown_item_names_the_other_kind() {
        let error = PalletItemLookupError::UnknownItem {
            pallet: String::from("Balances"),
            kind: PalletItemKind::Call,
            name: String::from("ExistentialDeposit"),
            candidates: vec![(PalletItemKind::Constant, String::from("ExistentialDeposit"))],
        };

        assert_eq!(
            error.to_string(),
            "no `call` named `ExistentialDeposit` in pallet `Balances`, but `constant` has `ExistentialDeposit`"
        );
    }

    #[test]
    fn unknown_item_lists_several_other_kinds() {
        let error = PalletItemLookupError::UnknownItem {
            pallet: String::from("System"),
            kind: PalletItemKind::Call,
            name: String::from("account"),
            candidates: vec![
                (PalletItemKind::Storage, String::from("Account")),
                (PalletItemKind::Event, String::from("Account")),
            ],
        };

        assert_eq!(
            error.to_string(),
            "no `call` named `account` in pallet `System`, but `storage` has `Account`, `event` has `Account`"
        );
    }

    #[test]
    fn unknown_item_without_candidates_points_at_the_listing() {
        let error = PalletItemLookupError::UnknownItem {
            pallet: String::from("Balances"),
            kind: PalletItemKind::ViewFunction,
            name: String::from("get"),
            candidates: Vec::new(),
        };

        assert_eq!(
            error.to_string(),
            "no `view_function` named `get` in pallet `Balances`; `list_pallet_items` lists them"
        );
    }

    #[test]
    fn no_view_functions_points_at_the_version_not_the_listing() {
        let error = PalletItemLookupError::NoViewFunctions {
            pallet: String::from("Balances"),
            name: String::from("get"),
            version: 15,
        };

        assert_eq!(
            error.to_string(),
            "no `view_function` named `get` in pallet `Balances`; view functions need metadata v16; this metadata is v15, so no pallet has any"
        );
    }

    #[test]
    fn no_runtime_apis_points_at_the_version() {
        let error = RuntimeApiLookupError::NoRuntimeApis { version: 14 };

        assert_eq!(
            error.to_string(),
            "runtime APIs need metadata v15; this metadata is v14"
        );
    }
}
