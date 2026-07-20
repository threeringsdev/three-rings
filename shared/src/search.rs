//! The v1 query parser (specs/catalog-search.md → V1 syntax subset).
//!
//! A flat AND of terms: whitespace-separated, double quotes group phrases,
//! `-` prefixes negate a term, comma = OR within one term's values (the rail
//! multi-select micro-extension). No `or`, no parentheses. Anything the
//! grammar doesn't recognize is a [`ParseError`] naming the offending term —
//! never a silently-dropped filter.
//!
//! Pure and dependency-free: this is the TDD core; SQL emission lives in the
//! hosted backend (`app/src/search/sql.rs`), which is the only half that needs
//! sqlx. The grammar itself is shared because the filter rail edits the same
//! query text client-side (specs/catalog-search.md "One filter state, two
//! views over it").
//!
//! Two entry points, and the difference matters:
//!
//! - [`parse`] — the AST, for translation. Lossy on purpose: `type:` and `t:`
//!   collapse to the same [`Pred`].
//! - [`parse_tokens`] — the AST *paired with the raw token it came from*, for
//!   the rail. Rewriting a query has to leave every term the rail doesn't own
//!   byte-for-byte intact (the spec's "unrecognized terms preserved verbatim"),
//!   and re-serializing from a lossy AST would quietly rewrite the user's
//!   `type:` into `t:` and their `"quotes"` away.

/// Comparison operator on numeric terms (`mv:`/`cmc:`); `:` means equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

/// One parsed predicate (before negation).
#[derive(Debug, Clone, PartialEq)]
pub enum Pred {
    /// Bare word, quoted phrase, or `name:` — name substring.
    Name(String),
    /// `o:` / `oracle:` / `text:` — oracle-text substring (all faces).
    OracleText(String),
    /// `t:` / `type:` — type_line substrings, comma-OR.
    ///
    /// Comma-OR here is this task's grammar extension: the rail's Type facet
    /// is a multi-select (wireframes: Creature/Instant/Sorcery/Artifact/
    /// Enchantment checkboxes), and flat syntax has no other way to say
    /// "instant OR sorcery" — which is exactly the case catalog-search's
    /// comma micro-extension exists for. It was specified only on `s:`/`r:`
    /// because those were the facets that existed when the grammar shipped.
    TypeLine(Vec<String>),
    /// `s:` / `set:` / `e:` — set codes, comma-OR. Printing-scoped.
    Set(Vec<String>),
    /// `r:` / `rarity:` — rarities, comma-OR. Printing-scoped.
    Rarity(Vec<String>),
    /// `c:` / `color:` — card (or any face) has ALL these colors (WUBRG).
    Colors(Vec<char>),
    /// `c:colorless`.
    Colorless,
    /// `id:` / `identity:` — color identity fits WITHIN these colors.
    Identity(Vec<char>),
    /// `mv:` / `cmc:` with a comparison.
    ManaValue(Cmp, f64),
}

/// One term of the flat AND.
#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub negated: bool,
    pub pred: Pred,
}

/// A parse failure, always naming the offending input so the UI error doubles
/// as syntax discovery.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseError {
    #[error("unknown search term {0:?} — v1 supports name words, o:, t:, s:, r:, c:, id:, mv:")]
    UnknownKey(String),
    #[error("operator not supported in {0:?} — comparisons only work on mv:/cmc:")]
    BadOperator(String),
    #[error("bad value in {0:?}")]
    BadValue(String),
    #[error("unclosed quote in query")]
    UnclosedQuote,
}

/// Parse a query string into the flat term list. Empty input is a valid,
/// empty query (browse-all).
pub fn parse(q: &str) -> Result<Vec<Term>, ParseError> {
    Ok(parse_tokens(q)?.into_iter().map(|t| t.term).collect())
}

/// One token of the query, kept next to what it parsed to.
#[derive(Debug, Clone, PartialEq)]
pub struct RawTerm {
    /// Exactly the characters the user typed for this term — quotes, alias
    /// spelling, and letter case included. This is what gets re-emitted for
    /// every term the rail does not own.
    pub raw: String,
    pub term: Term,
}

