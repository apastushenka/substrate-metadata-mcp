//! Finding items in the metadata by name, argument names and documentation.
//!
//! Both sides are cut into words by the same splitter, and the two sides are named apart
//! throughout: a query yields *terms*, an item's name, namespace, argument names and docs yield
//! *tokens*.
//! A term matches a token when they are equal ignoring ASCII case. Names are
//! ASCII identifiers, so they are unaffected; documentation in other scripts
//! compares as written.

use core::cmp::Ordering;

use anyhow::ensure;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

mod tokens;

use self::tokens::tokenize;
use crate::metadata::{
    MetadataSnapshot,
    item::{Item, ItemKind, ItemRef, for_each_item},
};

/// `name`: at least one query word hit the item's own name. `argument`: none did, but one hit
/// the name of an argument the item takes. `docs`: neither; the words were found in its
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
    /// a runtime API method, the names of its arguments; an item matches only if it matches every
    /// one. Matching is by whole word, not by fragment: names are split on underscores and case
    /// boundaries, so `transfer_allow_death` is found by `transfer`, by `death`, or by both, but
    /// not by `trans`. The query is split the same way, so `TransferAllowDeath` and
    /// `transfer_allow_death` work as queries. No stemming: `balance` and `balances`
    /// are different words; use the form the metadata uses, or try both. A word may also
    /// name the pallet or runtime API containing the item, which narrows the search --
    /// but that name alone finds the pallet or runtime API itself, not its contents.
    pub query: String,

    /// Restrict the search to these kinds of item. Empty means all of them.
    #[serde(default)]
    pub kinds: Vec<ItemKind>,

    /// How many hits to return, best match first. Capped at 100. A limit of zero returns
    /// the count only: `hits` is empty and only `total` and `unmatched` are returned.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResults {
    /// How many items matched in total. If this is larger than the number of hits
    /// returned, narrow the query -- these are not all of them.
    pub total: usize,
    /// The items that matched, best match first. Equally relevant hits follow the
    /// order of `ItemKind`'s values: pallets and runtime APIs first, then what
    /// you invoke, then what you read, then what you only observe.
    pub hits: Vec<SearchHit>,
    /// Words that matched nothing anywhere in the metadata. Every word has to match for
    /// an item to be returned, so a non-empty list is why the result is empty. Empty
    /// alongside `total` of 0 means every word exists, but never together on one item,
    /// or only on items outside `kinds`.
    pub unmatched: Vec<String>,
}

/// One item of the metadata, addressed by `kind`, `namespace` and `name`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchHit {
    #[serde(flatten)]
    pub item: Item,
    /// A `docs` hit is a lead to check, not the answer.
    pub matched_in: MatchedIn,
}

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

pub fn search(
    snapshot: &MetadataSnapshot,
    request: &SearchRequest,
) -> anyhow::Result<SearchResults> {
    let mut scanner = Scanner::from_query(&request.query)?;
    let mut scored: Vec<(Rank, SearchHit)> = Vec::new();

    for_each_item(&snapshot.metadata, |item| {
        // Before the kind filter, so `unmatched` is "nowhere in the metadata".
        let term_hits = scanner.scan(&item);

        if !request.kinds.is_empty() && !request.kinds.contains(&item.kind) {
            return;
        }

        let Some(rank) = term_hits.rank() else {
            return;
        };

        scored.push((
            rank,
            SearchHit {
                item: Item::new(&item),
                matched_in: rank.matched_in,
            },
        ));
    });

    scored.sort_by(|(rank_a, hit_a), (rank_b, hit_b)| {
        let a = &hit_a.item;
        let b = &hit_b.item;

        rank_a
            .cmp_rank(rank_b)
            // Everything below is only here to make the order reproducible.
            .then_with(|| (a.kind, &a.namespace, &a.name).cmp(&(b.kind, &b.namespace, &b.name)))
    });

    let total = scored.len();

    let hits = scored
        .into_iter()
        .take(request.limit.min(MAX_LIMIT))
        .map(|(_, hit)| hit)
        .collect();

    Ok(SearchResults {
        total,
        hits,
        unmatched: scanner.unmatched(),
    })
}

/// A set of terms is a `u64` mask, bit `i` for term `i`.
fn has_term(mask: u64, index: usize) -> bool {
    mask & (1u64 << index) != 0
}

/// One pass over the metadata: the query's words, and which of them have appeared in
/// any item so far.
struct Scanner<'a> {
    terms: Vec<&'a str>,
    seen: u64,
}

impl<'a> Scanner<'a> {
    /// Terms are tracked as bits of a `u64`, one per term, which is what caps them.
    const MAX_TERMS: usize = 64;

