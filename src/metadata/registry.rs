//! Reading the type registry: a type id as a name, a registry field as a `Field`.

use frame_decode::runtime_apis::RuntimeApiInput;
use rmcp::schemars::{self, JsonSchema};
use scale_info::{PortableRegistry, TypeDef, TypeDefPrimitive, TypeParameter, form::PortableForm};
use serde::Serialize;

/// A field of a call, event, error, view function or runtime API method: a name
/// if it has one, the type as the registry names it, and the field's own
/// documentation.
#[derive(Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Field {
    /// The field's name as the metadata spells it. Absent for an unnamed field --
    /// events in particular often have those. Pass it on verbatim; do not invent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub name: Option<String>,
    /// The field's type, rendered from the registry: a primitive (`u32`, `bool`,
    /// `str`), a sequence (`Vec<T>`), an array (`[T; 32]`), a tuple (`(A, B)`),
    /// `Compact<T>`, `BitVec`, or a named type with its parameters
    /// (`MultiAddress<AccountId32, ()>`). The last path segment only, not the
    /// module path. This is not the source text on the field (`T::AccountId`);
    /// that is unresolved and often missing.
    #[serde(rename = "type")]
    pub ty: String,
    /// The field's own documentation, uncut, one string per line, with the leading
    /// space FRAME leaves after `///` stripped. Empty when the field has none --
    /// runtime API method arguments never do.
    pub docs: Vec<String>,
}

impl Field {
    pub fn from_runtime_api_input(
        input: &RuntimeApiInput<'_, u32>,
        types: &PortableRegistry,
    ) -> Self {
        Self {
            name: Some(String::from(input.name.as_ref())),
            ty: type_name(input.id, types),
            docs: Vec::new(),
        }
    }
}

/// Walks parameters and sequence/array/tuple/compact elements, never the fields
/// of a named type: v14+ type graphs do not cycle along that route, and the
/// depth cap is only a backstop. Returns a stub with the id when the type is
/// missing, unnamed and unclassifiable, or nested past the cap.
// Hand-rolled: `scale-typegen`, the one crate that turns a type id into a name, does
// it for codegen -- full module paths, `syn` tokens, and a panic where we want a stub.
pub fn type_name(id: u32, types: &PortableRegistry) -> String {
    type_name_at(id, types, MAX_DEPTH)
}

const MAX_DEPTH: u8 = 32;

fn type_name_at(id: u32, types: &PortableRegistry, depth: u8) -> String {
    let Some(depth) = depth.checked_sub(1) else {
        return stub(id);
    };

    let Some(ty) = types.resolve(id) else {
        return stub(id);
    };

    match &ty.type_def {
        TypeDef::Primitive(primitive) => String::from(primitive_name(primitive)),
        TypeDef::Sequence(sequence) => {
            format!(
                "Vec<{}>",
                type_name_at(sequence.type_param.id, types, depth)
            )
        }
        TypeDef::Array(array) => {
            format!(
                "[{}; {}]",
                type_name_at(array.type_param.id, types, depth),
                array.len
            )
        }
        TypeDef::Tuple(tuple) => tuple_of(tuple.fields.iter().map(|field| field.id), types, depth),
        TypeDef::Compact(compact) => {
            format!(
                "Compact<{}>",
                type_name_at(compact.type_param.id, types, depth)
            )
        }
        TypeDef::BitSequence(_) => String::from("BitVec"),
        TypeDef::Composite(composite) => match ty.path.ident() {
            Some(ident) => named(&ident, &ty.type_params, types, depth),
            None => tuple_of(
                composite.fields.iter().map(|field| field.ty.id),
                types,
                depth,
            ),
        },
        TypeDef::Variant(_) => match ty.path.ident() {
            Some(ident) => named(&ident, &ty.type_params, types, depth),
            None => stub(id),
        },
    }
}

fn named(
    ident: &str,
    params: &[TypeParameter<PortableForm>],
    types: &PortableRegistry,
    depth: u8,
) -> String {
    let args: Vec<String> = params
        .iter()
        .filter_map(|param| {
            param
                .ty
                .as_ref()
                .map(|ty| type_name_at(ty.id, types, depth))
        })
        .collect();

    if args.is_empty() {
        String::from(ident)
    } else {
        format!("{ident}<{}>", args.join(", "))
    }
}

