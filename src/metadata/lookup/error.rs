//! What a failed lookup reports, and the words this server puts it in. The words name the
//! tools a caller should turn to next, which only this server knows.

use crate::metadata::item::PalletItemKind;

/// Several names match ignoring case, and none matches exactly. `name` is as the caller wrote
/// it, `matches` as the metadata spells them.
#[derive(Debug)]
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
#[derive(Debug)]
pub enum PalletLookupError {
    /// The name matches several pallets, or several runtime APIs, ignoring case.
    AmbiguousName(AmbiguousName),
    /// No pallet has the name, but this runtime API does.
    IsRuntimeApi(String),
    /// Neither a pallet nor a runtime API has the name, given as the caller wrote it.
    UnknownPallet(String),
}

impl From<AmbiguousName> for PalletLookupError {
    fn from(ambiguous: AmbiguousName) -> Self {
        Self::AmbiguousName(ambiguous)
    }
}

/// Why `pallet_item` found nothing.
#[derive(Debug)]
pub enum PalletItemLookupError {
    /// The item's name came with its pallet.
    QualifiedName(QualifiedName),
    /// The name matches several items of this kind in the pallet, ignoring case.
    AmbiguousName(AmbiguousName),
    /// The pallet itself was not found.
    Pallet(PalletLookupError),
    /// The pallet has no item of this kind by that name: `pallet` as the metadata spells it,
    /// `name` as the caller wrote it. `candidates` are the pallet's items of other kinds that
    /// do have the name: each its kind, and the name as the metadata spells it.
    UnknownItem {
        pallet: String,
        kind: PalletItemKind,
        name: String,
        candidates: Vec<(PalletItemKind, String)>,
    },
}

impl From<AmbiguousName> for PalletItemLookupError {
    fn from(ambiguous: AmbiguousName) -> Self {
        Self::AmbiguousName(ambiguous)
    }
}

impl From<PalletLookupError> for PalletItemLookupError {
    fn from(error: PalletLookupError) -> Self {
        Self::Pallet(error)
    }
}

/// Why `runtime_api` found nothing.
#[derive(Debug)]
pub enum RuntimeApiLookupError {
    /// The name matches several runtime APIs, or several pallets, ignoring case.
    AmbiguousName(AmbiguousName),
    /// No runtime API has the name, but this pallet does.
    IsPallet(String),
    /// Neither a runtime API nor a pallet has the name, given as the caller wrote it.
    UnknownRuntimeApi(String),
}

impl From<AmbiguousName> for RuntimeApiLookupError {
    fn from(ambiguous: AmbiguousName) -> Self {
        Self::AmbiguousName(ambiguous)
    }
}

/// Why `runtime_api_method` found nothing.
#[derive(Debug)]
pub enum RuntimeApiMethodLookupError {
    /// The method's name came with its runtime API.
    QualifiedName(QualifiedName),
    /// The name matches several methods of the runtime API, ignoring case.
    AmbiguousName(AmbiguousName),
    /// The runtime API itself was not found.
    RuntimeApi(RuntimeApiLookupError),
    /// The runtime API has no method by that name: `runtime_api` as the metadata spells it,
    /// `name` as the caller wrote it.
    UnknownMethod { runtime_api: String, name: String },
}

impl From<AmbiguousName> for RuntimeApiMethodLookupError {
    fn from(ambiguous: AmbiguousName) -> Self {
        Self::AmbiguousName(ambiguous)
    }
}

impl From<RuntimeApiLookupError> for RuntimeApiMethodLookupError {
    fn from(error: RuntimeApiLookupError) -> Self {
        Self::RuntimeApi(error)
    }
}

pub fn pallet_lookup_error_message(error: &PalletLookupError) -> String {
    match error {
        PalletLookupError::AmbiguousName(ambiguous) => ambiguous_name_message(ambiguous),
        PalletLookupError::IsRuntimeApi(runtime_api) => format!(
            "`{runtime_api}` is a runtime API, not a pallet; `list_runtime_api_methods` lists its methods and `get_runtime_api_method` reads one"
        ),
        PalletLookupError::UnknownPallet(name) => {
            format!("no pallet named `{name}`; `list_pallets` lists the valid names")
        }
    }
}

pub fn pallet_item_lookup_error_message(error: &PalletItemLookupError, version: u32) -> String {
    match error {
        PalletItemLookupError::QualifiedName(QualifiedName { qualifier, name }) => format!(
            "`name` is the item's own name: pass `{qualifier}` as `pallet` and `{name}` as `name`"
        ),
        PalletItemLookupError::AmbiguousName(ambiguous) => ambiguous_name_message(ambiguous),
        PalletItemLookupError::Pallet(error) => pallet_lookup_error_message(error),
        PalletItemLookupError::UnknownItem {
            pallet,
            kind,
            name,
            candidates,
        } => unknown_item_message(pallet, *kind, name, candidates, version),
    }
}

