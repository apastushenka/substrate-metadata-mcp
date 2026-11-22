//! Ranking the items of the metadata for a query, with BM25.
//!
//! The query and every item are cut into words by the same splitter, and the two sides are named
//! apart throughout: the query yields *terms*; an item's name, namespace, argument names and docs
//! yield *tokens*. A term matches a token when they are equal ignoring ASCII case. Names are ASCII
//! identifiers, so they are unaffected; documentation in other scripts compares as written.
//!
//! An item's score adds up, for each term, its IDF times its saturated tf (a term typed twice
//! counts twice), and grows by up to 50% when the query covers the item's name. tf (term frequency)
//! counts the term's occurrences in the item, each weighted by where it is. df (document frequency)
//! is how many items have the term, and IDF (inverse document frequency) turns it into a weight
//! that is higher the fewer of all N items have the term.

use core::ops::{BitOr, BitOrAssign};

use anyhow::ensure;

use super::{MAX_LIMIT, MatchedIn, SearchHit, SearchResults, tokens::tokenize};
use crate::metadata::item::{Item, ItemKind, ItemRef};

// What one occurrence of a term weighs, by where it is. The name and the namespace weigh as in
// rustdoc-mcp's search (ferritin-common 0.17), and arguments, which it does not have, as the
// namespace. It gives the first paragraph of docs 3; here all docs weigh 1, which ranked no worse.
const NAME_WEIGHT: u32 = 20;
const NAMESPACE_WEIGHT: u32 = 3;
const ARGUMENT_WEIGHT: u32 = 3;
const DOCS_WEIGHT: u32 = 1;

/// BM25's saturation: past a few occurrences, more of them add little. Scores are not normalized
/// by length (`b = 0`): long documentation is no sign of a worse match.
const K1: f64 = 1.2;

/// A name the query covers whole scores this much more again; a name covered in part, its share.
const COVERAGE_BONUS: f64 = 0.5;

/// One pass over the metadata. A score needs to know how many items have each term, so items
/// that may be hits are kept and only scored once the pass is over.
pub struct Scorer<'a> {
    /// The query; `df` and `tf` hold one entry per term, in its order.
    terms: Terms<'a>,
    /// The kinds a hit may be of; empty lets any kind through.
    kinds: &'a [ItemKind],
    /// How many items were visited, of any kind.
    items: u32,
    /// How many items have each term, anywhere.
    df: Vec<u32>,
    /// The weighted occurrences of each term in the item being visited.
    tf: Vec<u32>,
    /// The items that may be hits, kept until `results`, when the counts are complete.
    candidates: Vec<Candidate>,
}

impl<'a> Scorer<'a> {
    pub fn new(query: &'a str, kinds: &'a [ItemKind]) -> anyhow::Result<Self> {
        let terms = Terms::new(query)?;

        Ok(Self {
            df: vec![0; terms.len()],
            tf: vec![0; terms.len()],
            terms,
            kinds,
            items: 0,
            candidates: Vec::new(),
        })
    }

    pub fn visit(&mut self, item: &ItemRef<'_>) {
        self.items += 1;
        self.tf.fill(0);

        let mut name = TermSet::default();
        let mut coverage = NameCoverage::default();

        for token in tokenize(item.name) {
            let term = self.terms.matching(token);

            if let Some(i) = term {
                self.tf[i] += NAME_WEIGHT;
                name.insert(i);
            }

            coverage.record(term.is_some());
        }

        let mut args = TermSet::default();

        item.args
            .for_each_name(|arg| args |= self.scan(arg, ARGUMENT_WEIGHT));

        let docs = item.docs.iter().fold(TermSet::default(), |docs, line| {
            docs | self.scan(line, DOCS_WEIGHT)
        });

        let mut namespace = TermSet::default();

        for token in item.namespace.into_iter().flat_map(tokenize) {
            let Some(i) = self.terms.matching(token) else {
                continue;
            };

            // A word the name has counts there only, or an item named like its pallet would
            // outrank the pallet by the namespace alone: on `claims`, the storage `Claims::Claims`
            // would come before the pallet `Claims`.
            if !name.contains(i) {
                self.tf[i] += NAMESPACE_WEIGHT;
            }

            namespace.insert(i);
        }

        let present = name | args | docs | namespace;

        for (i, df) in self.df.iter_mut().enumerate() {
            *df += u32::from(present.contains(i));
        }

        // After the counts, so that IDF and `unmatched` cover the whole metadata, not just `kinds`.
        if !self.kinds.is_empty() && !self.kinds.contains(&item.kind) {
            return;
        }

        // A namespace ranks its items higher but does not select them: a query that only names
        // a pallet finds the pallet, not everything in it.
        let matched_in = if !name.is_empty() {
            MatchedIn::Name
        } else if !args.is_empty() {
            MatchedIn::Argument
        } else if !docs.is_empty() {
            MatchedIn::Docs
        } else {
            return;
        };

        self.candidates.push(Candidate {
            item: Item::new(item),
            tf: self.tf.clone(),
            present,
            matched_in,
            coverage: coverage.share(),
        });
    }

