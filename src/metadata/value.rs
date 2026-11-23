//! SCALE bytes as JSON, decoded by their type.
//!
//! scale-value decodes the bytes, but its value cannot say what the JSON should be: it widens
//! every integer to 128 bits, reads bytes as a list of numbers, and keeps a newtype's one field
//! as a one-element list. What it does keep is each node's type id, and the type picks the
//! node's `Repr`. `decode` hands the root to `to_json`, which applies the node's `Repr` and
//! otherwise writes the node as decoded.

use core::fmt;

use rmcp::serde_json::{Map, Value as Json};
use scale_info::{
    Path, PortableRegistry, TypeDef, TypeDefArray, TypeDefPrimitive, TypeDefSequence,
    form::PortableForm,
};
use scale_value::{Composite, Primitive, ValueDef, Variant};

/// A node of the tree scale-value decodes; its `context` is the id of its type.
type Node = scale_value::Value<u32>;

/// How a type is represented in JSON, chosen by the type alone.
#[derive(Debug, Clone, Copy)]
enum Repr {
    /// `u8` to `u32`, `i8` to `i32`: a number.
    Number,
    /// Wider integers, `primitive_types::U256` among them: a decimal string. Parsers that read
    /// JSON numbers as doubles, as JavaScript and jq do, round them past 2^53. `signed` is the
    /// type's, since scale-value loses the sign of an `i256`.
    Decimal { signed: bool },
    /// `Vec<u8>` and `[u8; N]`: a hex string.
    Hex,
    /// A newtype, a type with one unnamed field: that field, as `#[serde(transparent)]` writes it.
    Transparent,
    /// Anything else: the node as decoded, whose own shape already fits JSON.
    AsDecoded,
}

impl Repr {
    fn from_type(id: u32, types: &PortableRegistry) -> Self {
        let Some(ty) = types.resolve(id) else {
            return Self::AsDecoded;
        };

        match &ty.type_def {
            TypeDef::Primitive(primitive) => Self::from_primitive(primitive),
            TypeDef::Sequence(TypeDefSequence { type_param, .. })
            | TypeDef::Array(TypeDefArray { type_param, .. })
                if Self::is_primitive(type_param.id, TypeDefPrimitive::U8, types) =>
            {
                Self::Hex
            }
            TypeDef::Composite(composite) => match composite.fields.as_slice() {
                [field] if field.name.is_none() && Self::is_big_uint(&ty.path, field, types) => {
                    Self::Decimal { signed: false }
                }
                [field] if field.name.is_none() => Self::Transparent,
                _ => Self::AsDecoded,
            },
            _ => Self::AsDecoded,
        }
    }

    fn from_primitive(primitive: &TypeDefPrimitive) -> Self {
        match primitive {
            TypeDefPrimitive::U8
            | TypeDefPrimitive::U16
            | TypeDefPrimitive::U32
            | TypeDefPrimitive::I8
            | TypeDefPrimitive::I16
            | TypeDefPrimitive::I32 => Self::Number,
            TypeDefPrimitive::U64 | TypeDefPrimitive::U128 | TypeDefPrimitive::U256 => {
                Self::Decimal { signed: false }
            }
            TypeDefPrimitive::I64 | TypeDefPrimitive::I128 | TypeDefPrimitive::I256 => {
                Self::Decimal { signed: true }
            }
            TypeDefPrimitive::Bool | TypeDefPrimitive::Char | TypeDefPrimitive::Str => {
                Self::AsDecoded
            }
        }
    }

    /// `None` for `AsDecoded`, and for a node that is not what its type promised, such as a
    /// byte that is not one: the caller then writes the node as decoded.
    fn apply(self, node: &Node, types: &PortableRegistry) -> Option<Json> {
        match self {
            Self::Number => Self::number(node),
            Self::Decimal { signed } => Self::decimal(node, signed),
            Self::Hex => Self::hex(node),
            Self::Transparent => Self::transparent(node, types),
            Self::AsDecoded => None,
        }
    }

    fn is_primitive(id: u32, primitive: TypeDefPrimitive, types: &PortableRegistry) -> bool {
        types
            .resolve(id)
            .is_some_and(|ty| matches!(&ty.type_def, TypeDef::Primitive(def) if *def == primitive))
    }