pub fn runtime_api_lookup_error_message(error: &RuntimeApiLookupError, version: u32) -> String {
    // Metadata before v15 carries no runtime APIs at all, so whatever else the name missed,
    // that is why it was not found.
    if version < 15 {
        return format!("runtime APIs need metadata v15; this metadata is v{version}");
    }

    match error {
        RuntimeApiLookupError::AmbiguousName(ambiguous) => ambiguous_name_message(ambiguous),
        RuntimeApiLookupError::IsPallet(pallet) => {
            format!(
                "`{pallet}` is a pallet, not a runtime API; `list_pallet_items` lists its items and `get_pallet_item` reads one"
            )
        }
        RuntimeApiLookupError::UnknownRuntimeApi(name) => {
            format!("no runtime API named `{name}`; `list_runtime_apis` lists the valid names")
        }
    }
}

pub fn runtime_api_method_lookup_error_message(
    error: &RuntimeApiMethodLookupError,
    version: u32,
) -> String {
    match error {
        RuntimeApiMethodLookupError::QualifiedName(QualifiedName { qualifier, name }) => format!(
            "`name` is the method's own name: pass `{qualifier}` as `runtime_api` and `{name}` as `name`"
        ),
        RuntimeApiMethodLookupError::AmbiguousName(ambiguous) => ambiguous_name_message(ambiguous),
        RuntimeApiMethodLookupError::RuntimeApi(error) => {
            runtime_api_lookup_error_message(error, version)
        }
        RuntimeApiMethodLookupError::UnknownMethod { runtime_api, name } => format!(
            "no method named `{name}` in runtime API `{runtime_api}`; `list_runtime_api_methods` lists them"
        ),
    }
}

fn ambiguous_name_message(ambiguous: &AmbiguousName) -> String {
    format!(
        "`{}` matches several names ignoring case: {}",
        ambiguous.name,
        ambiguous.matches.join(", ")
    )
}

fn unknown_item_message(
    pallet: &str,
    kind: PalletItemKind,
    name: &str,
    candidates: &[(PalletItemKind, String)],
    version: u32,
) -> String {
    let kind_name = kind.as_str();

    if !candidates.is_empty() {
        // The metadata's spelling, not the request's: this is the string the caller
        // will copy into the retry, and `name` below is whatever they typed.
        let candidates = candidates
            .iter()
            .map(|(kind, actual)| format!("`{}` has `{actual}`", kind.as_str()))
            .collect::<Vec<_>>()
            .join(", ");

        return format!("no `{kind_name}` named `{name}` in pallet `{pallet}`, but {candidates}");
    }

    if kind == PalletItemKind::ViewFunction && version < 16 {
        return format!(
            "no `{kind_name}` named `{name}` in pallet `{pallet}`; view functions need metadata v16; this metadata is v{version}, so no pallet has any"
        );
    }

    format!("no `{kind_name}` named `{name}` in pallet `{pallet}`; `list_pallet_items` lists them")
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

    #[test]
    fn runtime_api_lookup_error_message_blames_the_version_before_v15() {
        assert_eq!(
            runtime_api_lookup_error_message(
                &RuntimeApiLookupError::IsPallet(String::from("Balances")),
                14
            ),
            "runtime APIs need metadata v15; this metadata is v14"
        );
    }

    #[test]
    fn ambiguous_name_message_lists_the_matches() {
        let ambiguous = AmbiguousName {
            name: String::from("BALANCES"),
            matches: vec![String::from("balances"), String::from("Balances")],
        };

        assert_eq!(
            ambiguous_name_message(&ambiguous),
            "`BALANCES` matches several names ignoring case: balances, Balances"
        );
    }

    #[test]
    fn unknown_item_message_names_the_other_kind() {
        assert_eq!(
            unknown_item_message(
                "Balances",
                PalletItemKind::Call,
                "ExistentialDeposit",
                &[(PalletItemKind::Constant, String::from("ExistentialDeposit"))],
                16
            ),
            "no `call` named `ExistentialDeposit` in pallet `Balances`, but `constant` has `ExistentialDeposit`"
        );
    }

    #[test]
    fn unknown_item_message_lists_several_other_kinds() {
        assert_eq!(
            unknown_item_message(
                "System",
                PalletItemKind::Call,
                "account",
                &[
                    (PalletItemKind::Storage, String::from("Account")),
                    (PalletItemKind::Event, String::from("Account"))
                ],
                16
            ),
            "no `call` named `account` in pallet `System`, but `storage` has `Account`, `event` has `Account`"
        );
    }

    #[test]
    fn unknown_item_message_points_at_the_version_not_the_listing_before_v16() {
        assert_eq!(
            unknown_item_message("Balances", PalletItemKind::ViewFunction, "get", &[], 15),
            "no `view_function` named `get` in pallet `Balances`; view functions need metadata v16; this metadata is v15, so no pallet has any"
        );
    }

    #[test]
    fn unknown_item_message_does_not_note_the_view_function_version_on_v16() {
        assert_eq!(
            unknown_item_message("Balances", PalletItemKind::ViewFunction, "get", &[], 16),
            "no `view_function` named `get` in pallet `Balances`; `list_pallet_items` lists them"
        );
    }
}
