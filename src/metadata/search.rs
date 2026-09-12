//! Finding items in the metadata by name and documentation.
//!
//! Both sides are cut into words by the same splitter, and the two sides are named apart
//! throughout: a query yields *terms*, an item's name, namespace and docs yield *tokens*.
//! A term matches a token when they are equal ignoring ASCII case. Names are
//! ASCII identifiers, so they are unaffected; documentation in other scripts
//! compares as written.

use core::cmp::Ordering;

use anyhow::ensure;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};
use subxt_metadata::Metadata;

use self::tokens::tokenize;
use super::LoadedMetadata;

/// A pallet, a runtime API trait, or an item inside one of them.
///
/// `runtime_api` is a trait of host-callable runtime methods; `runtime_api_method` is
/// one method of that trait; `view_function` is a read-only query on a pallet.
// `Ord` is derived from declaration order and `search` sorts by it, so reordering
// the variants reorders equally relevant `hits`.
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

/// `name`: at least one query word hit the item's own name. `docs`: none did; the
/// words were found in its documentation.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchedIn {
    Name,
    Docs,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    /// Words to look for; an item matches only if it matches every one. Matching is by
    /// whole word, not by fragment: names are split on underscores and case boundaries,
    /// so `transfer_allow_death` is found by `transfer`, by `death`, or by both, but
    /// not by `trans`. The query is split the same way, so `TransferAllowDeath` and
    /// `transfer_allow_death` work as queries. No stemming: `balance` and `balances`
    /// are different words; use the form the metadata uses, or try both. A word may
    /// also name the pallet or runtime API trait containing the item, which narrows
    /// the search -- but that name alone finds the pallet or trait itself, not its
    /// contents.
    pub query: String,

    /// Restrict the search to these kinds of item. Empty means all of them.
    #[serde(default)]
    pub kinds: Vec<ItemKind>,

    /// How many hits to return, best match first. Capped at 100. Zero is a count:
    /// `hits` is empty and only `total` and `unmatched` are returned.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchResults {
    /// How many items matched in total. If this is larger than the number of hits
    /// returned, narrow the query -- these are not all of them.
    pub total: usize,
    /// The items that matched, best match first. Equally relevant hits follow the
    /// order of `ItemKind`'s values: pallets and runtime API traits first, then what
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
    /// What the item is. It also fixes what `namespace` is: a runtime API trait for
    /// `runtime_api_method`, a pallet for every other kind that has one.
    pub kind: ItemKind,
    /// The pallet or runtime API trait the item belongs to -- which of the two is given
    /// by `kind`. Absent for a pallet or a trait itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub namespace: Option<String>,
    /// The item's own name, exactly as in the metadata -- without the pallet or
    /// trait it sits in. Pass it on verbatim; do not normalise case.
    pub name: String,
    /// First line of the item's own documentation, truncated. Absent when the item has
    /// no documentation of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string"))]
    pub summary: Option<String>,
    /// A `docs` hit is a lead to check, not the answer.
    pub matched_in: MatchedIn,
}

impl SearchHit {
    fn new(item: &Item<'_>, matched_in: MatchedIn) -> Self {
        Self {
            kind: item.kind,
            namespace: item.namespace.map(String::from),
            name: String::from(item.name),
            summary: summary(item.docs),
            matched_in,
        }
    }
}

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

