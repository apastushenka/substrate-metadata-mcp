//! Finding pallets, runtime APIs and their items by the names a caller gives.
//!
//! A name is found as the metadata spells it, or ignoring case when that picks out a single
//! name. A miss comes back as data: `error` defines it, together with the words this server
//! puts it in. The `*LookupError` types implement neither `Display` nor `std::error::Error`,
//! so `?` cannot pass a miss on before it has been worded.

use scale_info::{Variant, form::PortableForm};
use subxt_metadata::{
    ConstantMetadata, Metadata, PalletMetadata, RuntimeApiMetadata, RuntimeApiMethodMetadata,
    StorageEntryMetadata, ViewFunctionMetadata,
};

use super::item::PalletItemKind;

mod error;

use self::error::{
    AmbiguousName, PalletItemLookupError, PalletLookupError, QualifiedName, RuntimeApiLookupError,
    RuntimeApiMethodLookupError,
};
pub use self::error::{
    pallet_item_lookup_error_message, pallet_lookup_error_message,
    runtime_api_lookup_error_message, runtime_api_method_lookup_error_message,
};

/// An item found in a pallet, one variant per kind.
pub enum PalletItem<'a> {
    Call(&'a Variant<PortableForm>),
    ViewFunction(ViewFunctionMetadata<'a>),
    Storage(&'a StorageEntryMetadata),
    Constant(&'a ConstantMetadata),
    Event(&'a Variant<PortableForm>),
    Error(&'a Variant<PortableForm>),
}

impl<'a> PalletItem<'a> {
    fn name(&self) -> &'a str {
        match self {
            Self::Call(variant) | Self::Event(variant) | Self::Error(variant) => &variant.name,
            Self::ViewFunction(view_function) => view_function.name(),
            Self::Storage(entry) => entry.name(),
            Self::Constant(constant) => constant.name(),
        }
    }
}

/// The pallet called `name`. A miss also says whether a runtime API has that name, which tells
/// a confused namespace from a typo.
pub fn pallet<'a>(
    metadata: &'a Metadata,
    name: &str,
) -> Result<PalletMetadata<'a>, PalletLookupError> {
    let pallets = metadata.pallets().map(|pallet| (pallet.name(), pallet));

    if let Some(pallet) = resolve(name, pallets)? {
        return Ok(pallet);
    }

    let runtime_apis = metadata
        .runtime_api_traits()
        .map(|api| (api.name(), api.name()));

    if let Some(runtime_api) = resolve(name, runtime_apis)? {
        return Err(PalletLookupError::IsRuntimeApi(String::from(runtime_api)));
    }

    Err(PalletLookupError::UnknownPallet(String::from(name)))
}

/// The item of `kind` called `name` in `pallet`, along with the pallet. A `name` that carries
/// its pallet is refused, not split. A miss lists the pallet's items of other kinds that have
/// the name.
pub fn pallet_item<'a>(
    metadata: &'a Metadata,
    pallet: &str,
    kind: PalletItemKind,
    name: &str,
) -> Result<(PalletMetadata<'a>, PalletItem<'a>), PalletItemLookupError> {
    // The listings hand back `namespace` and `name` separately, so a qualified name is
    // already split somewhere: hand the split back rather than accept a second spelling.
    if let Some(qualified) = QualifiedName::parse(name) {
        return Err(PalletItemLookupError::QualifiedName(qualified));
    }

    let pallet = self::pallet(metadata, pallet)?;

    if let Some(item) = find_item(&pallet, kind, name)? {
        return Ok((pallet, item));
    }

    let mut candidates = Vec::new();

    for other in PalletItemKind::ALL {
        if other == kind {
            continue;
        }

        // Advisory: this only decides whether to offer a hint, so an unresolvable name
        // under a kind nobody asked about must not replace the answer above.
        if let Some(item) = find_item(&pallet, other, name).ok().flatten() {
            candidates.push((other, String::from(item.name())));
        }
    }

    Err(PalletItemLookupError::UnknownItem {
        pallet: String::from(pallet.name()),
        kind,
        name: String::from(name),
        candidates,
    })
}

/// The runtime API called `name`. A miss also says whether a pallet has that name.
pub fn runtime_api<'a>(
    metadata: &'a Metadata,
    name: &str,
) -> Result<RuntimeApiMetadata<'a>, RuntimeApiLookupError> {
    let runtime_apis = metadata.runtime_api_traits().map(|api| (api.name(), api));

    if let Some(runtime_api) = resolve(name, runtime_apis)? {
        return Ok(runtime_api);
    }

    let pallets = metadata
        .pallets()
        .map(|pallet| (pallet.name(), pallet.name()));

    if let Some(pallet) = resolve(name, pallets)? {
        return Err(RuntimeApiLookupError::IsPallet(String::from(pallet)));
    }

    Err(RuntimeApiLookupError::UnknownRuntimeApi(String::from(name)))
}