    /// `primitive_types::U256` and its siblings keep an integer as `[u64; N]`, least significant
    /// first. As a plain newtype the value would read as N numbers.
    fn is_big_uint(
        path: &Path<PortableForm>,
        field: &scale_info::Field<PortableForm>,
        types: &PortableRegistry,
    ) -> bool {
        // scale-info has no big integers: such a type reaches the registry as a newtype over
        // limbs, and only its path and width tell it apart from any other.
        const BIG_UINTS: [(&str, u32); 3] = [("U128", 2), ("U256", 4), ("U512", 8)];

        let [module, name] = path.segments.as_slice() else {
            return false;
        };

        let Some(TypeDef::Array(limbs)) = types.resolve(field.ty.id).map(|ty| &ty.type_def) else {
            return false;
        };

        module == "primitive_types"
            && BIG_UINTS.contains(&(name.as_str(), limbs.len))
            && Self::is_primitive(limbs.type_param.id, TypeDefPrimitive::U64, types)
    }

    fn number(node: &Node) -> Option<Json> {
        match node.value {
            ValueDef::Primitive(Primitive::U128(int)) => u32::try_from(int).ok().map(Json::from),
            ValueDef::Primitive(Primitive::I128(int)) => i32::try_from(int).ok().map(Json::from),
            _ => None,
        }
    }

    fn decimal(node: &Node, signed: bool) -> Option<Json> {
        let text = match &node.value {
            ValueDef::Primitive(Primitive::U128(int)) => int.to_string(),
            ValueDef::Primitive(Primitive::I128(int)) => int.to_string(),
            // scale-value 0.18.2 decodes an `i256` as `Primitive::U256`.
            ValueDef::Primitive(Primitive::U256(bytes) | Primitive::I256(bytes)) => {
                let mut int = BigUint::from_le_bytes(bytes);

                if signed && bytes[31] & 0x80 != 0 {
                    int.negate();
                    format!("-{int}")
                } else {
                    int.to_string()
                }
            }
            // `primitive_types::U256` and its siblings.
            _ => match items(node)? {
                [limbs] => BigUint::from_limbs(limbs)?.to_string(),
                _ => return None,
            },
        };

        Some(Json::String(text))
    }

    fn hex(node: &Node) -> Option<Json> {
        let bytes = items(node)?
            .iter()
            .map(|item| u8::try_from(unsigned(item)?).ok())
            .collect::<Option<Vec<u8>>>()?;

        Some(Json::String(format!("0x{}", hex::encode(bytes))))
    }

    fn transparent(node: &Node, types: &PortableRegistry) -> Option<Json> {
        let [field] = items(node)? else {
            return None;
        };

        Some(to_json(field, types))
    }
}

/// An unsigned integer wider than `u128`, as `u64` limbs, least significant first.
struct BigUint(Vec<u64>);

impl BigUint {
    fn from_le_bytes(bytes: &[u8; 32]) -> Self {
        Self(
            bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|chunk| u64::from_le_bytes(*chunk))
                .collect(),
        )
    }

    /// The `[u64; N]` field of `primitive_types::U256` and its siblings.
    fn from_limbs(limbs: &Node) -> Option<Self> {
        items(limbs)?
            .iter()
            .map(|limb| u64::try_from(unsigned(limb)?).ok())
            .collect::<Option<_>>()
            .map(Self)
    }

    /// Two's complement: the magnitude of a negative value is its inverse plus one.
    fn negate(&mut self) {
        let mut carry = true;

        for limb in &mut self.0 {
            (*limb, carry) = (!*limb).overflowing_add(u64::from(carry));
        }
    }
}

/// Decimal, by long division by 10^19, the largest power of ten in a `u64`: nineteen digits at
/// a time.
impl fmt::Display for BigUint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const CHUNK: u128 = 10_000_000_000_000_000_000;

        let mut limbs = self.0.clone();
        let mut chunks = Vec::new();

        while limbs.iter().any(|&limb| limb != 0) {
            let mut remainder = 0;

            for limb in limbs.iter_mut().rev() {
                let current = (remainder << 64) | u128::from(*limb);
                // `remainder` is below 10^19, so the quotient is below 2^64.
                *limb = (current / CHUNK) as u64;
                remainder = current % CHUNK;
            }

            chunks.push(remainder);
        }

        let Some(first) = chunks.pop() else {
            return f.write_str("0");
        };

        write!(f, "{first}")?;

        for chunk in chunks.iter().rev() {
            write!(f, "{chunk:019}")?;
        }

        Ok(())
    }
}

