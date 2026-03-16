use std::collections::BTreeMap;
use std::sync::Arc;

use crate::piece::{Piece, PieceDef};

pub struct PieceRegistry {
    pieces: BTreeMap<String, Arc<dyn Piece>>,
}

impl PieceRegistry {
    pub fn new() -> Self {
        Self {
            pieces: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, piece: impl Piece + 'static) {
        let id = piece.def().id.clone();
        self.pieces.insert(id, Arc::new(piece));
    }

    /// Register a pre-wrapped `Arc<dyn Piece>` with an explicit id.
    pub fn register_arc(&mut self, id: String, piece: Arc<dyn Piece>) {
        self.pieces.insert(id, piece);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Piece>> {
        self.pieces.get(id).cloned()
    }

    pub fn all_defs(&self) -> Vec<PieceDef> {
        self.pieces
            .values()
            .map(|piece| piece.def().clone())
            .collect()
    }

    pub fn visible_defs(&self, namespace: &str) -> Vec<PieceDef> {
        self.pieces
            .values()
            .filter_map(|piece| {
                let def = piece.def();
                def.is_visible_in_namespace(namespace).then(|| def.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use super::PieceRegistry;
    use crate::ast::Expr;
    use crate::piece::{Piece, PieceDef, PieceInputs};
    use crate::types::{PieceCategory, PieceSemanticKind, TileSide};

    struct TestPiece {
        def: PieceDef,
    }

    impl Piece for TestPiece {
        fn def(&self) -> &PieceDef {
            &self.def
        }

        fn compile(&self, _inputs: &PieceInputs, _inline_params: &BTreeMap<String, Value>) -> Expr {
            Expr::nil()
        }
    }

    #[test]
    fn visible_defs_returns_core_matching_namespace_and_tricks() {
        let mut registry = PieceRegistry::new();
        registry.register(TestPiece {
            def: PieceDef {
                id: "core.not".into(),
                label: "not".into(),
                category: PieceCategory::Transform,
                semantic_kind: PieceSemanticKind::Operator,
                namespace: "core".into(),
                params: vec![],
                output_type: Some("bool".into()),
                output_side: Some(TileSide::RIGHT),
                description: None,
            },
        });
        registry.register(TestPiece {
            def: PieceDef {
                id: "strudel.fast".into(),
                label: "fast".into(),
                category: PieceCategory::Transform,
                semantic_kind: PieceSemanticKind::Intrinsic,
                namespace: "strudel".into(),
                params: vec![],
                output_type: Some("pattern".into()),
                output_side: Some(TileSide::RIGHT),
                description: None,
            },
        });
        registry.register(TestPiece {
            def: PieceDef {
                id: "user.twist".into(),
                label: "twist".into(),
                category: PieceCategory::Trick,
                semantic_kind: PieceSemanticKind::Trick,
                namespace: "user".into(),
                params: vec![],
                output_type: Some("any".into()),
                output_side: Some(TileSide::RIGHT),
                description: None,
            },
        });

        let visible = registry.visible_defs("strudel");
        let ids = visible
            .iter()
            .map(|def| def.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["core.not", "strudel.fast", "user.twist"]);

        let visible = registry.visible_defs("lua");
        let ids = visible
            .iter()
            .map(|def| def.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["core.not", "user.twist"]);
    }
}
