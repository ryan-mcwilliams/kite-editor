//! Language registry and tree-sitter syntax highlighting.
//!
//! To add a language: add an entry to `langs()` with its file extensions, LSP
//! language id, comment token and (optionally) a tree-sitter grammar + query.

use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Token {
    Comment,
    String,
    Keyword,
    Function,
    Type,
    Constant,
    Property,
    Operator,
    Punct,
    Variable,
    Tag,
    Attribute,
    Title,
    Escape,
}

pub struct Grammar {
    pub language: Language,
    pub query: Query,
    tokens: Vec<Option<Token>>,
}

pub struct Lang {
    /// LSP language identifier.
    pub id: &'static str,
    pub exts: &'static [&'static str],
    pub files: &'static [&'static str],
    pub comment: &'static str,
    make: Option<fn() -> (Language, String)>,
    grammar: OnceLock<Option<Grammar>>,
}

impl Lang {
    /// The grammar is built on first use, so startup stays instant.
    pub fn grammar(&self) -> Option<&Grammar> {
        self.grammar
            .get_or_init(|| {
                let (language, src) = (self.make?)();
                let query = Query::new(&language, &src).ok()?;
                let tokens = query.capture_names().iter().map(|n| token_for(n)).collect();
                Some(Grammar {
                    language,
                    query,
                    tokens,
                })
            })
            .as_ref()
    }
}

fn lang(
    id: &'static str,
    exts: &'static [&'static str],
    files: &'static [&'static str],
    comment: &'static str,
    make: Option<fn() -> (Language, String)>,
) -> Lang {
    Lang {
        id,
        exts,
        files,
        comment,
        make,
        grammar: OnceLock::new(),
    }
}