fn tuple_of(ids: impl IntoIterator<Item = u32>, types: &PortableRegistry, depth: u8) -> String {
    let names: Vec<String> = ids
        .into_iter()
        .map(|id| type_name_at(id, types, depth))
        .collect();

    format!("({})", names.join(", "))
}

fn primitive_name(primitive: &TypeDefPrimitive) -> &'static str {
    match primitive {
        TypeDefPrimitive::Bool => "bool",
        TypeDefPrimitive::Char => "char",
        TypeDefPrimitive::Str => "str",
        TypeDefPrimitive::U8 => "u8",
        TypeDefPrimitive::U16 => "u16",
        TypeDefPrimitive::U32 => "u32",
        TypeDefPrimitive::U64 => "u64",
        TypeDefPrimitive::U128 => "u128",
        TypeDefPrimitive::U256 => "u256",
        TypeDefPrimitive::I8 => "i8",
        TypeDefPrimitive::I16 => "i16",
        TypeDefPrimitive::I32 => "i32",
        TypeDefPrimitive::I64 => "i64",
        TypeDefPrimitive::I128 => "i128",
        TypeDefPrimitive::I256 => "i256",
    }
}

fn stub(id: u32) -> String {
    format!("#{id}")
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, collections::BTreeMap};

    use frame_decode::runtime_apis::RuntimeApiInput;
    use scale_info::{MetaType, PortableRegistry, Registry, TypeInfo, scale::Compact};

    use super::*;

    fn registry_and_id<T: TypeInfo + 'static>() -> (PortableRegistry, u32) {
        let mut registry = Registry::new();
        let id = registry.register_type(&MetaType::new::<T>()).id;

        (registry.into(), id)
    }

    fn assert_type_name<T: TypeInfo + 'static>(expected: &str) {
        let (types, id) = registry_and_id::<T>();

        assert_eq!(type_name(id, &types), expected);
    }

    #[test]
    fn type_name_of_sequence_is_vec() {
        assert_type_name::<Vec<u8>>("Vec<u8>");
    }

    #[test]
    fn type_name_of_array() {
        assert_type_name::<[u8; 32]>("[u8; 32]");
    }

    #[test]
    fn type_name_of_tuple() {
        assert_type_name::<(u32, bool)>("(u32, bool)");
    }

    #[test]
    fn type_name_of_option() {
        assert_type_name::<Option<u32>>("Option<u32>");
    }

    #[test]
    fn type_name_of_result() {
        assert_type_name::<Result<u32, bool>>("Result<u32, bool>");
    }

    #[test]
    fn type_name_of_compact() {
        assert_type_name::<Compact<u64>>("Compact<u64>");
    }

    #[test]
    fn type_name_of_btreemap() {
        assert_type_name::<BTreeMap<u32, u64>>("BTreeMap<u32, u64>");
    }

    #[test]
    fn type_name_of_unit_is_empty_tuple() {
        assert_type_name::<()>("()");
    }

    #[test]
    fn type_name_stubs_unknown_id() {
        let types = PortableRegistry { types: Vec::new() };

        assert_eq!(type_name(42, &types), "#42");
    }

    #[test]
    fn field_from_runtime_api_input() {
        let (types, id) = registry_and_id::<u32>();
        let input = RuntimeApiInput {
            name: Cow::Borrowed("at"),
            id,
        };

        assert_eq!(
            Field::from_runtime_api_input(&input, &types),
            Field {
                name: Some(String::from("at")),
                ty: String::from("u32"),
                docs: Vec::new(),
            }
        );
    }

    #[test]
    fn field_from_runtime_api_input_renders_the_type() {
        let (types, id) = registry_and_id::<Vec<u8>>();
        let input = RuntimeApiInput {
            name: Cow::Borrowed("data"),
            id,
        };

        assert_eq!(Field::from_runtime_api_input(&input, &types).ty, "Vec<u8>");
    }
}