/// Parse, keeping each term's original text (see the module docs for why the
/// rail needs this and [`parse`] does not).
pub fn parse_tokens(q: &str) -> Result<Vec<RawTerm>, ParseError> {
    tokenize(q)?
        .into_iter()
        .map(|raw| term_from(raw.clone()).map(|term| RawTerm { raw, term }))
        .collect()
}

/// Quote a value for re-insertion into a query if it needs it.
///
/// Whitespace has to be quoted or the term splits into two on the next parse.
/// A comma must *not* be escaped away — it is the OR separator, and callers
/// that mean OR pass the values separately.
pub fn quote_value(v: &str) -> String {
    if v.chars().any(char::is_whitespace) {
        format!("\"{}\"", v.replace('"', ""))
    } else {
        v.to_string()
    }
}

/// Serialize `key:a,b,c` from a facet's selected values, quoting as needed.
/// Returns `None` for an empty selection — the caller drops the term entirely
/// rather than emitting a valueless `key:`, which the grammar rejects.
pub fn facet_term(key: &str, values: &[String]) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let joined = values
        .iter()
        .map(|v| quote_value(v))
        .collect::<Vec<_>>()
        .join(",");
    Some(format!("{key}:{joined}"))
}

/// Whitespace-split respecting double quotes; tokens keep their raw text
/// (quotes included) so errors can name exactly what the user typed.
fn tokenize(q: &str) -> Result<Vec<String>, ParseError> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in q.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if in_quotes {
        return Err(ParseError::UnclosedQuote);
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    Ok(tokens)
}

fn unquote(s: &str) -> String {
    s.trim_matches('"').to_string()
}

/// Comma-split a term's values (the rail's multi-select OR), lowercased.
fn csv(raw: &str, val: &str) -> Result<Vec<String>, ParseError> {
    let vals: Vec<String> = val
        .split(',')
        .map(|v| unquote(v.trim()).to_ascii_lowercase())
        .collect();
    if vals.iter().any(String::is_empty) {
        return Err(ParseError::BadValue(raw.to_string()));
    }
    Ok(vals)
}

/// WUBRG letters (any case, deduped, order preserved).
fn color_letters(raw: &str, val: &str) -> Result<Vec<char>, ParseError> {
    let mut out = Vec::new();
    if val.is_empty() {
        return Err(ParseError::BadValue(raw.to_string()));
    }
    for ch in val.chars() {
        let up = ch.to_ascii_uppercase();
        if !"WUBRG".contains(up) {
            return Err(ParseError::BadValue(raw.to_string()));
        }
        if !out.contains(&up) {
            out.push(up);
        }
    }
    Ok(out)
}

