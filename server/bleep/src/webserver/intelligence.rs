use std::sync::Arc;

use super::prelude::*;
use crate::{
    indexes::{reader::ContentDocument, Indexes},
    intelligence::{code_navigation, Language, NodeKind, ScopeGraph, TSLanguage},
    repo::RepoRef,
    snippet::{Snipper, Snippet},
    symbol::SymbolLocations,
    text_range::TextRange,
};

use axum::{extract::Query, response::IntoResponse, Extension};
use petgraph::graph::NodeIndex;
use serde::{Deserialize, Serialize};

/// The request made to the `local-intel` endpoint.
#[derive(Debug, Deserialize)]
pub(super) struct TokenInfoRequest {
    /// The repo_ref of the file of interest
    repo_ref: String,

    /// The path to the file of interest, relative to the repo root
    relative_path: String,

    /// The byte range to look for
    start: usize,
    end: usize,
}

/// The response from the `local-intel` endpoint.
#[derive(Serialize, Debug)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(super) enum TokenInfoResponse {
    /// The response returned when the input range is a definition
    Definition {
        /// A file-wise grouping of references, across this repo
        references: Vec<FileSymbols>,
    },
    /// The response returned when the input range is a reference
    Reference {
        /// The original definition(s) for this reference
        definitions: Vec<FileSymbols>,
        /// The other references in this document
        references: Vec<FileSymbols>,
    },
}

impl TokenInfoResponse {
    fn is_empty(&self) -> bool {
        match self {
            Self::Reference {
                definitions,
                references,
            } => definitions.is_empty() && references.is_empty(),
            Self::Definition { references } => references.is_empty(),
        }
    }
}

impl super::ApiResponse for TokenInfoResponse {}

#[derive(Debug, Serialize)]
pub(super) struct FileSymbols {
    // FIXME: choose a better name
    /// The file to which the following occurrences belong
    file: String,

    /// A collection of symbol locations with context in this file
    data: Vec<SymbolOccurrence>,
}

impl FileSymbols {
    fn is_populated(&self) -> bool {
        !self.data.is_empty()
    }
}

/// An occurrence of a single symbol in a document, along with some context
#[derive(Debug, Serialize)]
pub(super) struct SymbolOccurrence {
    /// The precise range of this symbol
    #[serde(flatten)]
    pub(super) range: TextRange,

    /// A few lines of surrounding context
    pub(super) snippet: Snippet,
}

fn handle_definition_local(
    scope_graph: &ScopeGraph,
    idx: NodeIndex<u32>,
    doc: &ContentDocument,
) -> FileSymbols {
    let file = doc.relative_path.clone();
    let handler = code_navigation::CurrentFileHandler {
        scope_graph,
        idx,
        doc,
    };
    let data = handler
        .handle_definition()
        .into_iter()
        .map(|range| to_occurrence(doc, range))
        .collect();
    FileSymbols { file, data }
}

fn handle_definition_repo_wide(
    token: &[u8],
    kind: Option<&str>,
    start_file: &str,
    all_docs: &[ContentDocument],
) -> Vec<FileSymbols> {
    all_docs
        .iter()
        .filter(|doc| doc.relative_path != start_file) // do not look in the current file
        .filter_map(|doc| match &doc.symbol_locations {
            SymbolLocations::TreeSitter(scope_graph) => {
                let file = doc.relative_path.clone();
                let handler = code_navigation::RepoWideHandler {
                    token,
                    kind,
                    scope_graph,
                    doc,
                };
                let data = handler
                    .handle_definition()
                    .into_iter()
                    .map(|range| to_occurrence(doc, range))
                    .collect();
                Some(FileSymbols { file, data })
            }
            _ => None,
        })
        .collect()
}

