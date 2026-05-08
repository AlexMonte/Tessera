use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::piece::{Piece, PieceDef};

pub struct PieceRegistry {
    pieces: BTreeMap<String, Arc<dyn Piece>>,
    duplicate_piece_ids: BTreeSet<String>,
}

impl Default for PieceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PieceRegistry {
    pub fn new() -> Self {
        Self {
            pieces: BTreeMap::new(),
            duplicate_piece_ids: BTreeSet::new(),
        }
    }

    pub fn register(&mut self, piece: impl Piece + 'static) {
        let id = piece.def().id.clone();
        if self.pieces.insert(id.clone(), Arc::new(piece)).is_some() {
            self.duplicate_piece_ids.insert(id);
        }
    }

    /// Register a pre-wrapped `Arc<dyn Piece>` with an explicit id.
    pub fn register_arc(&mut self, id: String, piece: Arc<dyn Piece>) {
        if self.pieces.insert(id.clone(), piece).is_some() {
            self.duplicate_piece_ids.insert(id);
        }
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

    pub fn search_by_tag(&self, tag: &str) -> Vec<PieceDef> {
        self.pieces
            .values()
            .filter_map(|piece| {
                let def = piece.def();
                def.tags
                    .iter()
                    .any(|candidate| candidate == tag)
                    .then(|| def.clone())
            })
            .collect()
    }

    pub fn search_by_tag_in_namespace(&self, tag: &str, namespace: &str) -> Vec<PieceDef> {
        self.pieces
            .values()
            .filter_map(|piece| {
                let def = piece.def();
                (def.is_visible_in_namespace(namespace)
                    && def.tags.iter().any(|candidate| candidate == tag))
                .then(|| def.clone())
            })
            .collect()
    }

    pub(crate) fn duplicate_piece_ids(&self) -> impl Iterator<Item = &String> {
        self.duplicate_piece_ids.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::PieceRegistry;
    use crate::piece::{Piece, PieceDef};
    use crate::types::{PieceCategory, PieceSemanticKind, TileSide};

    struct TestPiece {
        def: PieceDef,
    }

    impl Piece for TestPiece {
        fn def(&self) -> &PieceDef {
            &self.def
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["logic".into(), "boolean".into()],
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["math".into()],
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["math".into(), "favorite".into()],
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

    #[test]
    fn search_by_tag_returns_matching_defs() {
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["logic".into(), "boolean".into()],
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["timing".into()],
            },
        });

        let ids = registry
            .search_by_tag("logic")
            .into_iter()
            .map(|def| def.id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["core.not"]);
    }

    #[test]
    fn search_by_tag_in_namespace_uses_visibility_rules() {
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["logic".into()],
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["logic".into()],
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
                output_role: Default::default(),
                temporal_kind: Default::default(),
                fan_in: Default::default(),
                fan_out: Default::default(),
                description: None,
                tags: vec!["logic".into()],
            },
        });

        let strudel_ids = registry
            .search_by_tag_in_namespace("logic", "strudel")
            .into_iter()
            .map(|def| def.id)
            .collect::<Vec<_>>();
        assert_eq!(strudel_ids, vec!["core.not", "strudel.fast", "user.twist"]);

        let lua_ids = registry
            .search_by_tag_in_namespace("logic", "lua")
            .into_iter()
            .map(|def| def.id)
            .collect::<Vec<_>>();
        assert_eq!(lua_ids, vec!["core.not", "user.twist"]);
    }
}