    pub fn results(self, limit: usize) -> SearchResults {
        let Self {
            terms,
            items,
            df,
            candidates,
            ..
        } = self;

        let idf: Vec<f64> = df.iter().map(|&df| idf(items, df)).collect();
        let ranked = rank(candidates, |candidate| candidate.score(&idf, &terms.times));

        let hits = ranked
            .into_iter()
            .take(limit.min(MAX_LIMIT))
            .map(|candidate| candidate.into_hit(&terms))
            .collect();

        SearchResults {
            hits,
            unmatched: terms.listed(|i| df[i] == 0),
        }
    }

    /// Adds `weight` to a term's tf for each of its occurrences in `text`, and returns the terms it
    /// has.
    fn scan(&mut self, text: &str, weight: u32) -> TermSet {
        let mut found = TermSet::default();

        for token in tokenize(text) {
            if let Some(i) = self.terms.matching(token) {
                self.tf[i] += weight;
                found.insert(i);
            }
        }

        found
    }
}

/// The query, split into terms.
struct Terms<'a> {
    /// Each term once, as first typed.
    words: Vec<&'a str>,
    /// How many times each term was typed: a repeated word counts each time.
    times: Vec<u32>,
}

impl<'a> Terms<'a> {
    fn new(query: &'a str) -> anyhow::Result<Self> {
        // One word past the cap is enough to refuse the query, so a longer one is neither read
        // through nor merged.
        let query_words: Vec<&str> = tokenize(query).take(TermSet::CAPACITY + 1).collect();

        ensure!(!query_words.is_empty(), "query must not be empty");
        ensure!(
            query_words.len() <= TermSet::CAPACITY,
            "query must not have more than {} words",
            TermSet::CAPACITY
        );

        let mut words: Vec<&str> = Vec::new();
        let mut times = Vec::new();

        for word in query_words {
            match words
                .iter()
                .position(|term| term.eq_ignore_ascii_case(word))
            {
                Some(i) => times[i] += 1,
                None => {
                    words.push(word);
                    times.push(1);
                }
            }
        }

        Ok(Self { words, times })
    }

    fn len(&self) -> usize {
        self.words.len()
    }

    /// The term `token` matches. Repeats are merged, so there is at most one.
    fn matching(&self, token: &str) -> Option<usize> {
        self.words
            .iter()
            .position(|term| token.eq_ignore_ascii_case(term))
    }

    /// The terms whose index passes `keep`, in the order of the query.
    fn listed(&self, keep: impl Fn(usize) -> bool) -> Vec<String> {
        self.words
            .iter()
            .enumerate()
            .filter(|(i, _)| keep(*i))
            .map(|(_, term)| String::from(*term))
            .collect()
    }
}

/// A set of the query's terms, by index: one bit of a `u64` per term.
#[derive(Clone, Copy, Default)]
struct TermSet(u64);

impl TermSet {
    /// One bit per term, which is what caps the query.
    const CAPACITY: usize = u64::BITS as usize;

    fn insert(&mut self, term: usize) {
        self.0 |= 1 << term;
    }

