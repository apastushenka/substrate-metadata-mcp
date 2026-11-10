//! Listing the runtime's pallets.

use std::collections::HashMap;

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

use crate::metadata::{
    MetadataSnapshot,
    item::{PalletItemKind, for_each_item, summary},
};

// A struct rather than a bare `Vec`: MCP requires `structuredContent` to be an object.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PalletList {
    /// Every pallet of the runtime, never truncated, in the order the runtime
    /// declares them -- which is not the order of their indices.
    pub pallets: Vec<PalletEntry>,
}

/// One pallet: the name every other pallet tool takes, the byte that identifies it in
/// encoded data, what its own documentation says about it, and how much of each kind of
/// item it holds.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PalletEntry {
    /// The pallet's name, exactly as in the metadata. Pass it on verbatim as `pallet` to
    /// `list_pallet_items` and `get_pallet_item`; do not normalize case.
    pub name: String,
    /// The pallet's index, the byte that identifies it in encoded data: the first byte of
    /// an encoded call, and the `index` of a `DispatchError::Module` or a `ModuleError`.
    /// The same byte comes back as `pallet_index` on a call, an event or an error, each of
    /// which carries a second index of its own to pick the variant within the pallet.
    pub index: u8,
    /// The first line of the pallet's own documentation, truncated. Usually absent:
    /// runtimes rarely document pallets in their metadata, so a bare name is normal and
    /// not a sign of a missing pallet.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub summary: Option<String>,
    /// How many items of each kind the pallet holds, so that a listing can be sized
    /// before it is asked for.
    pub counts: PalletItemCounts,
}

/// How many items of each kind one pallet holds. A field name here is a value of `kinds`
/// in `list_pallet_items`, and its number is the `total` that tool reports for that kind:
/// zero means there is nothing to list, and a large count may need paging via that tool's
/// `offset` and `limit`.
///
/// A zero is reported rather than omitted: a kind a pallet declares none of and a kind
/// the metadata version predates are the same thing to a caller, since neither can be
/// listed. Under metadata v14 and v15 `view_function` is therefore always zero.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct PalletItemCounts {
    /// Extrinsics the pallet dispatches.
    pub call: usize,
    /// Read-only queries on the pallet's state, which only metadata v16 and later carry.
    pub view_function: usize,
    /// Entries of the pallet's storage.
    pub storage: usize,
    /// Values fixed in the runtime's code rather than in its storage.
    pub constant: usize,
    /// Events the pallet emits.
    pub event: usize,
    /// Errors the pallet's calls return.
    pub error: usize,
}

impl PalletItemCounts {
    /// Exhaustive on purpose: a new `PalletItemKind` does not compile until it has a
    /// counter of its own here.
    fn add(&mut self, kind: PalletItemKind) {
        let counter = match kind {
            PalletItemKind::Call => &mut self.call,
            PalletItemKind::ViewFunction => &mut self.view_function,
            PalletItemKind::Storage => &mut self.storage,
            PalletItemKind::Constant => &mut self.constant,
            PalletItemKind::Event => &mut self.event,
            PalletItemKind::Error => &mut self.error,
        };

        *counter += 1;
    }
}

pub fn pallets(snapshot: &MetadataSnapshot) -> PalletList {
    let metadata = &snapshot.metadata;

    // Counted from the walk `list_pallet_items` pages over, under the same
    // `from_item_kind` filter, so a count here and that tool's `total` move together.
    // The keys come from the metadata: an `ItemRef` does not outlive its visit.
    let mut counts: HashMap<&str, PalletItemCounts> = metadata
        .pallets()
        .map(|pallet| (pallet.name(), PalletItemCounts::default()))
        .collect();

    for_each_item(metadata, |item| {
        let Some(kind) = PalletItemKind::from_item_kind(item.kind) else {
            return;
        };

        // A runtime API method is already gone: `from_item_kind` has no pallet kind for
        // it. What remains is a pallet item, whose namespace is its pallet.
        if let Some(counts) = item.namespace.and_then(|name| counts.get_mut(name)) {
            counts.add(kind);
        }
    });

    let pallets = metadata
        .pallets()
        .map(|pallet| PalletEntry {
            name: String::from(pallet.name()),
            // subxt keeps a separate index for calls, events and errors, but fills all
            // three from the pallet's single `index` in v14, v15 and v16 alike, so any
            // one of them is the pallet's index.
            index: pallet.call_index(),
            summary: summary(pallet.docs()),
            counts: counts.remove(pallet.name()).unwrap_or_default(),
        })
        .collect();

    PalletList { pallets }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The counts are addressed by the same names as the kinds a listing is asked for, so
    // a caller reads `counts.storage` and passes `kinds: ["storage"]`. A kind without a
    // count would silently read as zero.
    //
    // Compared sorted: the order of an object's members carries no meaning, and schemars
    // keeps `properties` alphabetically rather than as declared.
    #[test]
    fn counter_names_match_pallet_item_kinds() {
        let schema = schemars::schema_for!(PalletItemCounts);

        let mut counted: Vec<&str> = schema
            .get("properties")
            .and_then(|properties| properties.as_object())
            .expect("PalletItemCounts is an object")
            .keys()
            .map(String::as_str)
            .collect();

        let mut kinds: Vec<&str> = PalletItemKind::ALL
            .iter()
            .map(|kind| kind.as_str())
            .collect();

        counted.sort_unstable();
        kinds.sort_unstable();
        assert_eq!(counted, kinds);
    }

    #[test]
    fn add_increments_the_counter_for_its_kind() {
        let mut counts = PalletItemCounts::default();

        counts.add(PalletItemKind::Storage);
        counts.add(PalletItemKind::Storage);
        counts.add(PalletItemKind::Event);

        assert_eq!(counts.storage, 2);
        assert_eq!(counts.event, 1);
        assert_eq!(counts.call, 0);
    }
}
