//! Finding items in the metadata by their names, argument names and documentation, ranked with
//! BM25.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

mod bm25;
mod tokens;

use self::bm25::Scorer;
use crate::metadata::{
    MetadataSnapshot,
    item::{Item, ItemKind, for_each_item},
};

/// `name`: at least one word of the query is in the item's own name. `argument`: none is, but
/// one is in the name of an argument the item takes. `docs`: neither; the item was found by its
/// documentation.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchedIn {
    Name,
    Argument,
    Docs,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchRequest {
    /// Words to look for in an item's name, its documentation and, for a call, a view function or
    /// a runtime API method, the names of its arguments. An item need not have every word: it ranks
    /// higher the more of them it has, the rarer they are, and the more of them are in its name
    /// rather than in an argument, in its pallet or runtime API, or in its documentation. Matching
    /// is by whole word, not by fragment: names are split on underscores and case boundaries, so
    /// `transfer_allow_death` is found by `transfer`, by `death`, or by both, but not by `trans`.
    /// The query is split the same way, so `TransferAllowDeath` and `transfer_allow_death` work as
    /// queries. No stemming: `balance` and `balances` are different words; use the form the
    /// metadata uses, or try both. The name of a pallet or runtime API alone finds the pallet or
    /// runtime API itself, not its contents.
    pub query: String,

    /// Restrict the search to these kinds of item. Empty means all of them.
    #[serde(default)]
    pub kinds: Vec<ItemKind>,

    /// How many hits to return, best first: 5 when left out, at most 100.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResults {
    /// The best matches first, whether or not they have every word. Equally good hits follow
    /// the order of `ItemKind`'s values: pallets and runtime APIs first, then what you invoke,
    /// then what you read, then what you only observe.
    pub hits: Vec<SearchHit>,
    /// Words of the query that are nowhere in the metadata, each once. No item has them, so they
    /// are in every hit's `missing` too.
    pub unmatched: Vec<String>,
}

/// One item of the metadata, addressed by `kind`, `namespace` and `name`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchHit {
    #[serde(flatten)]
    pub item: Item,
    /// A `docs` hit is a lead to check, not the answer.
    pub matched_in: MatchedIn,
    /// Words of the query this item does not have, each once, in the order of the query. Empty
    /// when it has them all.
    pub missing: Vec<String>,
}

const DEFAULT_LIMIT: usize = 5;
const MAX_LIMIT: usize = 100;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

pub fn search(
    snapshot: &MetadataSnapshot,
    request: &SearchRequest,
) -> anyhow::Result<SearchResults> {
    let mut scorer = Scorer::new(&request.query, &request.kinds)?;

    for_each_item(&snapshot.metadata, |item| scorer.visit(&item));

    Ok(scorer.results(request.limit))
}

#[cfg(test)]
mod tests {
    use rmcp::serde_json::{self, json};

    use super::*;

    #[test]
    fn limit_defaults_to_five() {
        let request: SearchRequest =
            serde_json::from_value(json!({ "query": "transfer" })).unwrap();

        assert_eq!(request.limit, 5);
    }
}