    fn contains(self, term: usize) -> bool {
        self.0 & (1 << term) != 0
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl BitOr for TermSet {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOrAssign for TermSet {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// An item of a requested kind that has a term outside its namespace.
struct Candidate {
    /// Copied out of the walk, whose `ItemRef` lives only for the visit.
    item: Item,
    /// `Scorer::tf` as it was after the visit.
    tf: Vec<u32>,
    /// The terms it has anywhere, its namespace included.
    present: TermSet,
    /// The best field it has a term in, its namespace aside.
    matched_in: MatchedIn,
    /// How much of its name the query covers, from 0 to 1.
    coverage: f64,
}

impl Candidate {
    fn score(&self, idf: &[f64], times: &[u32]) -> f64 {
        let relevance: f64 = self
            .tf
            .iter()
            .zip(times)
            .zip(idf)
            .map(|((&tf, &times), &idf)| f64::from(times) * idf * saturate(tf))
            .sum();

        relevance * (1.0 + COVERAGE_BONUS * self.coverage)
    }

    fn into_hit(self, terms: &Terms<'_>) -> SearchHit {
        SearchHit {
            missing: terms.listed(|i| !self.present.contains(i)),
            item: self.item,
            matched_in: self.matched_in,
        }
    }
}

/// How much of a name the query covers, counted in tokens.
#[derive(Default)]
struct NameCoverage {
    /// Tokens of the name that some term hit.
    matched: u32,
    /// Tokens in the name.
    total: u32,
}

impl NameCoverage {
    fn record(&mut self, hit: bool) {
        self.total += 1;
        self.matched += u32::from(hit);
    }

    fn share(&self) -> f64 {
        f64::from(self.matched) / f64::from(self.total.max(1))
    }
}

/// The candidates, best first. A score is rounded to the nearest 0.1% of the best one, and equal
/// ones follow a fixed order: sorting by the float itself would let its last bits, which differ
/// between platforms, decide.
fn rank(candidates: Vec<Candidate>, score: impl Fn(&Candidate) -> f64) -> Vec<Candidate> {
    let scored: Vec<(f64, Candidate)> = candidates
        .into_iter()
        .map(|candidate| (score(&candidate), candidate))
        .collect();

    let best = scored.iter().map(|(value, _)| *value).fold(0.0, f64::max);

    let mut bucketed: Vec<(u32, Candidate)> = scored
        .into_iter()
        .map(|(value, candidate)| ((value / best * 1000.0).round() as u32, candidate))
        .collect();

    bucketed.sort_by(|(bucket_a, a), (bucket_b, b)| {
        bucket_b
            .cmp(bucket_a)
            .then_with(|| a.item.address().cmp(&b.item.address()))
    });

    bucketed
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

/// Lucene's IDF. The classic one turns negative for a term that most items have, and would then
/// push down every item that has it.
fn idf(items: u32, df: u32) -> f64 {
    let (items, df) = (f64::from(items), f64::from(df));

    (1.0 + (items - df + 0.5) / (df + 0.5)).ln()
}

fn saturate(tf: u32) -> f64 {
    let tf = f64::from(tf);

    tf * (K1 + 1.0) / (tf + K1)
}

#[cfg(test)]
mod tests {
    use scale_info::form::PortableForm;

    use super::*;
    use crate::metadata::item::Args;

    /// An item to search, owning what an `ItemRef` borrows.
    struct TestItem {
        kind: ItemKind,
        namespace: Option<String>,
        name: String,
        docs: Vec<String>,
        args: Vec<scale_info::Field<PortableForm>>,
    }

    impl TestItem {
        fn new(kind: ItemKind, namespace: Option<&str>, name: &str) -> Self {
            Self {
                kind,
                namespace: namespace.map(String::from),
                name: String::from(name),
                docs: Vec::new(),
                args: Vec::new(),
            }
        }

        fn docs(mut self, line: &str) -> Self {
            self.docs.push(String::from(line));
            self
        }

        fn args(mut self, names: &[&str]) -> Self {
            self.args = names
                .iter()
                .map(|name| {
                    scale_info::Field::new(Some(String::from(*name)), 0u32.into(), None, Vec::new())
                })
                .collect();
            self
        }

        fn as_ref(&self) -> ItemRef<'_> {
            ItemRef {
                kind: self.kind,
                namespace: self.namespace.as_deref(),
                name: &self.name,
                docs: &self.docs,
                args: if self.args.is_empty() {
                    Args::None
                } else {
                    Args::Call(&self.args)
                },
            }
        }
    }

    fn call(namespace: &str, name: &str) -> TestItem {
        TestItem::new(ItemKind::Call, Some(namespace), name)
    }

    fn pallet(name: &str) -> TestItem {
        TestItem::new(ItemKind::Pallet, None, name)
    }

    fn run(query: &str, items: &[TestItem], limit: usize) -> SearchResults {
        run_kinds(query, &[], items, limit)
    }

    fn run_kinds(
        query: &str,
        kinds: &[ItemKind],
        items: &[TestItem],
        limit: usize,
    ) -> SearchResults {
        let mut scorer = Scorer::new(query, kinds).unwrap();

        for item in items {
            scorer.visit(&item.as_ref());
        }

        scorer.results(limit)
    }

    fn names(results: &SearchResults) -> Vec<&str> {
        results
            .hits
            .iter()
            .map(|hit| hit.item.name.as_str())
            .collect()
    }

    #[test]
    fn query_takes_one_to_64_words() {
        let words = |count: usize| {
            (0..count)
                .map(|i| format!("w{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        };

        assert!(Terms::new(" _ ").is_err());
        assert!(Terms::new(&words(64)).is_ok());
        assert!(Terms::new(&words(65)).is_err());
    }

    #[test]
    fn matching_ignores_case() {
        let results = run(
            "TransferDeath",
            &[call("Balances", "transfer_allow_death")],
            5,
        );

        assert_eq!(names(&results), ["transfer_allow_death"]);
        assert!(results.hits[0].missing.is_empty());
    }

    #[test]
    fn matching_finds_terms_in_argument_names() {
        let results = run(
            "dest value",
            &[call("Balances", "transfer_allow_death").args(&["dest", "value"])],
            5,
        );

        assert!(matches!(results.hits[0].matched_in, MatchedIn::Argument));
        assert!(results.hits[0].missing.is_empty());
    }

    #[test]
    fn matched_in_prefers_name_then_argument_then_docs() {
        let items = [call("Balances", "transfer_all")
            .args(&["dest"])
            .docs("Transfer funds.")];
        let matched_in = |query| run(query, &items, 5).hits[0].matched_in;

        assert!(matches!(matched_in("transfer dest funds"), MatchedIn::Name));
        assert!(matches!(matched_in("dest funds"), MatchedIn::Argument));
        assert!(matches!(matched_in("funds"), MatchedIn::Docs));
    }

    #[test]
    fn idf_falls_with_frequency_and_stays_positive() {
        assert!(idf(100, 1) > idf(100, 50));
        assert!(idf(100, 100) > 0.0);
    }

    #[test]
    fn idf_counts_items_outside_kinds() {
        let results = run_kinds(
            "zap transfer",
            &[ItemKind::Call],
            &[
                call("Balances", "zap"),
                call("Balances", "zap_transfer"),
                TestItem::new(ItemKind::Event, Some("Balances"), "Transfer"),
                TestItem::new(ItemKind::Event, Some("Assets"), "Transfer"),
                TestItem::new(ItemKind::Event, Some("Nfts"), "Transfer"),
            ],
            5,
        );

        // Three of the four items with `transfer` are events: were N to count the calls alone,
        // `transfer` would be on more items than N, and its IDF would turn negative.
        assert_eq!(names(&results), ["zap_transfer", "zap"]);
    }

    #[test]
    fn rare_term_outweighs_common_term() {
        let results = run(
            "add zap",
            &[
                call("Balances", "add_x"),
                call("Balances", "add_y"),
                call("Balances", "zap_z"),
            ],
            5,
        );

        assert_eq!(names(&results), ["zap_z", "add_x", "add_y"]);
    }

    #[test]
    fn documentation_mentions_saturate() {
        let results = run(
            "transfer",
            &[
                call("Balances", "set_balance").docs(&"transfer ".repeat(40)),
                call("Balances", "transfer"),
            ],
            5,
        );

        assert_eq!(names(&results), ["transfer", "set_balance"]);
    }

    #[test]
    fn repeated_term_counts_each_time_but_is_listed_once() {
        let items = [
            call("Balances", "transfer_all"),
            call("Balances", "keep_alive"),
        ];

        // Equal scores, so the name decides, until `transfer` is typed twice.
        assert_eq!(
            names(&run("transfer alive", &items, 5)),
            ["keep_alive", "transfer_all"]
        );
        assert_eq!(
            names(&run("transfer transfer alive", &items, 5)),
            ["transfer_all", "keep_alive"]
        );

        let results = run("nonsense Nonsense transfer", &items, 5);

        assert_eq!(results.unmatched, ["nonsense"]);
        assert_eq!(results.hits[0].missing, ["nonsense"]);
    }

    #[test]
    fn name_outscores_documentation() {
        let results = run(
            "transfer",
            &[
                call("Balances", "set_balance").docs("Transfer funds."),
                call("Balances", "transfer_all"),
            ],
            5,
        );

        assert_eq!(names(&results), ["transfer_all", "set_balance"]);
    }

    #[test]
    fn argument_falls_between_name_and_documentation() {
        let results = run(
            "dest",
            &[
                call("Balances", "teleport").docs("Sends to dest."),
                call("Balances", "transfer").args(&["dest"]),
                call("Balances", "set_dest"),
            ],
            5,
        );

        assert_eq!(names(&results), ["set_dest", "transfer", "teleport"]);
    }

    #[test]
    fn container_matches_on_its_own_name() {
        let results = run("balances", &[pallet("Balances")], 5);

        assert_eq!(names(&results), ["Balances"]);
    }

    #[test]
    fn container_narrows_but_does_not_select() {
        let items = [call("Balances", "transfer_all")];

        assert!(run("balances", &items, 5).hits.is_empty());

        let results = run("balances transfer", &items, 5);

        // Both terms count; what the container cannot do is select on its own.
        assert_eq!(names(&results), ["transfer_all"]);
        assert!(results.hits[0].missing.is_empty());
    }

    #[test]
    fn container_outweighs_documentation_mention() {
        let results = run(
            "staking bond",
            &[
                call("NominationPools", "bond").docs("Bond funds into a staking pool."),
                call("Staking", "bond"),
            ],
            5,
        );

        assert_eq!(results.hits[0].item.namespace.as_deref(), Some("Staking"));
    }

    #[test]
    fn container_counts_only_words_name_lacks() {
        let results = run("proxy", &[call("Proxy", "proxy"), pallet("Proxy")], 5);

        // The scores are equal, so the kind decides.
        assert_eq!(results.hits[0].item.kind, ItemKind::Pallet);
    }

    #[test]
    fn coverage_counts_matched_name_tokens() {
        let mut whole = NameCoverage::default();
        whole.record(true);

        let mut part = NameCoverage::default();
        part.record(true);
        part.record(false);
        part.record(false);

        assert!((whole.share() - 1.0).abs() < 1e-12);
        assert!((part.share() - 1.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn coverage_ranks_fully_matched_name_first() {
        let results = run(
            "transfer",
            &[
                call("Balances", "transfer_allow_death"),
                TestItem::new(ItemKind::Event, Some("Balances"), "Transfer"),
            ],
            5,
        );

        assert_eq!(names(&results), ["Transfer", "transfer_allow_death"]);
    }

    #[test]
    fn equal_scores_follow_kind_then_address() {
        let results = run(
            "transfer",
            &[
                TestItem::new(ItemKind::Event, Some("Balances"), "Transfer"),
                call("Indices", "transfer"),
                call("Balances", "transfer"),
            ],
            5,
        );

        let order: Vec<_> = results
            .hits
            .iter()
            .map(|hit| (hit.item.kind, hit.item.namespace.as_deref()))
            .collect();

        assert_eq!(
            order,
            [
                (ItemKind::Call, Some("Balances")),
                (ItemKind::Call, Some("Indices")),
                (ItemKind::Event, Some("Balances")),
            ]
        );
    }

    #[test]
    fn scores_round_to_tenth_of_percent_of_best() {
        let order = |balances: usize, assets: usize| {
            let results = run(
                "transfer",
                &[
                    call("Balances", "transfer").docs(&"transfer ".repeat(balances)),
                    call("Assets", "transfer").docs(&"transfer ".repeat(assets)),
                ],
                5,
            );

            results
                .hits
                .iter()
                .map(|hit| hit.item.namespace.clone().unwrap())
                .collect::<Vec<_>>()
        };

        // 0.03% apart: the same score once rounded, so the fixed order decides.
        assert_eq!(order(41, 40), ["Assets", "Balances"]);
        // 0.27% apart: the better score comes first.
        assert_eq!(order(1, 0), ["Balances", "Assets"]);
    }

    #[test]
    fn missing_lists_terms_hit_lacks() {
        let results = run("transfer nonsense", &[call("Balances", "transfer_all")], 5);

        assert_eq!(names(&results), ["transfer_all"]);
        assert_eq!(results.hits[0].missing, ["nonsense"]);
    }

    #[test]
    fn missing_is_empty_for_hit_with_every_term() {
        let results = run(
            "transfer all",
            &[
                call("Balances", "transfer_keep_alive"),
                call("Balances", "transfer_all"),
            ],
            5,
        );

        assert_eq!(names(&results), ["transfer_all", "transfer_keep_alive"]);
        assert!(results.hits[0].missing.is_empty());
        assert_eq!(results.hits[1].missing, ["all"]);
    }

    #[test]
    fn unmatched_lists_terms_that_hit_nothing() {
        let results = run("transfer nonsense", &[call("Balances", "transfer_all")], 5);

        assert_eq!(results.unmatched, ["nonsense"]);
    }

    #[test]
    fn unmatched_ignores_kinds() {
        let results = run_kinds(
            "transfer",
            &[ItemKind::Event],
            &[call("Balances", "transfer_all")],
            5,
        );

        assert!(results.hits.is_empty());
        assert!(results.unmatched.is_empty());
    }

    #[test]
    fn limit_cuts_hits_and_caps_at_hundred() {
        let items: Vec<TestItem> = (0..150)
            .map(|i| call("Balances", &format!("transfer_{i}")))
            .collect();

        assert_eq!(run("transfer", &items, 3).hits.len(), 3);
        assert_eq!(run("transfer", &items, 1000).hits.len(), 100);
        assert!(run("transfer", &items, 0).hits.is_empty());
    }
}
