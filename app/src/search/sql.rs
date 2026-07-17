//! Term list → SQL predicates (specs/catalog-search.md → Query → SQL).
//!
//! Appends ANDed predicates to the hosted search QueryBuilder — bind
//! parameters only, never spliced values. Printing-scoped terms (`s:`/`r:`)
//! share one EXISTS; negated ones get their own NOT EXISTS.

use sqlx::{Postgres, QueryBuilder};

use super::parse::{Cmp, Pred, Term};

/// Append `AND (…)` predicates for every term to `qb` (which must already
/// hold a complete `WHERE`-bearing prefix, e.g. `… WHERE true`).
pub fn apply(qb: &mut QueryBuilder<'_, Postgres>, terms: &[Term]) {
    // All POSITIVE printing-scoped terms share one EXISTS, so `s:mh3 r:common`
    // means one printing satisfying both (Scryfall's per-printing semantics).
    let mut shared_printing: Vec<&Pred> = Vec::new();
    for term in terms {
        match (&term.pred, term.negated) {
            (Pred::Set(_) | Pred::Rarity(_), false) => shared_printing.push(&term.pred),
            (Pred::Set(_) | Pred::Rarity(_), true) => {
                // Each negated printing-scoped term is its own NOT EXISTS
                // ("has no printing matching this"), independent of the rest.
                qb.push(" AND NOT ");
                push_printing_exists(qb, &[&term.pred]);
            }
            _ => push_card_pred(qb, term),
        }
    }
    if !shared_printing.is_empty() {
        qb.push(" AND ");
        push_printing_exists(qb, &shared_printing);
    }
}

fn push_printing_exists(qb: &mut QueryBuilder<'_, Postgres>, preds: &[&Pred]) {
    qb.push(
        "EXISTS (SELECT 1 FROM printings p JOIN sets st ON st.id = p.set_id \
         WHERE p.oracle_id = c.oracle_id",
    );
    for pred in preds {
        match pred {
            Pred::Set(codes) => {
                qb.push(" AND st.code = ANY(");
                qb.push_bind(codes.clone());
                qb.push(")");
            }
            Pred::Rarity(rarities) => {
                qb.push(" AND p.rarity = ANY(");
                qb.push_bind(rarities.clone());
                qb.push(")");
            }
            _ => unreachable!("only printing-scoped preds reach here"),
        }
    }
    qb.push(")");
}

fn push_card_pred(qb: &mut QueryBuilder<'_, Postgres>, term: &Term) {
    qb.push(" AND ");
    if term.negated {
        qb.push("NOT (");
    }
    match &term.pred {
        Pred::Name(v) => {
            qb.push("c.name ILIKE ");
            qb.push_bind(pattern(v));
        }
        Pred::OracleText(v) => {
            // The generated column is already lowercased; LIKE + a lowered
            // bind beats ILIKE here (same semantics, cheaper).
            qb.push("c.oracle_search_text LIKE ");
            qb.push_bind(pattern(&v.to_lowercase()));
        }
        Pred::TypeLine(v) => {
            qb.push("c.type_line ILIKE ");
            qb.push_bind(pattern(v));
        }
        Pred::Colors(colors) => {
            // Top-level colors is empty on multi-face layouts by design; the
            // jsonb containment probe implements Scryfall's "any single face
            // has them all".
            let arr: Vec<String> = colors.iter().map(char::to_string).collect();
            let probe = serde_json::json!([{ "colors": arr }]);
            qb.push("(c.colors @> ");
            qb.push_bind(arr.clone());
            // coalesce is load-bearing: card_faces IS NULL on single-face
            // cards, and `NULL @> probe` would poison the whole predicate to
            // NULL — silently dropping every single-face row from negated
            // color terms (caught by the live dev-catalog test).
            qb.push(" OR coalesce(c.card_faces, '[]'::jsonb) @> ");
            qb.push_bind(probe);
            qb.push(")");
        }
        Pred::Colorless => {
            qb.push(
                "(c.colors = '{}' AND NOT jsonb_path_exists(\
                 coalesce(c.card_faces, '[]'::jsonb), '$[*].colors[*]'))",
            );
        }
        Pred::Identity(colors) => {
            let arr: Vec<String> = colors.iter().map(char::to_string).collect();
            qb.push("c.color_identity <@ ");
            qb.push_bind(arr);
        }
        Pred::ManaValue(cmp, n) => {
            qb.push(match cmp {
                Cmp::Eq => "c.cmc = ",
                Cmp::Lt => "c.cmc < ",
                Cmp::Le => "c.cmc <= ",
                Cmp::Gt => "c.cmc > ",
                Cmp::Ge => "c.cmc >= ",
            });
            qb.push_bind(*n);
            qb.push("::numeric");
        }
        Pred::Set(_) | Pred::Rarity(_) => unreachable!("handled in apply"),
    }
    if term.negated {
        qb.push(")");
    }
}