fn handle_reference_local(
    scope_graph: &ScopeGraph,
    idx: NodeIndex<u32>,
    doc: &ContentDocument,
) -> (FileSymbols, FileSymbols) {
    let file = &doc.relative_path;
    let handler = code_navigation::CurrentFileHandler {
        scope_graph,
        idx,
        doc,
    };
    let (defs, refs) = handler.handle_reference();
    let def_data = FileSymbols {
        file: file.clone(),
        data: defs
            .into_iter()
            .map(|range| to_occurrence(doc, range))
            .collect(),
    };
    let ref_data = FileSymbols {
        file: file.clone(),
        data: refs
            .into_iter()
            .map(|range| to_occurrence(doc, range))
            .collect(),
    };

    (def_data, ref_data)
}

fn handle_reference_repo_wide(
    token: &[u8],
    kind: Option<&str>,
    start_file: &str,
    all_docs: &[ContentDocument],
) -> (Vec<FileSymbols>, Vec<FileSymbols>) {
    all_docs
        .iter()
        .filter(|doc| doc.relative_path != start_file) // do not look in the current file
        .filter_map(|doc| match &doc.symbol_locations {
            SymbolLocations::TreeSitter(scope_graph) => {
                let file = doc.relative_path.clone();
                let handler = code_navigation::RepoWideHandler {
                    token,
                    kind,
                    scope_graph,
                    doc,
                };
                let (defs, refs) = handler.handle_reference();

                let def_data = FileSymbols {
                    file: file.clone(),
                    data: defs
                        .into_iter()
                        .map(|range| to_occurrence(doc, range))
                        .collect(),
                };
                let ref_data = FileSymbols {
                    file,
                    data: refs
                        .into_iter()
                        .map(|range| to_occurrence(doc, range))
                        .collect(),
                };

                Some((def_data, ref_data))
            }
            _ => None,
        })
        .unzip()
}

fn to_occurrence(doc: &ContentDocument, range: TextRange) -> SymbolOccurrence {
    let src = &doc.content;
    let line_end_indices = &doc.line_end_indices;
    let highlight = range.start.byte..range.end.byte;
    let snippet = Snipper::default()
        .expand(highlight, src, line_end_indices)
        .reify(src, &[]);

    SymbolOccurrence { range, snippet }
}

// helper to merge two sets of file-symbols and omit the empty results
fn merge(
    a: impl IntoIterator<Item = FileSymbols>,
    b: impl IntoIterator<Item = FileSymbols>,
) -> Vec<FileSymbols> {
    a.into_iter()
        .chain(b.into_iter())
        .filter(FileSymbols::is_populated)
        .collect()
}

