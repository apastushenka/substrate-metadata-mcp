//! Listing everything one pallet contains.

use anyhow::anyhow;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use super::{
    LoadedMetadata,
    item::{Item, ItemRef, PalletItemKind, for_each_item},
    lookup::{self, pallet_lookup_error_message},
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PalletItemsRequest {
    /// The pallet whose items to list. `list_pallets` lists the valid names; case is
    /// forgiven, `balances` finds `Balances`, and the response spells the name as the
    /// metadata does. An unknown pallet is an error, so an empty result means the
    /// pallet exists and has no items of the requested kinds. A runtime API name is an
    /// error too: this tool lists pallet items only.
    pub pallet: String,

    /// Restrict the listing to these kinds of item. Empty means all of them.
    #[serde(default)]
    pub kinds: Vec<PalletItemKind>,

    /// Items to skip before the first one returned. Together with `limit`, this
    /// reads a long listing in pages: pass the count already seen.
    #[serde(default)]
    pub offset: usize,

    /// Items to return after `offset`. Capped at 100. A limit of zero returns the count
    /// only: `items` is empty and only `total` is returned.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

impl PalletItemsRequest {
    /// `pallet` is the name as the metadata spells it, not as the request does.
    fn selects(&self, pallet: &str, item: &ItemRef<'_>) -> bool {
        if item.namespace != Some(pallet) {
            return false;
        }

        let Some(kind) = PalletItemKind::from_item_kind(item.kind) else {
            return false;
        };

        self.kinds.is_empty() || self.kinds.contains(&kind)
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PalletItems {
    /// How many items the listing has in all, before `offset` and `limit`. If
    /// `offset` plus the number of items returned is less than this, there are more:
    /// repeat the call with a larger `offset`.
    pub total: usize,
    /// The items, grouped by kind: calls, view functions, storage entries, constants,
    /// events, errors. Within a kind they follow the order the pallet declares them,
    /// which for calls is not necessarily the order of their indices.
    pub items: Vec<Item>,
}

// A listing is not ranked, so there is no reason to return less than the cap by default.
const MAX_LIMIT: usize = 100;

fn default_limit() -> usize {
    MAX_LIMIT
}

pub fn pallet_items(
    loaded: &LoadedMetadata,
    request: &PalletItemsRequest,
) -> anyhow::Result<PalletItems> {
    let metadata = &loaded.metadata;

    // The filter below cannot tell "no such items" from "no such pallet".
    let pallet = lookup::pallet(metadata, &request.pallet)
        .map_err(|error| anyhow!(pallet_lookup_error_message(&error)))?
        .name();

    // Items cannot be kept past the visit (see `for_each_item`), so the page is cut on
    // the fly: count every match, keep only those inside the window.
    let limit = request.limit.min(MAX_LIMIT);
    let mut total = 0;
    let mut items = Vec::new();

    for_each_item(metadata, |item| {
        if !request.selects(pallet, &item) {
            return;
        }

        if total >= request.offset && items.len() < limit {
            items.push(Item::new(&item));
        }

        total += 1;
    });

    Ok(PalletItems { total, items })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::item::ItemKind;

    fn request(pallet: &str, kinds: &[PalletItemKind]) -> PalletItemsRequest {
        PalletItemsRequest {
            pallet: String::from(pallet),
            kinds: kinds.to_vec(),
            offset: 0,
            limit: MAX_LIMIT,
        }
    }

    fn item<'a>(kind: ItemKind, namespace: Option<&'a str>, name: &'a str) -> ItemRef<'a> {
        ItemRef {
            kind,
            namespace,
            name,
            docs: &[],
        }
    }

    #[test]
    fn selects_items_of_the_pallet_only() {
        let request = request("Balances", &[]);

        assert!(request.selects(
            "Balances",
            &item(ItemKind::Call, Some("Balances"), "transfer_all")
        ));
        assert!(!request.selects("Balances", &item(ItemKind::Call, Some("Staking"), "bond")));
        assert!(!request.selects("Balances", &item(ItemKind::Pallet, None, "Balances")));
    }

    #[test]
    fn selects_no_runtime_api_method_under_the_same_name() {
        let request = request("Balances", &[]);

        assert!(!request.selects(
            "Balances",
            &item(ItemKind::RuntimeApiMethod, Some("Balances"), "free_balance")
        ));
    }

    #[test]
    fn kinds_restrict_the_selection() {
        let request = request("Balances", &[PalletItemKind::Storage]);

        assert!(request.selects(
            "Balances",
            &item(ItemKind::Storage, Some("Balances"), "Account")
        ));
        assert!(!request.selects(
            "Balances",
            &item(ItemKind::Call, Some("Balances"), "transfer_all")
        ));
    }
}