pub fn search(loaded: &LoadedMetadata, request: &SearchRequest) -> anyhow::Result<SearchResults> {
    let mut scanner = Scanner::from_query(&request.query)?;
    let mut scored: Vec<(Rank, SearchHit)> = Vec::new();

    for_each_item(&loaded.metadata, |item| {
        // Before the kind filter, so `unmatched` is "nowhere in the metadata".
        let term_hits = scanner.scan(&item);

        if !request.kinds.is_empty() && !request.kinds.contains(&item.kind) {
            return;
        }

        let Some(rank) = term_hits.rank() else {
            return;
        };

        scored.push((rank, SearchHit::new(&item, rank.matched_in)));
    });

    scored.sort_by(|(rank_a, hit_a), (rank_b, hit_b)| {
        rank_a.cmp_rank(rank_b)
            // Everything below is only here to make the order reproducible.
            .then_with(|| hit_a.kind.cmp(&hit_b.kind))
            .then_with(|| hit_a.namespace.cmp(&hit_b.namespace))
            .then_with(|| hit_a.name.cmp(&hit_b.name))
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

struct Item<'a> {
    kind: ItemKind,
    namespace: Option<&'a str>,
    name: &'a str,
    docs: &'a [String],
}

/// Adding an `ItemKind` means adding a loop here and touching nothing else: no code
/// downstream has a case per kind. A kind the metadata version predates simply yields
/// nothing -- view functions before v16, runtime APIs before v15 -- so nothing checks the
/// version.
fn for_each_item(metadata: &Metadata, mut visit: impl FnMut(Item<'_>)) {
    for pallet in metadata.pallets() {
        let namespace = pallet.name();

        visit(Item {
            kind: ItemKind::Pallet,
            namespace: None,
            name: namespace,
            docs: pallet.docs(),
        });

        for variant in pallet.call_variants().unwrap_or_default() {
            visit(Item {
                kind: ItemKind::Call,
                namespace: Some(namespace),
                name: &variant.name,
                docs: &variant.docs,
            });
        }

        for variant in pallet.event_variants().unwrap_or_default() {
            visit(Item {
                kind: ItemKind::Event,
                namespace: Some(namespace),
                name: &variant.name,
                docs: &variant.docs,
            });
        }

        for variant in pallet.error_variants().unwrap_or_default() {
            visit(Item {
                kind: ItemKind::Error,
                namespace: Some(namespace),
                name: &variant.name,
                docs: &variant.docs,
            });
        }

        let entries = pallet.storage().map(|storage| storage.entries());

        for entry in entries.unwrap_or_default() {
            visit(Item {
                kind: ItemKind::Storage,
                namespace: Some(namespace),
                name: entry.name(),
                docs: entry.docs(),
            });
        }

        for constant in pallet.constants() {
            visit(Item {
                kind: ItemKind::Constant,
                namespace: Some(namespace),
                name: constant.name(),
                docs: constant.docs(),
            });
        }

        for view_function in pallet.view_functions() {
            visit(Item {
                kind: ItemKind::ViewFunction,
                namespace: Some(namespace),
                name: view_function.name(),
                docs: view_function.docs(),
            });
        }
    }

    for api in metadata.runtime_api_traits() {
        let namespace = api.name();

        visit(Item {
            kind: ItemKind::RuntimeApi,
            namespace: None,
            name: namespace,
            docs: api.docs(),
        });

        for method in api.methods() {
            visit(Item {
                kind: ItemKind::RuntimeApiMethod,
                namespace: Some(namespace),
                name: method.name(),
                docs: method.docs(),
            });
        }
    }
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

    fn scan(&mut self, item: &Item<'_>) -> TermHits {
        let (mut name, mut docs, mut namespace) = (0u64, 0u64, 0u64);

        let mut coverage = NameCoverage::default();

        for token in tokenize(item.name) {
            let matching = self.matching_terms(token);

            name |= matching;
            coverage.record(matching != 0);
        }

        for token in item.docs.iter().flat_map(|doc| tokenize(doc)) {
            docs |= self.matching_terms(token);
        }

        for token in item.namespace.into_iter().flat_map(tokenize) {
            namespace |= self.matching_terms(token);
        }

        self.seen |= name | docs | namespace;

        TermHits {
            count: self.terms.len(),
            name,
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
    docs: u64,
    namespace: u64,
    coverage: NameCoverage,
}

impl TermHits {
    /// `None` when the item does not match.
    fn rank(&self) -> Option<Rank> {
        let mut score = 0;
        let mut by_name = false;
        let mut by_docs = false;

        // Name, then namespace, then docs: first hit wins, not a ranking of weights.
        // Name and namespace both score 2. Namespace before docs so a named pallet
        // outranks a docs-only mention. Namespace sets no flag: a pallet must not
        // select its own children.
        for i in 0..self.count {
            if has_term(self.name, i) {
                score += 2;
                by_name = true;
            } else if has_term(self.namespace, i) {
                score += 2;
            } else if has_term(self.docs, i) {
                score += 1;
                by_docs = true;
            } else {
                return None;
            }
        }

        // So a query naming only a container finds the container itself and nothing under it.
        let matched_in = match (by_name, by_docs) {
            (true, _) => MatchedIn::Name,
            (false, true) => MatchedIn::Docs,
            (false, false) => return None,
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

mod tokens {
    /// Word breaks, not substrings: `all` does not hit `allow` in
    /// `transfer_allow_death`, and a fragment of a pallet name does not select
    /// the pallet's children.
    pub(super) fn tokenize(text: &str) -> impl Iterator<Item = &str> {
        let mut rest = text;

        core::iter::from_fn(move || {
            rest = rest.trim_start_matches(|c: char| !c.is_alphanumeric());

            if rest.is_empty() {
                return None;
            }

            let (token, tail) = rest.split_at(token_end(rest));
            rest = tail;

            Some(token)
        })
    }

    /// Length in bytes of the word starting `text`, which must begin with an alphanumeric.
    fn token_end(text: &str) -> usize {
        let mut chars = text.char_indices();
        let mut prev = chars.next().expect("text starts with an alphanumeric").1;

        for (i, c) in chars {
            if !c.is_alphanumeric() {
                return i;
            }

            if c.is_uppercase() && !prev.is_uppercase() {
                return i;
            }

            // `XCMPQueue` -> `XCMP`, `Queue`: a run of capitals belongs to the word before,
            // except for the last one when a lowercase letter follows it.
            if prev.is_uppercase()
                && c.is_uppercase()
                && text[i + c.len_utf8()..]
                    .chars()
                    .next()
                    .is_some_and(char::is_lowercase)
            {
                return i;
            }

            prev = c;
        }

        text.len()
    }

    #[cfg(test)]
    mod tests {
        use super::tokenize;

        fn split(text: &str) -> Vec<&str> {
            tokenize(text).collect()
        }

        #[test]
        fn splits_on_underscores() {
            assert_eq!(
                split("transfer_allow_death"),
                ["transfer", "allow", "death"]
            );
        }

        #[test]
        fn splits_on_case_boundaries() {
            assert_eq!(split("ExistentialDeposit"), ["Existential", "Deposit"]);
            assert_eq!(split("transferAll"), ["transfer", "All"]);
        }

        #[test]
        fn splits_run_of_capitals_before_lowercase_word() {
            assert_eq!(split("XCMPQueue"), ["XCMP", "Queue"]);
            assert_eq!(split("XCMP"), ["XCMP"]);
        }

        #[test]
        fn keeps_digits_with_their_word() {
            assert_eq!(split("AccountId32"), ["Account", "Id32"]);
            assert_eq!(split("transfer_v2"), ["transfer", "v2"]);
        }

        #[test]
        fn drops_punctuation() {
            assert_eq!(
                split("Transfer succeeded. See `transfer_all`."),
                ["Transfer", "succeeded", "See", "transfer", "all"]
            );
        }

        #[test]
        fn yields_nothing_without_alphanumerics() {
            assert!(split("  _ -- ").is_empty());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(name: &str, namespace: Option<&str>, docs: &[&str], query: &str) -> Option<Rank> {
        let docs: Vec<String> = docs.iter().copied().map(String::from).collect();

        let item = Item {
            kind: ItemKind::Call,
            namespace,
            name,
            docs: &docs,
        };

        let mut scanner = Scanner::from_query(query).unwrap();

        scanner.scan(&item).rank()
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

        let item = Item {
            kind: ItemKind::Pallet,
            namespace: None,
            name: "Balances",
            docs: &docs,
        };
        let mut scanner = Scanner::from_query("balances").unwrap();

        let hit = scanner.scan(&item).rank();

        assert_eq!(hit.unwrap().score, 2);
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
        let item = Item {
            kind: ItemKind::Call,
            namespace: Some("Balances"),
            name: "transfer_all",
            docs: &docs,
        };
        let mut scanner = Scanner::from_query("transfer nonsense").unwrap();

        scanner.scan(&item);

        assert_eq!(scanner.unmatched(), ["nonsense"]);
    }

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