    fn from_query(query: &'a str) -> anyhow::Result<Self> {
        let terms: Vec<&str> = tokenize(query).collect();

        ensure!(!terms.is_empty(), "query must not be empty");
        ensure!(
            terms.len() <= Self::MAX_TERMS,
            "query must not have more than {} words",
            Self::MAX_TERMS
        );

        Ok(Self { terms, seen: 0 })
    }

    fn scan(&mut self, item: &ItemRef<'_>) -> TermHits {
        let (mut name, mut args, mut docs, mut namespace) = (0u64, 0u64, 0u64, 0u64);

        let mut coverage = NameCoverage::default();

        for token in tokenize(item.name) {
            let matching = self.matching_terms(token);

            name |= matching;
            coverage.record(matching != 0);
        }

        item.args.for_each_name(|arg| {
            for token in tokenize(arg) {
                args |= self.matching_terms(token);
            }
        });

        for token in item.docs.iter().flat_map(|doc| tokenize(doc)) {
            docs |= self.matching_terms(token);
        }

        for token in item.namespace.into_iter().flat_map(tokenize) {
            namespace |= self.matching_terms(token);
        }

        self.seen |= name | args | docs | namespace;

        TermHits {
            count: self.terms.len(),
            name,
            args,
            docs,
            namespace,
            coverage,
        }
    }

    fn unmatched(&self) -> Vec<String> {
        self.terms
            .iter()
            .enumerate()
            .filter(|(i, _)| !has_term(self.seen, *i))
            .map(|(_, term)| String::from(*term))
            .collect()
    }

    fn matching_terms(&self, token: &str) -> u64 {
        self.terms
            .iter()
            .enumerate()
            .filter(|(_, term)| token.eq_ignore_ascii_case(term))
            .fold(0, |matching, (i, _)| matching | 1u64 << i)
    }
}

/// Tokens of the name that some term hit, and how many there are.
#[derive(Clone, Copy, Default)]
struct NameCoverage {
    matched: usize,
    total: usize,
}

impl NameCoverage {
    fn record(&mut self, hit: bool) {
        self.total += 1;
        self.matched += usize::from(hit);
    }

    fn cmp_coverage(&self, other: &Self) -> Ordering {
        // Coverage as a fraction, without dividing.
        (self.matched * other.total).cmp(&(other.matched * self.total))
    }
}

/// Which of the query's terms one item hit, and in which of its texts.
struct TermHits {
    /// How many terms the query had -- how far `rank` reads the masks below.
    count: usize,
    name: u64,
    args: u64,
    docs: u64,
    namespace: u64,
    coverage: NameCoverage,
}

impl TermHits {
    /// `None` when the item does not match.
    fn rank(&self) -> Option<Rank> {
        let mut score = 0;
        let mut by_name = false;
        let mut by_args = false;
        let mut by_docs = false;

        // Name, then namespace, then arguments, then docs: first hit wins, not a ranking of
        // weights. Name and namespace both score 2, arguments and docs 1. Namespace before docs
        // so a named pallet outranks a docs-only mention. Namespace sets no flag: a pallet must
        // not select its own children.
        for i in 0..self.count {
            if has_term(self.name, i) {
                score += 2;
                by_name = true;
            } else if has_term(self.namespace, i) {
                score += 2;
            } else if has_term(self.args, i) {
                score += 1;
                by_args = true;
            } else if has_term(self.docs, i) {
                score += 1;
                by_docs = true;
            } else {
                return None;
            }
        }

        let matched_in = if by_name {
            MatchedIn::Name
        } else if by_args {
            MatchedIn::Argument
        } else if by_docs {
            MatchedIn::Docs
        } else {
            // So a query naming only a container finds the container itself and nothing under it.
            return None;
        };

        Some(Rank {
            score,
            coverage: self.coverage,
            matched_in,
        })
    }
}

#[derive(Clone, Copy)]
struct Rank {
    score: usize,
    coverage: NameCoverage,
    matched_in: MatchedIn,
}

impl Rank {
    fn cmp_rank(&self, other: &Self) -> Ordering {
        other
            .score
            .cmp(&self.score)
            .then_with(|| other.coverage.cmp_coverage(&self.coverage))
    }
}

#[cfg(test)]
mod tests {
    use scale_info::form::PortableForm;

    use super::*;
    use crate::metadata::item::Args;

    fn rank(name: &str, namespace: Option<&str>, docs: &[&str], query: &str) -> Option<Rank> {
        let docs: Vec<String> = docs.iter().copied().map(String::from).collect();

        let item = ItemRef {
            kind: ItemKind::Call,
            namespace,
            name,
            docs: &docs,
            args: Args::None,
        };

        let mut scanner = Scanner::from_query(query).unwrap();

        scanner.scan(&item).rank()
    }