pub fn langs() -> &'static [Lang] {
    static LANGS: OnceLock<Vec<Lang>> = OnceLock::new();
    LANGS.get_or_init(|| {
        use tree_sitter_javascript as js;
        use tree_sitter_typescript as ts;
        vec![
            lang(
                "rust",
                &["rs"],
                &[],
                "//",
                Some(|| {
                    (
                        tree_sitter_rust::LANGUAGE.into(),
                        tree_sitter_rust::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "python",
                &["py", "pyi"],
                &[],
                "#",
                Some(|| {
                    (
                        tree_sitter_python::LANGUAGE.into(),
                        tree_sitter_python::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "javascript",
                &["js", "mjs", "cjs", "jsx"],
                &[],
                "//",
                Some(|| {
                    (
                        js::LANGUAGE.into(),
                        format!("{}\n{}", js::HIGHLIGHT_QUERY, js::JSX_HIGHLIGHT_QUERY),
                    )
                }),
            ),
            lang(
                "typescript",
                &["ts", "mts", "cts"],
                &[],
                "//",
                Some(|| {
                    (
                        ts::LANGUAGE_TYPESCRIPT.into(),
                        format!("{}\n{}", js::HIGHLIGHT_QUERY, ts::HIGHLIGHTS_QUERY),
                    )
                }),
            ),
            lang(
                "typescriptreact",
                &["tsx"],
                &[],
                "//",
                Some(|| {
                    (
                        ts::LANGUAGE_TSX.into(),
                        format!(
                            "{}\n{}\n{}",
                            js::HIGHLIGHT_QUERY,
                            js::JSX_HIGHLIGHT_QUERY,
                            ts::HIGHLIGHTS_QUERY
                        ),
                    )
                }),
            ),
            lang(
                "go",
                &["go"],
                &[],
                "//",
                Some(|| {
                    (
                        tree_sitter_go::LANGUAGE.into(),
                        tree_sitter_go::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "json",
                &["json", "jsonc"],
                &[],
                "",
                Some(|| {
                    (
                        tree_sitter_json::LANGUAGE.into(),
                        tree_sitter_json::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "toml",
                &["toml"],
                &["Cargo.lock"],
                "#",
                Some(|| {
                    (
                        tree_sitter_toml_ng::LANGUAGE.into(),
                        tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "yaml",
                &["yaml", "yml"],
                &[],
                "#",
                Some(|| {
                    (
                        tree_sitter_yaml::LANGUAGE.into(),
                        tree_sitter_yaml::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "shellscript",
                &["sh", "bash", "zsh"],
                &[".zshrc", ".bashrc", ".profile"],
                "#",
                Some(|| {
                    (
                        tree_sitter_bash::LANGUAGE.into(),
                        tree_sitter_bash::HIGHLIGHT_QUERY.into(),
                    )
                }),
            ),
            lang(
                "c",
                &["c", "h"],
                &[],
                "//",
                Some(|| {
                    (
                        tree_sitter_c::LANGUAGE.into(),
                        tree_sitter_c::HIGHLIGHT_QUERY.into(),
                    )
                }),
            ),
            lang("cpp", &["cpp", "cc", "hpp", "cxx"], &[], "//", None),
            lang(
                "css",
                &["css"],
                &[],
                "",
                Some(|| {
                    (
                        tree_sitter_css::LANGUAGE.into(),
                        tree_sitter_css::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "html",
                &["html", "htm"],
                &[],
                "",
                Some(|| {
                    (
                        tree_sitter_html::LANGUAGE.into(),
                        tree_sitter_html::HIGHLIGHTS_QUERY.into(),
                    )
                }),
            ),
            lang(
                "markdown",
                &["md", "markdown"],
                &[],
                "",
                Some(|| {
                    (
                        tree_sitter_md::LANGUAGE.into(),
                        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK.into(),
                    )
                }),
            ),
        ]
    })
}

pub fn lang_for_path(path: &Path) -> Option<&'static Lang> {
    let name = path.file_name()?.to_str()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    langs()
        .iter()
        .find(|l| l.files.contains(&name) || (!ext.is_empty() && l.exts.contains(&ext)))
}

/// Map a tree-sitter capture name (e.g. `function.method`) to a theme token.
fn token_for(name: &str) -> Option<Token> {
    use Token::*;
    let has = |s: &str| name.contains(s);
    Some(match name.split('.').next().unwrap_or("") {
        "comment" => Comment,
        "string" | "character" if has("escape") => Escape,
        "string" | "character" => String,
        "escape" => Escape,
        "keyword" | "include" | "conditional" | "repeat" | "exception" => Keyword,
        "function" | "method" | "constructor" => Function,
        "type" | "namespace" | "module" => Type,
        "number" | "float" | "boolean" | "constant" | "label" => Constant,
        "property" | "field" => Property,
        "variable" if has("builtin") => Property,
        "variable" => Variable,
        "operator" => Operator,
        "punctuation" | "delimiter" => Punct,
        "tag" => Tag,
        "attribute" => Attribute,
        "text" | "markup" if has("title") || has("heading") => Title,
        "text" | "markup" if has("literal") || has("raw") => String,
        "text" | "markup" if has("uri") || has("link") || has("reference") => Function,
        _ => return None,
    })
}

pub struct Syntax {
    pub tree: Tree,
    pub text: String,
    pub version: i32,
}

impl Syntax {
    pub fn parse(g: &Grammar, text: String, version: i32) -> Option<Syntax> {
        let mut parser = Parser::new();
        parser.set_language(&g.language).ok()?;
        let tree = parser.parse(&text, None)?;
        Some(Syntax {
            tree,
            text,
            version,
        })
    }

    /// Highlight spans (byte ranges) intersecting `range`, ordered so that
    /// painting them in sequence gives the right result: outer nodes first,
    /// and for the same node, later query patterns win.
    pub fn spans(&self, g: &Grammar, range: Range<usize>) -> Vec<(usize, usize, Token)> {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut out = vec![];
        let mut matches = cursor.matches(&g.query, self.tree.root_node(), self.text.as_bytes());
        while let Some(m) = matches.next() {
            for c in m.captures() {
                if let Some(tok) = g.tokens[c.index as usize] {
                    let r = c.node.byte_range();
                    out.push((r.start, r.end, tok, m.pattern_index));
                }
            }
        }
        out.sort_by(|a, b| (b.1 - b.0).cmp(&(a.1 - a.0)).then(a.3.cmp(&b.3)));
        out.into_iter().map(|(s, e, t, _)| (s, e, t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grammar_and_query_loads() {
        for l in langs() {
            if l.make.is_some() {
                assert!(
                    l.grammar().is_some(),
                    "highlight query failed to compile for {}",
                    l.id
                );
            }
        }
    }

    #[test]
    fn rust_keywords_strings_and_comments_are_highlighted() {
        let l = lang_for_path(Path::new("x.rs")).unwrap();
        let g = l.grammar().unwrap();
        let src = "fn main() { let s = \"hi\"; } // done";
        let syn = Syntax::parse(g, src.into(), 0).unwrap();
        let spans = syn.spans(g, 0..src.len());
        let tok = |needle: &str| {
            let at = src.find(needle).unwrap();
            spans
                .iter()
                .rev()
                .find(|s| s.0 <= at && at < s.1)
                .map(|s| s.2)
        };
        assert_eq!(tok("fn"), Some(Token::Keyword));
        assert_eq!(tok("main"), Some(Token::Function));
        assert_eq!(tok("\"hi\""), Some(Token::String));
        assert_eq!(tok("// done"), Some(Token::Comment));
    }
}
