//! Reading one item of a pallet.

use anyhow::{Context, bail};
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use subxt_metadata::{PalletMetadata, StorageHasher};

use super::{
    LoadedMetadata,
    item::{PalletItemKind, docs, resolve_name, split_qualified_name},
    registry::{Field, render_value, type_name},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PalletItemAddress {
    /// The pallet the item belongs to. `list_pallets` lists the valid names; take a
    /// pallet's `name` from there, or an item's `namespace` from `search` or
    /// `list_pallet_items`, as `pallet`. Case is forgiven, `balances` finds `Balances`,
    /// and the response spells the name as the metadata does. A runtime API name is an
    /// error: this tool reads pallet items only.
    pub pallet: String,

    /// Which kind of item to read. The six values are those of `list_pallet_items` and of
    /// `search`'s pallet-item hits; pass the listing's `kind` unchanged. A name that
    /// exists in the pallet under a different kind is an error, not a redirect -- the
    /// kind is part of the address.
    pub kind: PalletItemKind,

    /// The item's own name, exactly as `search` and `list_pallet_items` return it in
    /// `name` -- without the pallet. Pass it on verbatim; do not qualify it
    /// (`Balances::transfer_allow_death` belongs in two fields, not one). Case is
    /// forgiven. An unknown name is an error.
    pub name: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PalletItemDetails {
    /// The pallet the item belongs to, spelled as the metadata does -- not as the
    /// request did.
    pub pallet: String,

    /// The item's own name, spelled as the metadata does -- not as the request did.
    pub name: String,

    /// The item's own documentation, uncut, one string per line, with the leading space
    /// FRAME leaves after `///` stripped. Empty when the item has none. Unlike `search`
    /// and `list_pallet_items`, this is not a summary and is not truncated.
    pub docs: Vec<String>,

    #[serde(flatten)]
    pub shape: PalletItemShape,
}

// A branch's fields are serialised into the same object as `pallet`, `name`, `docs` and
// `kind`, so none of those names can be reused inside one: nothing reports the collision,
// not serde, not schemars, not the compiler. `flatten` also drops this type's own
// description from the schema, so what the model needs belongs on the tool. A branch's
// own description is read by the client and by whoever reads this file, like every
// description on the response side.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PalletItemShape {
    /// A dispatchable call: the two SCALE indices that identify it, and its arguments.
    /// Calls have no return type in the metadata; the field is absent, not null.
    Call {
        /// The first byte of the SCALE encoding: which pallet this call belongs to.
        pallet_index: u8,
        /// The second byte of the SCALE encoding: which variant of the pallet's call
        /// enum this is. The codec index (`#[codec(index = N)]`), not the position in
        /// `list_pallet_items`; FRAME may leave gaps.
        call_index: u8,
        /// The call's arguments, in the order the pallet declares them.
        arguments: Vec<Field>,
    },
    /// A read-only pallet query (metadata v16 and later): its arguments and return type.
    ViewFunction {
        /// The query's arguments, in the order the pallet declares them. Their `docs`
        /// are always empty -- metadata does not document view-function parameters.
        arguments: Vec<Field>,
        /// The query's return type, rendered from the registry the same way as each
        /// argument's `type`: a primitive (`u32`), a sequence (`Vec<u32>`), a named type
        /// with its parameters (`Result<AccountData, DispatchError>`), and so on. The
        /// last path segment only, not the module path.
        returns: String,
    },
    /// A storage entry: the keys (empty for a plain value), the value's type, whether a
    /// missing key is valid, and the default value when the entry has one.
    Storage {
        /// The keys of this entry, in order: none for a plain (unmapped) value, one for a
        /// map, and one per key of an N-map, which has no fixed width.
        keys: Vec<StorageKey>,
        /// The type stored under the keys, rendered from the registry the same way as a
        /// field's `type`. A type name, not a value -- the value a missing key falls back
        /// to is `default`.
        value_type: String,
        /// Whether a missing key is valid (`true`) or the entry always has a value
        /// (`false`). Stated rather than left to be inferred from `default`, which is
        /// absent exactly when this is `true`.
        optional: bool,
        /// The value a missing key falls back to. Absent only when the entry is
        /// optional, which has no fallback; an entry that is not optional always has
        /// one, so this is never dropped silently. Rendered by scale-value's `Display`,
        /// not as JSON: everything unnamed is a parenthesised list, so `()` is an empty
        /// sequence and not a unit, `(x)` a one-field wrapper, and a byte array reads as
        /// decimal numbers; variants read `Some ({ ... })` and `None ()`. Hex (`0x...`)
        /// instead for a `u256` or an `i256`, which have no text form.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[schemars(extend("type" = "string"))]
        default: Option<String>,
    },
    /// A pallet constant: its type and its SCALE-decoded value.
    Constant {
        /// The constant's type, rendered from the registry the same way as a field's
        /// `type`.
        #[serde(rename = "type")]
        ty: String,
        /// The constant's value, decoded from SCALE and rendered by scale-value's
        /// `Display`, not as JSON: everything unnamed is a parenthesised list, so `()`
        /// is an empty sequence and not a unit, `(x)` a one-field wrapper, and a byte
        /// array reads as decimal numbers; variants read `Some ({ ... })` and
        /// `None ()`. Hex (`0x...`) instead for a `u256` or an `i256`, which have no
        /// text form.
        value: String,
    },
    /// A pallet event: the two SCALE indices that identify it, and its fields, which may
    /// be unnamed.
    Event {
        /// The first byte of the SCALE encoding: which pallet this event belongs to.
        pallet_index: u8,
        /// The second byte of the SCALE encoding: which variant of the pallet's event
        /// enum this is. The codec index (`#[codec(index = N)]`), not the position in
        /// `list_pallet_items`; FRAME may leave gaps.
        event_index: u8,
        /// The event's fields, in the order the pallet declares them. `name` is absent
        /// on an unnamed field -- events often have those.
        fields: Vec<Field>,
    },
    /// A pallet error: the two SCALE indices that identify it, and its fields.
    Error {
        /// The first byte of the SCALE encoding: which pallet this error belongs to.
        pallet_index: u8,
        /// The second byte of the SCALE encoding: which variant of the pallet's error
        /// enum this is. The codec index (`#[codec(index = N)]`), not the position in
        /// `list_pallet_items`; FRAME may leave gaps.
        error_index: u8,
        /// The error's fields, in the order the pallet declares them.
        fields: Vec<Field>,
    },
}

/// One key of a storage map: how it is hashed, and its type. A plain value has none, a
/// map one, an N-map one per key.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StorageKey {
    /// How this key is hashed in the storage key. The names are FRAME's
    /// (`Blake2_128Concat`, `Twox64Concat`, `Identity`), as written on `StorageMap`.
    pub hasher: Hasher,
    /// The key's type, rendered from the registry the same way as a field's `type`.
    #[serde(rename = "type")]
    pub ty: String,
}

