//! Reading one item of a pallet.

use anyhow::{Context, anyhow};
use rmcp::schemars::{self, JsonSchema};
use scale_info::{PortableRegistry, Variant, form::PortableForm};
use serde::{Deserialize, Serialize};
use subxt_metadata::{
    ConstantMetadata, PalletMetadata, StorageEntryMetadata, StorageHasher, ViewFunctionMetadata,
};

use super::{
    LoadedMetadata,
    item::{PalletItemKind, docs},
    lookup::{self, PalletItem, pallet_item_lookup_error_message},
    registry::{Field, render_value, type_name},
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    let (pallet, item) = lookup::pallet_item(
        &loaded.metadata,
        &address.pallet,
        address.kind,
        &address.name,
    )
    .map_err(|error| anyhow!(pallet_item_lookup_error_message(&error, loaded.version)))?;

    let types = loaded.metadata.types();

    match item {
        PalletItem::Call(variant) => Ok(call_details(&pallet, variant, types)),
        PalletItem::ViewFunction(view_function) => {
            Ok(view_function_details(&pallet, &view_function, types))
        }
        PalletItem::Storage(entry) => storage_details(&pallet, entry, types),
        PalletItem::Constant(constant) => constant_details(&pallet, constant, types),
        PalletItem::Event(variant) => Ok(event_details(&pallet, variant, types)),
        PalletItem::Error(variant) => Ok(error_details(&pallet, variant, types)),
    }
}

fn call_details(
    pallet: &PalletMetadata<'_>,
    variant: &Variant<PortableForm>,
    types: &PortableRegistry,
) -> PalletItemDetails {
    PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: variant.name.clone(),
        docs: docs(&variant.docs),
        shape: PalletItemShape::Call {
            pallet_index: pallet.call_index(),
            call_index: variant.index,
            arguments: variant_fields(variant, types),
        },
    }
}

fn view_function_details(
    pallet: &PalletMetadata<'_>,
    view_function: &ViewFunctionMetadata<'_>,
    types: &PortableRegistry,
) -> PalletItemDetails {
    let arguments = view_function
        .inputs()
        .map(|input| Field::from_view_function_input(input, types))
        .collect();

    PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: String::from(view_function.name()),
        docs: docs(view_function.docs()),
        shape: PalletItemShape::ViewFunction {
            arguments,
            returns: type_name(view_function.output_ty(), types),
        },
    }
}

fn storage_details(
    pallet: &PalletMetadata<'_>,
    entry: &StorageEntryMetadata,
    types: &PortableRegistry,
) -> anyhow::Result<PalletItemDetails> {
    let keys = entry
        .keys()
        .map(|key| StorageKey {
            hasher: Hasher::from(key.hasher),
            ty: type_name(key.key_id, types),
        })
        .collect();

    let value_type = type_name(entry.value_ty(), types);

    let default = entry
        .default_value()
        .map(|bytes| {
            render_value(bytes, entry.value_ty(), types).with_context(|| {
                format!(
                    "cannot decode the default of storage entry `{}` in pallet `{}` as `{value_type}`",
                    entry.name(),
                    pallet.name(),
                )
            })
        })
        .transpose()?;

    Ok(PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: String::from(entry.name()),
        docs: docs(entry.docs()),
        shape: PalletItemShape::Storage {
            keys,
            value_type,
            optional: entry.default_value().is_none(),
            default,
        },
    })
}

fn constant_details(
    pallet: &PalletMetadata<'_>,
    constant: &ConstantMetadata,
    types: &PortableRegistry,
) -> anyhow::Result<PalletItemDetails> {
    let ty = type_name(constant.ty(), types);

    let value = render_value(constant.value(), constant.ty(), types).with_context(|| {
        format!(
            "cannot decode the value of constant `{}` in pallet `{}` as `{ty}`",
            constant.name(),
            pallet.name(),
        )
    })?;

    Ok(PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: String::from(constant.name()),
        docs: docs(constant.docs()),
        shape: PalletItemShape::Constant { ty, value },
    })
}

fn event_details(
    pallet: &PalletMetadata<'_>,
    variant: &Variant<PortableForm>,
    types: &PortableRegistry,
) -> PalletItemDetails {
    PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: variant.name.clone(),
        docs: docs(&variant.docs),
        shape: PalletItemShape::Event {
            pallet_index: pallet.event_index(),
            event_index: variant.index,
            fields: variant_fields(variant, types),
        },
    }
}

fn error_details(
    pallet: &PalletMetadata<'_>,
    variant: &Variant<PortableForm>,
    types: &PortableRegistry,
) -> PalletItemDetails {
    PalletItemDetails {
        pallet: String::from(pallet.name()),
        name: variant.name.clone(),
        docs: docs(&variant.docs),
        shape: PalletItemShape::Error {
            pallet_index: pallet.error_index(),
            error_index: variant.index,
            fields: variant_fields(variant, types),
        },
    }
}

fn variant_fields(variant: &Variant<PortableForm>, types: &PortableRegistry) -> Vec<Field> {
    variant
        .fields
        .iter()
        .map(|field| Field::from_field(field, types))
        .collect()
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