/// Decode SCALE bytes as the type `id`, into JSON. `None` only when the bytes are not exactly
/// one value of that type -- leftover bytes mean the type was wrong, not that a value was read.
pub fn decode(bytes: &[u8], id: u32, types: &PortableRegistry) -> Option<Json> {
    let mut cursor = bytes;
    let root = scale_value::scale::decode_as_type(&mut cursor, id, types).ok()?;

    cursor.is_empty().then(|| to_json(&root, types))
}

fn to_json(node: &Node, types: &PortableRegistry) -> Json {
    Repr::from_type(node.context, types)
        .apply(node, types)
        .unwrap_or_else(|| as_decoded(node, types))
}

/// An integer lands here only when its type could not say its width, and a decimal string loses
/// nothing.
fn as_decoded(node: &Node, types: &PortableRegistry) -> Json {
    match &node.value {
        ValueDef::Primitive(Primitive::Bool(bool)) => Json::Bool(*bool),
        ValueDef::Primitive(Primitive::Char(char)) => Json::String(char.to_string()),
        ValueDef::Primitive(Primitive::String(string)) => Json::String(string.clone()),
        ValueDef::Primitive(Primitive::U128(int)) => Json::String(int.to_string()),
        ValueDef::Primitive(Primitive::I128(int)) => Json::String(int.to_string()),
        ValueDef::Primitive(Primitive::U256(bytes) | Primitive::I256(bytes)) => {
            Json::String(BigUint::from_le_bytes(bytes).to_string())
        }
        ValueDef::BitSequence(bits) => bits.iter().map(Json::Bool).collect(),
        ValueDef::Variant(variant) => tagged(variant, types),
        ValueDef::Composite(Composite::Named(fields)) => object(fields, types),
        ValueDef::Composite(Composite::Unnamed(items)) => array(items, types),
    }
}

/// As serde writes an externally tagged enum: the name alone for a variant without fields, and
/// otherwise an object whose one key is the name.
fn tagged(variant: &Variant<u32>, types: &PortableRegistry) -> Json {
    let fields = match &variant.values {
        // scale-value decodes a variant without fields as `Unnamed`, so `Named` always has some.
        Composite::Named(fields) => object(fields, types),
        Composite::Unnamed(items) => match items.as_slice() {
            [] => return Json::String(variant.name.clone()),
            [item] => to_json(item, types),
            items => array(items, types),
        },
    };

    Json::Object(Map::from_iter([(variant.name.clone(), fields)]))
}

fn object(fields: &[(String, Node)], types: &PortableRegistry) -> Json {
    Json::Object(
        fields
            .iter()
            .map(|(name, field)| (name.clone(), to_json(field, types)))
            .collect(),
    )
}

fn array(items: &[Node], types: &PortableRegistry) -> Json {
    items.iter().map(|item| to_json(item, types)).collect()
}

/// The items of an unnamed composite: a tuple, a sequence, an array, a newtype.
fn items(node: &Node) -> Option<&[Node]> {
    match &node.value {
        ValueDef::Composite(Composite::Unnamed(items)) => Some(items),
        _ => None,
    }
}