/// The method called `name` in `runtime_api`, along with the runtime API. A `name` that carries
/// its runtime API is refused, not split.
pub fn runtime_api_method<'a>(
    metadata: &'a Metadata,
    runtime_api: &str,
    name: &str,
) -> Result<(RuntimeApiMetadata<'a>, RuntimeApiMethodMetadata<'a>), RuntimeApiMethodLookupError> {
    // As for a pallet item: the listings return the runtime API and the method apart.
    if let Some(qualified) = QualifiedName::parse(name) {
        return Err(RuntimeApiMethodLookupError::QualifiedName(qualified));
    }

    let runtime_api = self::runtime_api(metadata, runtime_api)?;
    let methods = runtime_api.methods().map(|method| (method.name(), method));

    match resolve(name, methods)? {
        Some(method) => Ok((runtime_api, method)),
        None => Err(RuntimeApiMethodLookupError::UnknownMethod {
            runtime_api: String::from(runtime_api.name()),
            name: String::from(name),
        }),
    }
}

fn find_item<'a>(
    pallet: &PalletMetadata<'a>,
    kind: PalletItemKind,
    name: &str,
) -> Result<Option<PalletItem<'a>>, AmbiguousName> {
    let items = items_of_kind(pallet, kind);

    resolve(name, items.into_iter().map(|item| (item.name(), item)))
}

fn items_of_kind<'a>(pallet: &PalletMetadata<'a>, kind: PalletItemKind) -> Vec<PalletItem<'a>> {
    match kind {
        PalletItemKind::Call => pallet
            .call_variants()
            .unwrap_or_default()
            .iter()
            .map(PalletItem::Call)
            .collect(),
        PalletItemKind::ViewFunction => pallet
            .view_functions()
            .map(PalletItem::ViewFunction)
            .collect(),
        PalletItemKind::Storage => pallet
            .storage()
            .map(|storage| storage.entries())
            .unwrap_or_default()
            .iter()
            .map(PalletItem::Storage)
            .collect(),
        PalletItemKind::Constant => pallet.constants().map(PalletItem::Constant).collect(),
        PalletItemKind::Event => pallet
            .event_variants()
            .unwrap_or_default()
            .iter()
            .map(PalletItem::Event)
            .collect(),
        PalletItemKind::Error => pallet
            .error_variants()
            .unwrap_or_default()
            .iter()
            .map(PalletItem::Error)
            .collect(),
    }
}

/// The item whose name is `name`: spelled exactly, or ignoring case when only one name
/// matches that way.
fn resolve<'a, T>(
    name: &str,
    items: impl IntoIterator<Item = (&'a str, T)>,
) -> Result<Option<T>, AmbiguousName> {
    let mut ignoring_case = Vec::new();

    for (candidate, item) in items {
        if candidate == name {
            return Ok(Some(item));
        }

        if candidate.eq_ignore_ascii_case(name) {
            ignoring_case.push((candidate, item));
        }
    }

    if ignoring_case.len() > 1 {
        return Err(AmbiguousName {
            name: String::from(name),
            matches: ignoring_case
                .into_iter()
                .map(|(candidate, _)| String::from(candidate))
                .collect(),
        });
    }

    Ok(ignoring_case.pop().map(|(_, item)| item))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each name stands in for the item it names.
    fn named<'a>(names: &[&'a str]) -> Vec<(&'a str, &'a str)> {
        names.iter().map(|name| (*name, *name)).collect()
    }

    #[test]
    fn resolve_returns_exact_name() {
        let names = named(&["System", "Balances"]);

        assert_eq!(resolve("Balances", names).unwrap(), Some("Balances"));
    }

    #[test]
    fn resolve_forgives_case() {
        let names = named(&["System", "Balances"]);

        assert_eq!(resolve("balances", names).unwrap(), Some("Balances"));
    }

    #[test]
    fn resolve_prefers_exact_name_over_case_variant() {
        let names = named(&["balances", "Balances"]);

        assert_eq!(resolve("Balances", names).unwrap(), Some("Balances"));
    }

    #[test]
    fn resolve_finds_nothing_for_unknown_name() {
        let names = named(&["System", "Balances"]);

        assert_eq!(resolve("Nonsense", names).unwrap(), None);
    }

    #[test]
    fn resolve_rejects_ambiguous_case_variants() {
        let names = named(&["balances", "Balances"]);
        let ambiguous = resolve("BALANCES", names).unwrap_err();

        assert_eq!(ambiguous.name, "BALANCES");
        assert_eq!(ambiguous.matches, ["balances", "Balances"]);
    }
}