    fn call_args(names: &[&str]) -> Vec<scale_info::Field<PortableForm>> {
        names
            .iter()
            .map(|name| {
                scale_info::Field::new(Some(String::from(*name)), 0u32.into(), None, Vec::new())
            })
            .collect()
    }

    #[test]
    fn matching_ignores_case() {
        let hit = rank(
            "transfer_allow_death",
            Some("Balances"),
            &[],
            "TransferDeath",
        );

        assert_eq!(hit.unwrap().score, 4);
    }

    #[test]
    fn matching_needs_every_term() {
        assert!(rank("transfer_all", Some("Balances"), &[], "transfer nonsense").is_none());
    }

    #[test]
    fn name_outscores_documentation() {
        let name = rank("transfer_all", Some("Balances"), &[], "transfer");
        let docs = rank(
            "set_balance",
            Some("Balances"),
            &["Transfer funds."],
            "transfer",
        );

        assert!(name.unwrap().score > docs.unwrap().score);
    }

    #[test]
    fn container_narrows_but_does_not_select() {
        assert!(rank("transfer_all", Some("Balances"), &[], "balances").is_none());

        let hit = rank("transfer_all", Some("Balances"), &[], "balances transfer");

        // Both terms score; what the container cannot do is select on its own.
        assert_eq!(hit.unwrap().score, 4);
    }

    #[test]
    fn container_outweighs_documentation_mention() {
        let named = rank("bond", Some("Staking"), &[], "staking bond");
        let mentioned = rank(
            "bond",
            Some("NominationPools"),
            &["Bond funds into a staking pool."],
            "staking bond",
        );

        assert!(named.unwrap().score > mentioned.unwrap().score);
    }

    #[test]
    fn container_matches_on_its_own_name() {
        let docs: Vec<String> = Vec::new();

        let item = ItemRef {
            kind: ItemKind::Pallet,
            namespace: None,
            name: "Balances",
            docs: &docs,
            args: Args::None,
        };
        let mut scanner = Scanner::from_query("balances").unwrap();

        let hit = scanner.scan(&item).rank();

        assert_eq!(hit.unwrap().score, 2);
    }

    #[test]
    fn matching_finds_terms_in_argument_names() {
        let args = call_args(&["dest", "value"]);
        let item = ItemRef {
            kind: ItemKind::Call,
            namespace: Some("Balances"),
            name: "transfer_allow_death",
            docs: &[],
            args: Args::Call(&args),
        };
        let mut scanner = Scanner::from_query("dest value").unwrap();

        let hit = scanner.scan(&item).rank();

        assert!(matches!(hit.unwrap().matched_in, MatchedIn::Argument));
    }

    #[test]
    fn matched_in_prefers_name_then_argument_then_docs() {
        let args = call_args(&["dest"]);
        let docs = [String::from("Transfer funds.")];
        let item = ItemRef {
            kind: ItemKind::Call,
            namespace: Some("Balances"),
            name: "transfer_all",
            docs: &docs,
            args: Args::Call(&args),
        };
        let matched_in = |query: &str| {
            let mut scanner = Scanner::from_query(query).unwrap();

            scanner.scan(&item).rank().unwrap().matched_in
        };

        assert!(matches!(matched_in("transfer dest funds"), MatchedIn::Name));
        assert!(matches!(matched_in("dest funds"), MatchedIn::Argument));
        assert!(matches!(matched_in("funds"), MatchedIn::Docs));
    }

    #[test]
    fn coverage_counts_matched_name_tokens() {
        let whole = rank("Transfer", Some("Balances"), &[], "transfer").unwrap();
        let part = rank("transfer_allow_death", Some("Balances"), &[], "transfer").unwrap();

        assert_eq!((whole.coverage.matched, whole.coverage.total), (1, 1));
        assert_eq!((part.coverage.matched, part.coverage.total), (1, 3));
    }

    #[test]
    fn coverage_ranks_fully_matched_name_first() {
        let whole = rank("Transfer", Some("Balances"), &[], "transfer").unwrap();
        let part = rank("transfer_allow_death", Some("Balances"), &[], "transfer").unwrap();

        assert_eq!(whole.score, part.score);
        assert_eq!(whole.cmp_rank(&part), Ordering::Less);
    }

    #[test]
    fn unmatched_lists_terms_that_hit_nothing() {
        let docs: Vec<String> = Vec::new();
        let item = ItemRef {
            kind: ItemKind::Call,
            namespace: Some("Balances"),
            name: "transfer_all",
            docs: &docs,
            args: Args::None,
        };
        let mut scanner = Scanner::from_query("transfer nonsense").unwrap();

        scanner.scan(&item);

        assert_eq!(scanner.unmatched(), ["nonsense"]);
    }
}
