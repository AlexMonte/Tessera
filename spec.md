authored_board
    -> validated_board
    -> grouped_container_exprs
    -> pattern_streams
    -> transformed_streams
    -> pattern_ir ;


program =
    root_flow_graph ;

root_flow_graph =
    flow ;

flow =
    pattern_source transform_chain? output_tile ;

pattern_source =
    container_chain ;

container_chain =
    root_container
  | root_container "->" container_chain ;

root_container =
    container_tile ;

container_tile =
    container_kind container_stack ;

container_kind =
    "sequence"
  | "alternate"
  | "layer" ;

container_stack =
    atom_expr* ;

atom_expr =
    musical_value atom_modifier_chain? ;

musical_value =
    note_atom
  | rest_atom
  | nested_container ;

nested_container =
    container_tile ;

atom_modifier_chain =
    atom_modifier+ ;

atom_modifier =
    atom_operator scalar_atom ;

atom_operator =
    "@"
  | "*"
  | "/" ;

note_atom =
    Note ;

rest_atom =
    Rest ;

scalar_atom =
    Number ;

transform_chain =
    transform_tile+ ;

transform_tile =
    "slow" transform_args
  | "fast" transform_args
  | "attack" transform_args
  | "gain" transform_args
  | "transpose" transform_args
  | "rev" transform_args ;

transform_args =
    scalar_atom* ;

output_tile =
    Output ;