/// `%…%` substring pattern with LIKE wildcards escaped so typed text is
/// always literal.
fn pattern(v: &str) -> String {
    format!("%{}%", escape_like(v))
}

/// Escape LIKE/ILIKE wildcards in user input (`%`, `_`, and the default `\`
/// escape itself) so typed text is always literal.
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::super::parse::parse;
    use super::*;

    fn sql_for(q: &str) -> String {
        let mut qb: QueryBuilder<'_, Postgres> = QueryBuilder::new("WHERE true");
        apply(&mut qb, &parse(q).unwrap());
        qb.sql().to_string()
    }

    #[test]
    fn name_terms_use_trgm_ilike() {
        let sql = sql_for("bolt");
        assert!(sql.contains("c.name ILIKE"), "got: {sql}");
    }

    #[test]
    fn oracle_text_uses_the_generated_search_column() {
        let sql = sql_for("o:flying");
        assert!(sql.contains("c.oracle_search_text LIKE"), "got: {sql}");
    }

    #[test]
    fn type_line_uses_ilike() {
        let sql = sql_for("t:instant");
        assert!(sql.contains("c.type_line ILIKE"), "got: {sql}");
    }

    #[test]
    fn positive_printing_terms_share_one_exists() {
        let sql = sql_for("s:mh3 r:common");
        assert_eq!(
            sql.matches("EXISTS (SELECT 1 FROM printings").count(),
            1,
            "one printing must satisfy ALL positive printing-scoped terms — got: {sql}"
        );
        assert!(sql.contains("st.code = ANY"), "got: {sql}");
        assert!(sql.contains("p.rarity = ANY"), "got: {sql}");
    }

    #[test]
    fn negated_printing_terms_get_their_own_not_exists() {
        let sql = sql_for("s:mh3 -s:lea");
        assert_eq!(
            sql.matches("EXISTS (SELECT 1 FROM printings").count(),
            2,
            "got: {sql}"
        );
        assert_eq!(sql.matches("NOT EXISTS").count(), 1, "got: {sql}");
    }

    #[test]
    fn colors_check_card_and_faces() {
        let sql = sql_for("c:ur");
        assert!(sql.contains("c.colors @>"), "got: {sql}");
        // MUST be null-safe: card_faces IS NULL on single-face cards, and a
        // bare `NULL @> probe` poisons the whole predicate to NULL — which
        // silently drops every single-face row from negated color terms.
        assert!(
            sql.contains("coalesce(c.card_faces, '[]'::jsonb) @>"),
            "got: {sql}"
        );
    }

    #[test]
    fn colorless_requires_no_colors_anywhere() {
        let sql = sql_for("c:colorless");
        assert!(sql.contains("c.colors = '{}'"), "got: {sql}");
        assert!(sql.contains("jsonb_path_exists"), "got: {sql}");
    }

    #[test]
    fn identity_is_contained_within() {
        let sql = sql_for("id:wu");
        assert!(sql.contains("c.color_identity <@"), "got: {sql}");
    }

    #[test]
    fn mana_value_compares_cmc() {
        assert!(sql_for("mv<=2").contains("c.cmc <= "));
        assert!(sql_for("mv:3").contains("c.cmc = "));
        assert!(sql_for("cmc>4").contains("c.cmc > "));
    }

    #[test]
    fn negation_wraps_not() {
        let sql = sql_for("-t:instant");
        assert!(sql.contains("NOT (c.type_line ILIKE"), "got: {sql}");
    }

    #[test]
    fn like_wildcards_in_user_input_are_escaped() {
        // A "%" typed by the user must not become a wildcard: the bind value
        // is escaped, so the SQL text just carries a bind slot — assert the
        // escaping helper here instead.
        assert_eq!(super::escape_like("100%_\\"), "100\\%\\_\\\\");
    }
}