/// An unsigned integer of any width: scale-value widens them all to 128 bits.
fn unsigned(node: &Node) -> Option<u128> {
    match node.value {
        ValueDef::Primitive(Primitive::U128(int)) => Some(int),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use parity_scale_codec::{Compact, Encode};
    use rmcp::serde_json::json;
    use scale_info::{
        MetaType, Path, Registry, Type, TypeDefBitSequence, TypeDefPrimitive, TypeInfo,
        build::Fields,
    };

    use super::*;

    fn decode_as<T: TypeInfo + 'static>(bytes: &[u8]) -> Option<Json> {
        let mut registry = Registry::new();
        let id = registry.register_type(&MetaType::new::<T>()).id;

        decode(bytes, id, &registry.into())
    }

    fn json_of<T: TypeInfo + Encode + 'static>(value: &T) -> Json {
        decode_as::<T>(&value.encode()).expect("an encoded value decodes")
    }

    /// A newtype over `[u64; 4]` at the path `module::name`.
    fn limbs_type(name: &'static str, module: &'static str) -> Type {
        Type::builder()
            .path(Path::new(name, module))
            .composite(Fields::unnamed().field(|field| field.ty::<[u64; 4]>()))
    }

    // Only its `primitive_types` path makes this an integer, and a derive would give it this
    // module's path.
    #[derive(Encode)]
    struct U256([u64; 4]);

    impl TypeInfo for U256 {
        type Identity = Self;

        fn type_info() -> Type {
            limbs_type("U256", "primitive_types")
        }
    }

    // The same limbs in another crate, under another name, and under the name of another width:
    // all plain newtypes.
    #[derive(Encode)]
    struct OtherCrateU256([u64; 4]);

    impl TypeInfo for OtherCrateU256 {
        type Identity = Self;

        fn type_info() -> Type {
            limbs_type("U256", "other_crate")
        }
    }

    #[derive(Encode)]
    struct X256([u64; 4]);

    impl TypeInfo for X256 {
        type Identity = Self;

        fn type_info() -> Type {
            limbs_type("X256", "primitive_types")
        }
    }

    #[derive(Encode)]
    struct WideU128([u64; 4]);

    impl TypeInfo for WideU128 {
        type Identity = Self;

        fn type_info() -> Type {
            limbs_type("U128", "primitive_types")
        }
    }

    #[derive(Encode, TypeInfo)]
    struct Newtype(u32);

    #[derive(Encode, TypeInfo)]
    struct Account([u8; 4]);

    struct PrimitiveU256;

    impl TypeInfo for PrimitiveU256 {
        type Identity = Self;

        fn type_info() -> Type {
            TypeDefPrimitive::U256.into()
        }
    }

    struct PrimitiveI256;

    impl TypeInfo for PrimitiveI256 {
        type Identity = Self;

        fn type_info() -> Type {
            TypeDefPrimitive::I256.into()
        }
    }

    struct Lsb0;

    impl TypeInfo for Lsb0 {
        type Identity = Self;

        fn type_info() -> Type {
            Type::builder()
                .path(Path::new("Lsb0", "bitvec::order"))
                .composite(Fields::unit())
        }
    }

    struct Bits;

    impl TypeInfo for Bits {
        type Identity = Self;

        fn type_info() -> Type {
            TypeDefBitSequence::new::<u8, Lsb0>().into()
        }
    }

    #[derive(Encode, TypeInfo)]
    enum Choice {
        Unit,
        One(u32),
        Two(u8, u8),
        Named { width: u8 },
    }

    #[derive(Encode, TypeInfo)]
    struct Named {
        inner: u32,
    }

    #[derive(Encode, TypeInfo)]
    struct Unordered {
        second: u8,
        first: bool,
    }

    #[test]
    fn narrow_integers_are_numbers() {
        assert_eq!(json_of(&7u8), json!(7));
        assert_eq!(json_of(&7u16), json!(7));
        assert_eq!(json_of(&u32::MAX), json!(u32::MAX));
        assert_eq!(json_of(&-7i8), json!(-7));
        assert_eq!(json_of(&-7i16), json!(-7));
        assert_eq!(json_of(&-7i32), json!(-7));
    }

    #[test]
    fn wide_integers_are_decimal_strings() {
        assert_eq!(json_of(&u64::MAX), json!("18446744073709551615"));
        assert_eq!(json_of(&7u64), json!("7"));
        assert_eq!(json_of(&u128::MAX), json!(u128::MAX.to_string()));
        assert_eq!(json_of(&-7i64), json!("-7"));
        assert_eq!(json_of(&i128::MIN), json!(i128::MIN.to_string()));
    }

    #[test]
    fn compact_takes_width_of_inner_type() {
        assert_eq!(json_of(&Compact(7u32)), json!(7));
        assert_eq!(json_of(&Compact(7u64)), json!("7"));
    }

    #[test]
    fn primitive_types_integer_is_decimal_string() {
        assert_eq!(
            json_of(&U256([800_000_000_000, 0, 0, 0])),
            json!("800000000000")
        );
        assert_eq!(
            json_of(&U256([u64::MAX, 1, 0, 0])),
            json!("36893488147419103231")
        );
        assert_eq!(json_of(&U256([0; 4])), json!("0"));
        // The lower nineteen digits are all zeros, which only the padding keeps.
        assert_eq!(
            json_of(&U256([10_000_000_000_000_000_000, 0, 0, 0])),
            json!("10000000000000000000")
        );
        assert_eq!(
            json_of(&U256([u64::MAX; 4])),
            json!("115792089237316195423570985008687907853269984665640564039457584007913129639935")
        );
    }

    #[test]
    fn u256_lookalikes_are_newtypes() {
        let limbs = json!(["1", "0", "0", "0"]);

        assert_eq!(json_of(&OtherCrateU256([1, 0, 0, 0])), limbs);
        assert_eq!(json_of(&X256([1, 0, 0, 0])), limbs);
        assert_eq!(json_of(&WideU128([1, 0, 0, 0])), limbs);
    }

    #[test]
    fn bytes_are_hex() {
        assert_eq!(json_of(&vec![0xdeu8, 0xad]), json!("0xdead"));
        assert_eq!(json_of(&[1u8, 2, 3, 4]), json!("0x01020304"));
        assert_eq!(json_of(&Vec::<u8>::new()), json!("0x"));
    }

    #[test]
    fn newtype_is_its_field() {
        assert_eq!(json_of(&Newtype(5)), json!(5));
        assert_eq!(json_of(&Account([1, 2, 3, 4])), json!("0x01020304"));
    }

    // No Rust type in a v14+ registry maps to these primitives -- `primitive_types::U256` is a
    // composite -- but the metadata format can carry them.
    #[test]
    fn u256_and_i256_are_decimal_strings() {
        let mut five = [0u8; 32];
        five[0] = 5;

        assert_eq!(decode_as::<PrimitiveU256>(&five), Some(json!("5")));
        assert_eq!(decode_as::<PrimitiveI256>(&five), Some(json!("5")));
        // The same bytes: the top bit is a sign only when the type is signed.
        assert_eq!(
            decode_as::<PrimitiveU256>(&[0xff; 32]),
            Some(json!(
                "115792089237316195423570985008687907853269984665640564039457584007913129639935"
            ))
        );
        assert_eq!(decode_as::<PrimitiveI256>(&[0xff; 32]), Some(json!("-1")));

        let mut min = [0u8; 32];
        min[31] = 0x80;

        assert_eq!(
            decode_as::<PrimitiveI256>(&min),
            Some(json!(
                "-57896044618658097711785492504343953926634992332820282019728792003956564819968"
            ))
        );
    }

    #[test]
    fn leftover_bytes_are_rejected() {
        let mut bytes = 42u32.encode();
        bytes.push(0);

        assert_eq!(decode_as::<u32>(&bytes), None);
    }

    #[test]
    fn undecodable_bytes_are_rejected() {
        assert_eq!(decode_as::<u32>(&[]), None);
    }

    #[test]
    fn char_and_str_are_strings() {
        // `char` has no `Encode`; SCALE writes it as a `u32`.
        assert_eq!(
            decode_as::<char>(&u32::from('a').encode()),
            Some(json!("a"))
        );
        assert_eq!(json_of(&String::from("polkadot")), json!("polkadot"));
    }

    #[test]
    fn bit_sequence_is_array_of_booleans() {
        // Three bits, then one byte that stores them least significant first.
        let bytes = [Compact(3u32).encode(), vec![0b011]].concat();

        assert_eq!(decode_as::<Bits>(&bytes), Some(json!([true, true, false])));
    }

    #[test]
    fn variant_without_fields_is_its_name() {
        assert_eq!(json_of(&Choice::Unit), json!("Unit"));
        assert_eq!(json_of(&None::<u8>), json!("None"));
    }

    #[test]
    fn variant_with_fields_is_single_key_object() {
        assert_eq!(json_of(&Choice::One(5)), json!({ "One": 5 }));
        assert_eq!(json_of(&Choice::Two(1, 2)), json!({ "Two": [1, 2] }));
        assert_eq!(
            json_of(&Choice::Named { width: 3 }),
            json!({ "Named": { "width": 3 } })
        );
        assert_eq!(json_of(&Some(5u8)), json!({ "Some": 5 }));
    }

    #[test]
    fn single_named_field_stays_object() {
        assert_eq!(json_of(&Named { inner: 5 }), json!({ "inner": 5 }));
    }

    #[test]
    fn struct_keeps_field_order() {
        let value = json_of(&Unordered {
            second: 1,
            first: true,
        });

        assert_eq!(value.to_string(), r#"{"second":1,"first":true}"#);
    }

    #[test]
    fn tuples_and_unit_are_arrays() {
        assert_eq!(json_of(&(1u8, true)), json!([1, true]));
        assert_eq!(json_of(&[1u16, 2]), json!([1, 2]));
        assert_eq!(json_of(&()), json!([]));
    }

    #[test]
    fn map_is_array_of_pairs() {
        let map = BTreeMap::from([(1u32, true), (2, false)]);

        assert_eq!(json_of(&map), json!([[1, true], [2, false]]));
    }
}