/// How a storage key is hashed. The names are FRAME's, as written on `StorageMap` and as
/// polkadot-js shows them -- not snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
pub enum Hasher {
    /// 128-bit Blake2 hash.
    Blake2_128,
    /// 256-bit Blake2 hash.
    Blake2_256,
    /// Blake2 128-bit hash followed by the key itself.
    Blake2_128Concat,
    /// 128-bit xxHash.
    Twox128,
    /// 256-bit xxHash.
    Twox256,
    /// 64-bit xxHash followed by the key itself.
    Twox64Concat,
    /// The key bytes themselves, not hashed.
    Identity,
}

impl From<StorageHasher> for Hasher {
    fn from(hasher: StorageHasher) -> Self {
        match hasher {
            StorageHasher::Blake2_128 => Self::Blake2_128,
            StorageHasher::Blake2_256 => Self::Blake2_256,
            StorageHasher::Blake2_128Concat => Self::Blake2_128Concat,
            StorageHasher::Twox128 => Self::Twox128,
            StorageHasher::Twox256 => Self::Twox256,
            StorageHasher::Twox64Concat => Self::Twox64Concat,
            StorageHasher::Identity => Self::Identity,
        }
    }
}

pub fn pallet_item(
    loaded: &LoadedMetadata,
    address: &PalletItemAddress,
) -> anyhow::Result<PalletItemDetails> {
    // The listings hand back `namespace` and `name` separately, so a qualified name is
    // already split somewhere: hand the split back rather than accept a second spelling.
    if let Some((qualifier, item)) = split_qualified_name(&address.name) {
        bail!(
            "`name` is the item's own name: pass `{qualifier}` as `pallet` and `{item}` as `name`"
        );
    }

    let pallet_name = resolve_pallet_name(loaded, &address.pallet)?;
    let pallet = loaded
        .metadata
        .pallet_by_name(pallet_name)
        .expect("resolve_pallet_name selected this pallet");

    let name = resolve_item_name(&pallet, address, loaded.version)?;
    let types = loaded.metadata.types();

    let (docs, shape) = match address.kind {
        PalletItemKind::Call => {
            let variant = pallet
                .call_variant_by_name(name)
                .expect("resolve_item_name found this call");

            let arguments = variant
                .fields
                .iter()
                .map(|field| Field::from_field(field, types))
                .collect();

            (
                docs(&variant.docs),
                PalletItemShape::Call {
                    pallet_index: pallet.call_index(),
                    call_index: variant.index,
                    arguments,
                },
            )
        }
        PalletItemKind::ViewFunction => {
            let view_function = pallet
                .view_function_by_name(name)
                .expect("resolve_item_name found this view function");

            let arguments = view_function
                .inputs()
                .map(|input| Field::from_view_function_input(input, types))
                .collect();

            let returns = type_name(view_function.output_ty(), types);

            (
                docs(view_function.docs()),
                PalletItemShape::ViewFunction { arguments, returns },
            )
        }
        PalletItemKind::Storage => {
            let entry = pallet
                .storage()
                .and_then(|storage| storage.entry_by_name(name))
                .expect("resolve_item_name found this storage entry");

            let keys = entry
                .keys()
                .map(|key| StorageKey {
                    hasher: Hasher::from(key.hasher),
                    ty: type_name(key.key_id, types),
                })
                .collect();

            let value_type = type_name(entry.value_ty(), types);

            let default = entry.default_value()
                .map(|bytes| {
                    render_value(bytes, entry.value_ty(), types).with_context(|| {
                        format!(
                            "cannot decode the default of storage entry `{name}` in pallet `{}` as `{value_type}`",
                            pallet.name(),
                        )
                    })
                })
                .transpose()?;

            (
                docs(entry.docs()),
                PalletItemShape::Storage {
                    keys,
                    value_type,
                    optional: entry.default_value().is_none(),
                    default,
                },
            )
        }
        PalletItemKind::Constant => {
            let constant = pallet
                .constant_by_name(name)
                .expect("resolve_item_name found this constant");

            let ty = type_name(constant.ty(), types);
            let value =
                render_value(constant.value(), constant.ty(), types).with_context(|| {
                    format!(
                        "cannot decode the value of constant `{name}` in pallet `{}` as `{ty}`",
                        pallet.name(),
                    )
                })?;

            (
                docs(constant.docs()),
                PalletItemShape::Constant { ty, value },
            )
        }
        PalletItemKind::Event => {
            let variant = pallet
                .event_variants()
                .unwrap_or_default()
                .iter()
                .find(|variant| variant.name == name)
                .expect("resolve_item_name found this event");

            let fields = variant
                .fields
                .iter()
                .map(|field| Field::from_field(field, types))
                .collect();

            (
                docs(&variant.docs),
                PalletItemShape::Event {
                    pallet_index: pallet.event_index(),
                    event_index: variant.index,
                    fields,
                },
            )
        }
        PalletItemKind::Error => {
            let variant = pallet
                .error_variants()
                .unwrap_or_default()
                .iter()
                .find(|variant| variant.name == name)
                .expect("resolve_item_name found this error");

            let fields = variant
                .fields
                .iter()
                .map(|field| Field::from_field(field, types))
                .collect();

            (
                docs(&variant.docs),
                PalletItemShape::Error {
                    pallet_index: pallet.error_index(),
                    error_index: variant.index,
                    fields,
                },
            )
        }
    };

    Ok(PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: String::from(name),
        docs,
        shape,
    })
}