pub(super) async fn handle(
    Query(payload): Query<TokenInfoRequest>,
    Extension(indexes): Extension<Arc<Indexes>>,
) -> Result<impl IntoResponse> {
    let repo_ref = &payload.repo_ref.parse::<RepoRef>().map_err(Error::user)?;

    let content = indexes
        .file
        .by_path(repo_ref, &payload.relative_path)
        .await
        .map_err(Error::user)?;
    let hovered_range = payload.start..payload.end;
    let hovered_text = &content.content.as_str()[hovered_range.clone()];

    let scope_graph = match content.symbol_locations {
        SymbolLocations::TreeSitter(ref graph) => graph,
        _ => return Err(Error::user("Intelligence is unavailable for this language")),
    };

    let node = scope_graph.node_by_range(payload.start, payload.end);

    let idx = match node {
        None => {
            return search_nav(
                Arc::clone(&indexes),
                repo_ref,
                hovered_text,
                hovered_range,
                content.lang.clone(),
            )
            .await
            .map(json);
        }
        Some(idx) => idx,
    };

    let src = &content.content;
    let current_file = &content.relative_path;
    let kind = scope_graph.symbol_name_of(idx);
    let lang = content.lang.as_deref();

    let associated_langs = match lang.map(TSLanguage::from_id) {
        Some(Language::Supported(config)) => config.language_ids,
        _ => &[],
    };

    let all_docs = indexes
        .file
        .by_repo(repo_ref, associated_langs.iter())
        .await;

    match &scope_graph.graph[idx] {
        // we are already at a def
        // - find refs from the current file
        // - find refs from other files
        NodeKind::Def(d) => {
            // fetch local references with scope-graphs
            let local_references = handle_definition_local(scope_graph, idx, &content);

            // fetch repo-wide references with trivial search, only if the def is
            // a top-level def (typically functions, ADTs, consts)
            let repo_wide_references = if scope_graph.is_top_level(idx) {
                let token = d.name(src.as_bytes());
                handle_definition_repo_wide(token, kind, current_file, &all_docs)
            } else {
                vec![]
            };

            // merge the two
            let references = merge([local_references], repo_wide_references);

            let response = TokenInfoResponse::Definition { references };

            if response.is_empty() {
                search_nav(
                    Arc::clone(&indexes),
                    repo_ref,
                    hovered_text,
                    hovered_range,
                    content.lang.clone(),
                )
                .await
                .map(json)
            } else {
                Ok(json(response))
            }
        }

        // we are at a reference:
        // - find def from the current file
        // - find defs from other files
        // - find refs from the current file
        // - find refs from other files
        //
        // the ordering here prefers occurrences from the current file, over occurrences
        // from other files.
        NodeKind::Ref(r) => {
            // fetch local (defs, refs) with scope-graphs
            let (local_definitions, local_references) =
                handle_reference_local(scope_graph, idx, &content);

            // fetch repo-wide (defs, refs) with trivial search
            let token = r.name(src.as_bytes());
            let (repo_wide_definitions, repo_wide_references) =
                handle_reference_repo_wide(token, kind, current_file, &all_docs);

            // if we already have a local-def, do not search globally
            let definitions = if local_definitions.data.is_empty() {
                merge([], repo_wide_definitions)
            } else {
                merge([local_definitions], [])
            };
            let references = merge([local_references], repo_wide_references);

            let response = TokenInfoResponse::Reference {
                definitions,
                references,
            };

            if response.is_empty() {
                search_nav(
                    Arc::clone(&indexes),
                    repo_ref,
                    hovered_text,
                    hovered_range,
                    content.lang.clone(),
                )
                .await
                .map(json)
            } else {
                Ok(json(response))
            }
        }

        // we are at an import:
        //
        // an import is in an of itself a "definition" of a name in the current tree,
        // it is the first time a name is introduced in a tree. however the original
        // definition of this name can only exist in other trees (or not exist at all).
        NodeKind::Import(i) => {
            // fetch local refs with scope-graph
            let local_references = handle_definition_local(scope_graph, idx, &content);

            // fetch repo-wide defs with trivial search
            let token = i.name(src.as_bytes());
            let (repo_wide_definitions, repo_wide_references) =
                handle_reference_repo_wide(token, kind, current_file, &all_docs);

            let definitions = repo_wide_definitions
                .into_iter()
                .filter(FileSymbols::is_populated)
                .collect();

            let references = merge([local_references], repo_wide_references);

            let response = TokenInfoResponse::Reference {
                definitions,
                references,
            };

            if response.is_empty() {
                search_nav(
                    Arc::clone(&indexes),
                    repo_ref,
                    hovered_text,
                    hovered_range,
                    content.lang.clone(),
                )
                .await
                .map(json)
            } else {
                Ok(json(response))
            }
        }
        _ => Err(Error::user(
            "provided range is not eligible for intelligence",
        )),
    }
}

