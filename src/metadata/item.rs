//! The items of the metadata, and one walk over all of them.
//!
//! Every tool that answers with an item answers with the same address -- `kind`,
//! `namespace`, `name` -- so that a model can hand it on to another tool unchanged.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use subxt_metadata::Metadata;

/// A pallet, a runtime API trait, or an item inside one of them.
///
/// `runtime_api` is a trait of host-callable runtime methods; `runtime_api_method` is
/// one method of that trait; `view_function` is a read-only query on a pallet.
// `Ord` is derived from declaration order: `search` sorts equally relevant hits by it,
// and `for_each_item` visits a pallet's members in it, so reordering the variants
// reorders both.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Pallet,
    RuntimeApi,
    Call,
    RuntimeApiMethod,
    ViewFunction,
    Storage,
    Constant,
    Event,
    Error,
}

/// One item of the metadata: its address -- `kind`, `namespace`, `name` -- and the
/// first line of its documentation.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Item {
    /// What the item is. It also fixes what `namespace` is: a runtime API trait for
    /// `runtime_api_method`, a pallet for every other kind that has one.
    pub kind: ItemKind,
    /// The pallet or runtime API trait the item belongs to -- which of the two is given
    /// by `kind`. Absent for a pallet or a trait itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub namespace: Option<String>,
    /// The item's own name, exactly as in the metadata -- without the pallet or
    /// trait it sits in. Pass it on verbatim; do not normalize case.
    pub name: String,
    /// The first line of the item's own documentation, truncated. Absent when the item has
    /// no documentation of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub summary: Option<String>,
}

impl Item {
    pub fn new(item: &ItemRef<'_>) -> Self {
        Self {
            kind: item.kind,
            namespace: item.namespace.map(String::from),
            name: String::from(item.name),
            summary: summary(item.docs),
        }
    }
}

/// An item as the walk sees it: borrowed from the metadata, docs in full.
pub struct ItemRef<'a> {
    pub kind: ItemKind,
    pub namespace: Option<&'a str>,
    pub name: &'a str,
    pub docs: &'a [String],
}

/// Adding an `ItemKind` means adding a loop here and touching nothing else: no code
/// downstream has a case per kind. A kind the metadata version predates simply yields
/// nothing -- view functions before v16, runtime APIs before v15 -- so nothing checks the
/// version.
///
/// A pallet's members come in `ItemKind` order, so a listing that keeps the walk's
/// order is grouped by kind.
///
/// `ItemRef` lives only for the call: subxt ties a runtime API trait's docs to the
/// accessor's borrow rather than to the metadata, so nothing longer can be promised.
pub fn for_each_item(metadata: &Metadata, mut visit: impl FnMut(ItemRef<'_>)) {
    for pallet in metadata.pallets() {
        let namespace = pallet.name();

        visit(ItemRef {
            kind: ItemKind::Pallet,
            namespace: None,
            name: namespace,
            docs: pallet.docs(),
        });

        for variant in pallet.call_variants().unwrap_or_default() {
            visit(ItemRef {
                kind: ItemKind::Call,
                namespace: Some(namespace),
                name: &variant.name,
                docs: &variant.docs,
            });
        }

        for view_function in pallet.view_functions() {
            visit(ItemRef {
                kind: ItemKind::ViewFunction,
                namespace: Some(namespace),
                name: view_function.name(),
                docs: view_function.docs(),
            });
        }

        let entries = pallet.storage().map(|storage| storage.entries());

        for entry in entries.unwrap_or_default() {
            visit(ItemRef {
                kind: ItemKind::Storage,
                namespace: Some(namespace),
                name: entry.name(),
                docs: entry.docs(),
            });
        }

        for constant in pallet.constants() {
            visit(ItemRef {
                kind: ItemKind::Constant,
                namespace: Some(namespace),
                name: constant.name(),
                docs: constant.docs(),
            });
        }

        for variant in pallet.event_variants().unwrap_or_default() {
            visit(ItemRef {
                kind: ItemKind::Event,
                namespace: Some(namespace),
                name: &variant.name,
                docs: &variant.docs,
            });
        }

        for variant in pallet.error_variants().unwrap_or_default() {
            visit(ItemRef {
                kind: ItemKind::Error,
                namespace: Some(namespace),
                name: &variant.name,
                docs: &variant.docs,
            });
        }
    }

    for api in metadata.runtime_api_traits() {
        let namespace = api.name();

        visit(ItemRef {
            kind: ItemKind::RuntimeApi,
            namespace: None,
            name: namespace,
            docs: api.docs(),
        });

        for method in api.methods() {
            visit(ItemRef {
                kind: ItemKind::RuntimeApiMethod,
                namespace: Some(namespace),
                name: method.name(),
                docs: method.docs(),
            });
        }
    }
}

const SUMMARY_CHARS: usize = 200;

fn summary(docs: &[String]) -> Option<String> {
    let line = docs
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())?;

    match line.char_indices().nth(SUMMARY_CHARS) {
        Some((end, _)) => Some(format!("{}...", &line[..end])),
        None => Some(String::from(line)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_skips_blank_lines_and_trims() {
        let docs = ["", "  ", "  Transfer funds.  "].map(String::from);

        assert_eq!(summary(&docs).as_deref(), Some("Transfer funds."));
    }

    #[test]
    fn summary_is_absent_without_text() {
        assert_eq!(summary(&[]), None);
        assert_eq!(summary(&["  ".into(), "\t".into()]), None);
    }

    #[test]
    fn summary_keeps_line_at_limit_whole() {
        let line = "a".repeat(SUMMARY_CHARS);

        assert_eq!(
            summary(core::slice::from_ref(&line)).as_deref(),
            Some(line.as_str())
        );
    }

    #[test]
    fn summary_truncates_past_limit() {
        let line = "я".repeat(SUMMARY_CHARS + 1);

        assert_eq!(
            summary(core::slice::from_ref(&line)),
            Some(format!("{}...", "я".repeat(SUMMARY_CHARS)))
        );
    }
}