fn resolve_pallet_name<'a>(loaded: &'a LoadedMetadata, name: &str) -> anyhow::Result<&'a str> {
    let metadata = &loaded.metadata;

    if let Some(pallet) = resolve_name(name, metadata.pallets().map(|pallet| pallet.name()))? {
        return Ok(pallet);
    }

    if let Some(runtime_api) =
        resolve_name(name, metadata.runtime_api_traits().map(|api| api.name()))?
    {
        bail!(
            "`{runtime_api}` is a runtime API, not a pallet; `get_runtime_api_method` reads its methods"
        );
    }

    bail!("no pallet named `{name}`; `list_pallets` lists the valid names");
}

fn resolve_item_name<'a>(
    pallet: &PalletMetadata<'a>,
    address: &PalletItemAddress,
    version: u32,
) -> anyhow::Result<&'a str> {
    if let Some(name) = resolve_name_for_kind(pallet, address.kind, &address.name)? {
        return Ok(name);
    }

    let mut others = Vec::new();

    for kind in PalletItemKind::ALL {
        if kind == address.kind {
            continue;
        }

        // Advisory: this only decides whether to offer a hint, so an unresolvable name
        // under a kind nobody asked about must not replace the answer above.
        if let Some(actual) = resolve_name_for_kind(pallet, kind, &address.name)
            .ok()
            .flatten()
        {
            others.push((kind, actual));
        }
    }

    bail!(
        "{}",
        missing_item_message(pallet.name(), address.kind, &address.name, &others, version)
    );
}