async fn search_nav(
    indexes: Arc<Indexes>,
    repo_ref: &RepoRef,
    hovered_text: &str,
    payload_range: std::ops::Range<usize>,
    lang: Option<String>,
) -> Result<TokenInfoResponse> {
    use crate::{
        indexes::{reader::ContentReader, DocumentRead},
        query::compiler::trigrams,
    };
    use tantivy::{
        collector::TopDocs,
        query::{BooleanQuery, TermQuery},
        schema::{IndexRecordOption, Term},
    };

    let associated_langs = match lang.as_deref().map(TSLanguage::from_id) {
        Some(Language::Supported(config)) => config.language_ids,
        _ => &[],
    };

    // produce search based results here
    let target = regex::Regex::new(&format!(r"\b{hovered_text}\b")).expect("failed to build regex");
    // perform a text search for hovered_text
    let file_source = &indexes.file.source;
    let indexer = &indexes.file;
    let query = {
        let repo_filter = Term::from_field_text(indexer.source.repo_ref, &repo_ref.to_string());
        let terms = trigrams(hovered_text)
            .map(|token| Term::from_field_text(indexer.source.content, token.as_str()))
            .map(|term| {
                Box::new(TermQuery::new(term, IndexRecordOption::Basic))
                    as Box<dyn tantivy::query::Query>
            })
            .chain(std::iter::once(
                Box::new(TermQuery::new(repo_filter, IndexRecordOption::Basic))
                    as Box<dyn tantivy::query::Query>,
            ))
            .chain(std::iter::once(Box::new(BooleanQuery::union(
                associated_langs
                    .iter()
                    .map(|l| {
                        Term::from_field_bytes(
                            indexer.source.lang,
                            l.to_ascii_lowercase().as_bytes(),
                        )
                    })
                    .map(|l| {
                        Box::new(TermQuery::new(l, IndexRecordOption::Basic))
                            as Box<dyn tantivy::query::Query>
                    })
                    .collect::<Vec<_>>(),
            ))
                as Box<dyn tantivy::query::Query>))
            .collect::<Vec<Box<dyn tantivy::query::Query>>>();
        BooleanQuery::intersection(terms)
    };
    let collector = TopDocs::with_limit(500);
    let reader = indexes.file.reader.read().await;
    let searcher = reader.searcher();
    let results = searcher
        .search(&query, &collector)
        .expect("failed to search index");

    // classify search results into defs and refs
    let (definitions, references): (Vec<_>, Vec<_>) = results
        .into_iter()
        .map(|(_, doc_addr)| {
            let retrieved_doc = searcher
                .doc(doc_addr)
                .expect("failed to get document by address");
            let doc = ContentReader.read_document(file_source, retrieved_doc);
            let hoverable_ranges = doc.hoverable_ranges().unwrap(); // infallible
            let (defs, refs): (Vec<_>, Vec<_>) = target
                .find_iter(&doc.content)
                .map(|m| TextRange::from_byte_range(m.range(), &doc.line_end_indices))
                .filter(|range| hoverable_ranges.iter().any(|r| r.contains(range)))
                .filter(|range| {
                    !(payload_range.start >= range.start.byte
                        && payload_range.end <= range.end.byte)
                })
                .map(|range| {
                    let start_byte = range.start.byte;
                    let end_byte = range.end.byte;
                    let is_def = doc
                        .symbol_locations
                        .scope_graph()
                        .and_then(|graph| {
                            graph
                                .node_by_range(start_byte, end_byte)
                                .map(|idx| matches!(graph.graph[idx], NodeKind::Def(_)))
                        })
                        .unwrap_or_default();
                    let highlight = start_byte..end_byte;
                    let snippet = Snipper::default()
                        .expand(highlight, &doc.content, &doc.line_end_indices)
                        .reify(&doc.content, &[]);
                    (is_def, SymbolOccurrence { range, snippet })
                })
                .partition(|(is_def, _)| *is_def);

            let mut defs = defs
                .into_iter()
                .map(|(_, occurrence)| occurrence)
                .collect::<Vec<_>>();
            defs.sort_by_key(|k| k.range);

            let mut refs = refs
                .into_iter()
                .map(|(_, occurrence)| occurrence)
                .collect::<Vec<_>>();
            refs.sort_by_key(|k| k.range);

            let file = doc.relative_path;

            let def_symbols = FileSymbols {
                file: file.clone(),
                data: defs,
            };

            let ref_symbols = FileSymbols { file, data: refs };

            (def_symbols, ref_symbols)
        })
        .unzip();

    Ok(TokenInfoResponse::Reference {
        definitions: definitions
            .into_iter()
            .filter(FileSymbols::is_populated)
            .collect(),
        references: references
            .into_iter()
            .filter(FileSymbols::is_populated)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_range::Point;

    #[test]
    fn serialize_response_reference() {
        let expected = serde_json::json!({
            "kind": "reference",
            "definitions": [{
                "file": "server/bleep/src/symbol.rs",
                "data": [{
                    "start": { "byte": 2620, "line": 90, "column": 0  },
                    "end": { "byte": 2627, "line": 90, "column": 0  },
                    "snippet": {
                        "highlights": [ { "start": 12, "end": 19 } ],
                        "data": "        let indexes = Indexes::new(self.clone(), threads).await?;\n",
                        "line_range": { "start": 91, "end": 92 },
                        "symbols": [],
                    }
                }]
            }],
            "references": [{
                "file": "server/bleep/src/intelligence/scope_resolution.rs",
                "data": [{
                    "start": { "byte": 2725, "line": 93, "column": 0  },
                    "end": { "byte": 2732, "line": 93, "column": 0  },
                    "snippet": {
                        "highlights": [ { "start": 12, "end": 19 } ],
                        "symbols": [],
                        "line_range": { "start": 94, "end": 95 },
                        "data": "            indexes.reindex().await?;\n"
                    }
                }]
            }]
        });

        let observed = serde_json::to_value(TokenInfoResponse::Reference {
            definitions: vec![FileSymbols {
                file: "server/bleep/src/symbol.rs".into(),
                data: vec![SymbolOccurrence {
                    range: TextRange {
                        start: Point {
                            byte: 2620,
                            line: 90,
                            column: 0,
                        },
                        end: Point {
                            byte: 2627,
                            line: 90,
                            column: 0,
                        },
                    },
                    snippet: Snippet {
                        line_range: 91..92,
                        data: "        let indexes = Indexes::new(self.clone(), threads).await?;\n"
                            .to_owned(),
                        highlights: vec![12..19],
                        symbols: vec![],
                    },
                }],
            }],
            references: vec![FileSymbols {
                file: "server/bleep/src/intelligence/scope_resolution.rs".into(),
                data: vec![SymbolOccurrence {
                    range: TextRange {
                        start: Point {
                            byte: 2725,
                            line: 93,
                            column: 0,
                        },
                        end: Point {
                            byte: 2732,
                            line: 93,
                            column: 0,
                        },
                    },
                    snippet: Snippet {
                        line_range: 94..95,
                        data: "            indexes.reindex().await?;\n".to_owned(),
                        highlights: vec![12..19],
                        symbols: vec![],
                    },
                }],
            }],
        })
        .unwrap();

        pretty_assertions::assert_eq!(expected, observed)
    }

    #[test]
    fn serialize_response_definition() {
        let expected = serde_json::json!({
            "kind": "definition",
            "references": [{
                "file": "server/bleep/benches/snippets.rs",
                "data": [{
                    "start": { "byte": 2725, "line": 93, "column": 0 },
                    "end": { "byte": 2732, "line": 93, "column": 0  },
                    "snippet": {
                        "highlights": [ { "start": 12, "end": 19 } ],
                        "line_range": { "start": 94, "end": 95 },
                        "data": "            indexes.reindex().await?;\n",
                        "symbols": []
                    }
                }]
            }]
        });

        let observed = serde_json::to_value(TokenInfoResponse::Definition {
            references: vec![FileSymbols {
                file: "server/bleep/benches/snippets.rs".into(),
                data: vec![SymbolOccurrence {
                    range: TextRange {
                        start: Point {
                            byte: 2725,
                            line: 93,
                            column: 0,
                        },
                        end: Point {
                            byte: 2732,
                            line: 93,
                            column: 0,
                        },
                    },
                    snippet: Snippet {
                        line_range: 94..95,
                        data: "            indexes.reindex().await?;\n".to_owned(),
                        highlights: vec![12..19],
                        symbols: vec![],
                    },
                }],
            }],
        })
        .unwrap();

        assert_eq!(expected, observed)
    }
}