fn term_from(raw: String) -> Result<Term, ParseError> {
    let (negated, body) = match raw.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, raw.as_str()),
    };
    // A leading quote means the whole token is a name phrase, colons and all.
    let op_pos = if body.starts_with('"') {
        None
    } else {
        body.find([':', '=', '<', '>'])
    };
    let Some(pos) = op_pos else {
        return Ok(Term {
            negated,
            pred: Pred::Name(unquote(body)),
        });
    };

    let key = body[..pos].to_ascii_lowercase();
    let rest = &body[pos..];
    let (cmp, is_ordering, val) = if let Some(v) = rest.strip_prefix("<=") {
        (Cmp::Le, true, v)
    } else if let Some(v) = rest.strip_prefix(">=") {
        (Cmp::Ge, true, v)
    } else if let Some(v) = rest.strip_prefix('<') {
        (Cmp::Lt, true, v)
    } else if let Some(v) = rest.strip_prefix('>') {
        (Cmp::Gt, true, v)
    } else if let Some(v) = rest.strip_prefix('=') {
        (Cmp::Eq, false, v)
    } else {
        (Cmp::Eq, false, &rest[1..]) // ':'
    };

    let numeric = matches!(key.as_str(), "mv" | "cmc");
    let known = matches!(
        key.as_str(),
        "name"
            | "o"
            | "oracle"
            | "text"
            | "t"
            | "type"
            | "s"
            | "set"
            | "e"
            | "r"
            | "rarity"
            | "c"
            | "color"
            | "id"
            | "identity"
            | "mv"
            | "cmc"
    );
    if !known {
        return Err(ParseError::UnknownKey(raw.clone()));
    }
    if is_ordering && !numeric {
        return Err(ParseError::BadOperator(raw.clone()));
    }
    if val.is_empty() {
        return Err(ParseError::BadValue(raw.clone()));
    }

    let pred = match key.as_str() {
        "name" => Pred::Name(unquote(val)),
        "o" | "oracle" | "text" => Pred::OracleText(unquote(val)),
        "t" | "type" => Pred::TypeLine(csv(&raw, val)?),
        "s" | "set" | "e" => Pred::Set(csv(&raw, val)?),
        "r" | "rarity" => Pred::Rarity(csv(&raw, val)?),
        "c" | "color" => {
            let v = unquote(val);
            if v.eq_ignore_ascii_case("colorless") || v.eq_ignore_ascii_case("c") {
                Pred::Colorless
            } else {
                Pred::Colors(color_letters(&raw, &v)?)
            }
        }
        "id" | "identity" => Pred::Identity(color_letters(&raw, &unquote(val))?),
        "mv" | "cmc" => Pred::ManaValue(
            cmp,
            val.parse::<f64>()
                .map_err(|_| ParseError::BadValue(raw.clone()))?,
        ),
        _ => unreachable!("key already validated"),
    };
    Ok(Term { negated, pred })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(pred: Pred) -> Term {
        Term {
            negated: false,
            pred,
        }
    }

    fn neg(pred: Pred) -> Term {
        Term {
            negated: true,
            pred,
        }
    }

    #[test]
    fn empty_query_is_browse_all() {
        assert_eq!(parse("").unwrap(), vec![]);
        assert_eq!(parse("   ").unwrap(), vec![]);
    }

    #[test]
    fn bare_words_are_anded_name_terms() {
        assert_eq!(
            parse("lightning bolt").unwrap(),
            vec![
                t(Pred::Name("lightning".into())),
                t(Pred::Name("bolt".into()))
            ]
        );
    }

    #[test]
    fn quoted_phrase_is_one_name_term() {
        assert_eq!(
            parse("\"lightning bolt\"").unwrap(),
            vec![t(Pred::Name("lightning bolt".into()))]
        );
    }

    #[test]
    fn key_aliases_normalize() {
        assert_eq!(parse("t:instant").unwrap(), parse("type:instant").unwrap());
        assert_eq!(parse("o:flying").unwrap(), parse("oracle:flying").unwrap());
        assert_eq!(parse("o:flying").unwrap(), parse("text:flying").unwrap());
        assert_eq!(parse("s:mh3").unwrap(), parse("set:mh3").unwrap());
        assert_eq!(parse("s:mh3").unwrap(), parse("e:mh3").unwrap());
        assert_eq!(parse("r:rare").unwrap(), parse("rarity:rare").unwrap());
        assert_eq!(parse("c:ur").unwrap(), parse("color:ur").unwrap());
        assert_eq!(parse("id:wu").unwrap(), parse("identity:wu").unwrap());
        assert_eq!(parse("mv:3").unwrap(), parse("cmc:3").unwrap());
        assert_eq!(
            parse("name:bolt").unwrap(),
            vec![t(Pred::Name("bolt".into()))]
        );
    }

    #[test]
    fn quoted_values_carry_spaces() {
        assert_eq!(
            parse("o:\"draw a card\"").unwrap(),
            vec![t(Pred::OracleText("draw a card".into()))]
        );
    }

    #[test]
    fn comma_is_or_within_a_term() {
        assert_eq!(
            parse("s:mh3,lea r:rare,mythic t:instant,sorcery").unwrap(),
            vec![
                t(Pred::Set(vec!["mh3".into(), "lea".into()])),
                t(Pred::Rarity(vec!["rare".into(), "mythic".into()])),
                t(Pred::TypeLine(vec!["instant".into(), "sorcery".into()])),
            ]
        );
    }

    #[test]
    fn parse_tokens_keeps_the_text_the_user_typed() {
        // The rail rewrites its own terms and re-emits everything else
        // verbatim, so the alias spelling, the case, and the quoting all have
        // to survive the round trip — the AST alone has lost all three.
        let toks = parse_tokens("type:Instant -id:wu o:\"draw a card\"").unwrap();
        let raws: Vec<&str> = toks.iter().map(|t| t.raw.as_str()).collect();
        assert_eq!(raws, vec!["type:Instant", "-id:wu", "o:\"draw a card\""]);
        assert_eq!(toks[0].term, t(Pred::TypeLine(vec!["instant".into()])));
    }

    #[test]
    fn facet_terms_serialize_back_into_the_grammar() {
        assert_eq!(
            facet_term("r", &["rare".into(), "mythic".into()]),
            Some("r:rare,mythic".into())
        );
        // A value with a space must come back quoted or it splits into two
        // terms on the next parse — the round trip is the whole contract.
        let term = facet_term("t", &["legendary creature".into()]).unwrap();
        assert_eq!(term, "t:\"legendary creature\"");
        assert_eq!(
            parse(&term).unwrap(),
            vec![t(Pred::TypeLine(vec!["legendary creature".into()]))]
        );
        // An empty selection is no term at all — `t:` alone is a parse error.
        assert_eq!(facet_term("t", &[]), None);
    }

    #[test]
    fn colors_parse_as_wubrg_letters() {
        assert_eq!(
            parse("c:ur").unwrap(),
            vec![t(Pred::Colors(vec!['U', 'R']))]
        );
        assert_eq!(
            parse("c:WUBRG").unwrap(),
            vec![t(Pred::Colors(vec!['W', 'U', 'B', 'R', 'G']))]
        );
        assert_eq!(parse("c:colorless").unwrap(), vec![t(Pred::Colorless)]);
        assert_eq!(
            parse("id:wu").unwrap(),
            vec![t(Pred::Identity(vec!['W', 'U']))]
        );
    }

    #[test]
    fn mana_value_operators() {
        assert_eq!(
            parse("mv:3").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Eq, 3.0))]
        );
        assert_eq!(
            parse("mv=3").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Eq, 3.0))]
        );
        assert_eq!(
            parse("mv<=2").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Le, 2.0))]
        );
        assert_eq!(
            parse("mv>=2").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Ge, 2.0))]
        );
        assert_eq!(
            parse("cmc>4").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Gt, 4.0))]
        );
        assert_eq!(
            parse("mv<1").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Lt, 1.0))]
        );
        assert_eq!(
            parse("mv:0.5").unwrap(),
            vec![t(Pred::ManaValue(Cmp::Eq, 0.5))]
        );
    }

    #[test]
    fn dash_negates_any_term() {
        assert_eq!(
            parse("-t:instant -goblin").unwrap(),
            vec![
                neg(Pred::TypeLine(vec!["instant".into()])),
                neg(Pred::Name("goblin".into()))
            ]
        );
    }

    #[test]
    fn a_realistic_mixed_query() {
        assert_eq!(
            parse("t:instant c:ur mv<=2 -s:mh3 \"fire // ice\"").unwrap(),
            vec![
                t(Pred::TypeLine(vec!["instant".into()])),
                t(Pred::Colors(vec!['U', 'R'])),
                t(Pred::ManaValue(Cmp::Le, 2.0)),
                neg(Pred::Set(vec!["mh3".into()])),
                t(Pred::Name("fire // ice".into())),
            ]
        );
    }

    #[test]
    fn unknown_keys_error_naming_the_term() {
        assert_eq!(
            parse("pow>3").unwrap_err(),
            ParseError::UnknownKey("pow>3".into())
        );
        assert_eq!(
            parse("is:commander").unwrap_err(),
            ParseError::UnknownKey("is:commander".into())
        );
        // `or` is a bare word by our grammar — but a likely boolean-grammar
        // attempt; it parses as a name term (documented: no boolean grammar).
        assert_eq!(parse("or").unwrap(), vec![t(Pred::Name("or".into()))]);
    }

    #[test]
    fn operators_on_non_numeric_keys_error() {
        assert_eq!(
            parse("t>creature").unwrap_err(),
            ParseError::BadOperator("t>creature".into())
        );
    }

    #[test]
    fn bad_values_error_naming_the_term() {
        assert_eq!(
            parse("c:xyz").unwrap_err(),
            ParseError::BadValue("c:xyz".into())
        );
        assert_eq!(
            parse("mv:abc").unwrap_err(),
            ParseError::BadValue("mv:abc".into())
        );
        assert_eq!(parse("c:").unwrap_err(), ParseError::BadValue("c:".into()));
    }

    #[test]
    fn unclosed_quote_errors() {
        assert_eq!(parse("o:\"draw a").unwrap_err(), ParseError::UnclosedQuote);
    }
}