fn resolve_name_for_kind<'a>(
    pallet: &PalletMetadata<'a>,
    kind: PalletItemKind,
    name: &str,
) -> anyhow::Result<Option<&'a str>> {
    match kind {
        PalletItemKind::Call => resolve_name(
            name,
            pallet
                .call_variants()
                .unwrap_or_default()
                .iter()
                .map(|variant| variant.name.as_str()),
        ),
        PalletItemKind::ViewFunction => {
            resolve_name(name, pallet.view_functions().map(|view| view.name()))
        }
        PalletItemKind::Storage => resolve_name(
            name,
            pallet
                .storage()
                .map(|storage| storage.entries())
                .unwrap_or_default()
                .iter()
                .map(|entry| entry.name()),
        ),
        PalletItemKind::Constant => {
            resolve_name(name, pallet.constants().map(|constant| constant.name()))
        }
        PalletItemKind::Event => resolve_name(
            name,
            pallet
                .event_variants()
                .unwrap_or_default()
                .iter()
                .map(|variant| variant.name.as_str()),
        ),
        PalletItemKind::Error => resolve_name(
            name,
            pallet
                .error_variants()
                .unwrap_or_default()
                .iter()
                .map(|variant| variant.name.as_str()),
        ),
    }
}

fn missing_item_message(
    pallet: &str,
    kind: PalletItemKind,
    name: &str,
    others: &[(PalletItemKind, &str)],
    version: u32,
) -> String {
    let kind_name = kind.as_str();

    if !others.is_empty() {
        // The metadata's spelling, not the request's: this is the string the caller
        // will copy into the retry, and `name` below is whatever they typed.
        let others = others
            .iter()
            .map(|(kind, actual)| format!("`{}` has `{actual}`", kind.as_str()))
            .collect::<Vec<_>>()
            .join(", ");

        return format!("no `{kind_name}` named `{name}` in pallet `{pallet}`, but {others}");
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
    use rmcp::serde_json::{json, to_value};

    use super::*;

    #[test]
    fn pallet_item_details_serializes_flat_and_omits_an_absent_default() {
        let details = PalletItemDetails {
            pallet: String::from("Staking"),
            name: String::from("ActiveEra"),
            docs: vec![String::from("The active era information.")],
            shape: PalletItemShape::Storage {
                keys: Vec::new(),
                value_type: String::from("ActiveEraInfo"),
                optional: true,
                default: None,
            },
        };

        assert_eq!(
            to_value(details).unwrap(),
            json!({
                "pallet": "Staking",
                "name": "ActiveEra",
                "docs": ["The active era information."],
                "kind": "storage",
                "keys": [],
                "value_type": "ActiveEraInfo",
                "optional": true
            })
        );
    }

    #[test]
    fn missing_item_message_names_the_other_kind() {
        assert_eq!(
            missing_item_message(
                "Balances",
                PalletItemKind::Call,
                "ExistentialDeposit",
                &[(PalletItemKind::Constant, "ExistentialDeposit")],
                16
            ),
            "no `call` named `ExistentialDeposit` in pallet `Balances`, but `constant` has `ExistentialDeposit`"
        );
    }

    #[test]
    fn missing_item_message_lists_several_other_kinds() {
        assert_eq!(
            missing_item_message(
                "System",
                PalletItemKind::Call,
                "account",
                &[
                    (PalletItemKind::Storage, "Account"),
                    (PalletItemKind::Event, "Account")
                ],
                16
            ),
            "no `call` named `account` in pallet `System`, but `storage` has `Account`, `event` has `Account`"
        );
    }

    #[test]
    fn missing_item_message_points_at_the_version_not_the_listing_before_v16() {
        assert_eq!(
            missing_item_message("Balances", PalletItemKind::ViewFunction, "get", &[], 15),
            "no `view_function` named `get` in pallet `Balances`; view functions need metadata v16; this metadata is v15, so no pallet has any"
        );
    }

    #[test]
    fn missing_item_message_does_not_note_the_view_function_version_on_v16() {
        assert_eq!(
            missing_item_message("Balances", PalletItemKind::ViewFunction, "get", &[], 16),
            "no `view_function` named `get` in pallet `Balances`; `list_pallet_items` lists them"
        );
    }

    #[test]
    fn hasher_names_match_frame() {
        assert_eq!(
            Hasher::from(StorageHasher::Blake2_128Concat),
            Hasher::Blake2_128Concat
        );
        assert_eq!(
            Hasher::from(StorageHasher::Twox64Concat),
            Hasher::Twox64Concat
        );
        assert_eq!(Hasher::from(StorageHasher::Identity), Hasher::Identity);
    }
}
